/* Copyright (c) 2021-2025 Richard Rodger, MIT License */

// The engine is the tabnas parser; jsonic supplies the relaxed-JSON
// grammar that the embedded grammar text is authored in.
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

// Engine types used by plugins (re-exported by @tabnas/parser).
import {
  Rule,
  RuleSpec,
  Plugin,
  Context,
  Config,
  TabnasOptions,
  Lex,
} from '@tabnas/parser'

// A parsed XML element.
//
// Fields:
//   name       - qualified name as written in the source (e.g. "ns:tag")
//   prefix     - namespace prefix if any ("ns"), else undefined
//   localName  - local part of the qualified name ("tag")
//   namespace  - URI bound to the prefix/default at parse time
//   attributes - attribute map, with entity references decoded. Namespace
//                declarations ("xmlns", "xmlns:*") are kept here too.
//   children   - mixed array of text strings and nested elements.
type XmlElement = {
  name: string
  prefix?: string
  localName: string
  namespace?: string
  // Effective xml:space (XML 1.0 §2.10). Present only when the
  // element or an ancestor sets xml:space to something other than
  // the default value "default" (typically "preserve").
  space?: string
  // Effective xml:lang (XML 1.0 §2.12). Present only when the
  // element or an ancestor specifies xml:lang.
  lang?: string
  attributes: Record<string, string>
  children: Array<XmlElement | string>
}

type XmlOptions = {
  // Whether to resolve namespaces (annotate elements with
  // `prefix`/`localName`/`namespace`). Default: true.
  namespaces: boolean
  // Whether to decode the five predefined entities and numeric character
  // references in text and attribute values. Default: true.
  entities: boolean
  // Additional named entities to recognise beyond the five predefined ones.
  customEntities: Record<string, string>
  // Whether to enforce XML 1.0 §4.1 — every named entity reference must
  // resolve to a declared entity (predefined, customEntities, or a DOCTYPE
  // <!ENTITY> declaration). Default: true. When set to false, references
  // to unknown names are left as-is in the output (legacy behaviour
  // useful for templating).
  strictEntities: boolean
  // Whether to enforce the Namespaces in XML 1.0 constraint that every
  // prefix used on an element or attribute name be bound by an
  // in-scope `xmlns:prefix` declaration. Default: false.
  //
  // XML 1.0 well-formedness and Namespaces in XML are separate specs:
  // `<a><foo:b/></a>` is a well-formed XML document (`foo:b` is a legal
  // Name), it is merely not *namespace*-well-formed. The parser's job
  // is XML 1.0, so an unbound prefix is reported only when this option
  // is turned on. Either way the element keeps its `prefix` and
  // `localName`; an unbound prefix simply leaves `namespace` unset.
  strictNamespaces: boolean
  // Embed mode. When `false` (default), the plugin configures the parser
  // for pure-XML input: the start rule becomes `xml`, JSON structural
  // tokens are disabled, and all non-XML lexing is turned off.
  //
  // When `true`, the plugin leaves Jsonic's JSON/JSONIC rules in place
  // and adds an alternate to the `val` rule so that a literal XML
  // element (`<tag>…</tag>` or `<tag/>`) appears wherever Jsonic
  // expects a value. The XML literal is parsed with the same element
  // grammar used in pure mode.
  embed: boolean
}

// --- BEGIN EMBEDDED xml-grammar.jsonic ---
const grammarText = `
# XML Grammar Definition (elements + attributes + mixed content)
# Parsed by a standard Jsonic instance and passed to jsonic.grammar()
# Function references (@ prefixed) are resolved against the refs map
#
# Token naming:
#   #XOP - XML open tag, e.g. <tagname attr="value">
#   #XCL - XML close tag, e.g. </tagname>
#   #XSC - XML self-close tag, e.g. <tagname attr="value"/>
#   #XIG - comment / processing instruction / DOCTYPE (ignored)
#   #TX  - text content between tags (CDATA included)
#   #ZZ  - end of input

{
  rule: xml: open: [
    { s: '#ZZ' }
    { s: '#TX' r: xml a: '@doc-text-open' }
    { p: element c: '@no-root-yet' }
  ]
  rule: xml: close: [
    { s: '#ZZ' g: end }
    { s: '#TX' r: xml a: '@doc-text-close' g: comma }
  ]

  rule: element: open: [
    { s: '#XSC' a: '@element-selfclose' u: { selfclose: 1 } }
    { s: '#XOP' p: content a: '@element-open' }
  ]
  rule: element: close: [
    { c: '@element-is-selfclosed' }
    { s: '#XCL' a: '@element-close' g: close }
  ]

  rule: content: open: [
    { s: '#XCL' b: 1 }
    { p: child }
  ]
  rule: content: close: [
    { s: '#XCL' b: 1 g: close }
    { r: content }
  ]

  rule: child: open: [
    { s: '#TX' a: '@child-text' }
    { s: '#XOP' b: 1 p: element }
    { s: '#XSC' b: 1 p: element }
  ]
}
`
// --- END EMBEDDED xml-grammar.jsonic ---


const Xml: Plugin = (tn: Tabnas, options: XmlOptions) => {
  const embed = options.embed === true
  // Namespace-constraint checking (unbound prefixes) is opt-in — XML
  // 1.0 well-formedness does not require prefixes to be bound.
  const strictNamespaces = options.strictNamespaces === true
  const decodeEntity = buildEntityDecoder(options)

  // Human descriptions for the XML tokens, surfaced in railroad diagram
  // legends (read off the live config by @tabnas/railroad).
  tn.options({
    config: {
      modify: {
        'xml-tokendesc': (cfg: any) => {
          cfg.tokenDesc = Object.assign(cfg.tokenDesc || {}, {
            '#XOP': 'XML open tag <name ...>',
            '#XSC': 'XML self-closing tag <name .../>',
            '#XCL': 'XML closing tag </name>',
            '#XIG': 'ignored markup: comment, PI, or DOCTYPE',
            '#TX': 'text / character data between tags (entities decoded)',
          })
        },
      },
    },
  })

  // Register custom lexer matcher. The same matcher is used in both
  // modes; in embed mode it additionally consumes text between tags so
  // Jsonic's own text/fixed lexers don't split it on `,` `:` etc.
  tn.options({
    lex: {
      match: {
        xmltag: {
          order: 1e5,
          make: buildXmlTagMatcher(decodeEntity, embed, options),
        },
      },
      emptyResult: undefined,
    },
    // Terminate Jsonic text at `<` so XML tag starts are not absorbed
    // into Jsonic text runs.
    ender: ['<'],
  })

  if (!embed) {
    // Pure XML mode: reconfigure the parser so Jsonic's own value
    // grammar is unreachable and all lexers other than our tag matcher
    // are quiescent.
    //
    // Note: we deliberately do NOT install a `text.modify` hook here.
    // While the root element is open the custom matcher itself emits
    // the text tokens (with entity decoding and well-formedness
    // checks); Jsonic's text matcher only sees whitespace before the
    // root and after it, where no decoding is needed.
    tn.options({
      rule: {
        start: 'xml',
        exclude: 'jsonic,imp',
      },
      fixed: {
        token: {
          '#OB': null, '#CB': null, '#OS': null, '#CS': null,
          '#CL': null, '#CA': null,
        },
      },
      tokenSet: {
        IGNORE: ['#SP', '#LN', '#CM', '#XIG'],
      },
      number:  { lex: false },
      value:   { lex: false },
      string:  { lex: false },
      comment: { lex: false },
      space:   { lex: false },
      line:    { lex: false },
      // XML 1.0 §2.1: a well-formed document has exactly ONE document
      // element. Input that parses to no value at all — a prolog on its own,
      // a lone comment, whitespace — satisfies every other rule and then
      // yields `undefined`, which the caller cannot distinguish from a
      // successful parse of nothing. Treat it as the well-formedness error it
      // is. Only in standalone mode: in embed mode this grammar is a fragment
      // inside a jsonic document and has no document element of its own.
      result: { fail: [undefined] },
      // ...and the same rule makes empty input ill-formed. The engine
      // short-circuits `''` before the rule loop, so result.fail never sees
      // it; lex.empty is the switch that governs that path.
      lex: { empty: false },
    })
  } else {
    // Embed mode: keep all of Jsonic's standard grammar. Still register
    // #XIG for comments/PIs/DOCTYPE and add it to IGNORE.
    tn.options({
      tokenSet: {
        IGNORE: ['#SP', '#LN', '#CM', '#XIG'],
      },
    })
  }

  // Error templates and hints are installed in both modes.
  tn.options({
    error: {
      // Error templates interpolate `{key}` against the failing
      // token's `use` details (plus the token/rule/context fields);
      // `$name` is not a placeholder syntax the engine understands, so
      // never use it here — it would be emitted verbatim.
      xml_mismatched_tag:
        'closing tag </{closename}> does not match opening tag <{openname}>',
      xml_invalid_tag: 'invalid tag: {src}',
      unterminated_comment: 'unterminated comment: {src}',
      unterminated_cdata: 'unterminated CDATA section: {src}',
      unterminated_pi: 'unterminated processing instruction: {src}',
      unterminated_doctype: 'unterminated DOCTYPE declaration: {src}',
      comment_double_dash: 'comment body cannot contain "--"',
      cdata_terminator_in_text: 'character data cannot contain "]]>"',
      pi_target_invalid: 'processing instruction target is missing or invalid',
      lt_in_attr_value: '"<" is not allowed in an attribute value',
      bad_entity_ref: 'malformed entity reference (need &name; or &#NNN; or &#xHHH;)',
      duplicate_attribute: 'duplicate attribute name in tag',
      invalid_xml_char: 'illegal control character in XML data',
      reserved_namespace: 'invalid use of a reserved namespace prefix or URI',
      unbound_prefix: 'element or attribute uses an undeclared namespace prefix',
      invalid_namespace_uri: 'namespace name cannot contain white space',
      undeclared_entity: 'reference to undeclared entity',
      unparsed_entity_ref: 'reference to an unparsed (NDATA) entity',
      external_entity_in_attr:
        'attribute value cannot reference an external entity',
      text_at_top_level: 'character data is not allowed outside the root element',
    },
    hint: {
      xml_mismatched_tag: `Each opening tag must be paired with a matching closing tag.
Expected </{openname}> but found </{closename}>.`,
      xml_invalid_tag: `The tag syntax is not valid XML.`,
      unterminated_comment: `The comment starting at this position has no closing "-->".`,
      unterminated_cdata: `The CDATA section starting at this position has no closing "]]>".`,
      unterminated_pi: `The processing instruction starting at this position has no closing "?>".`,
      unterminated_doctype: `The DOCTYPE declaration starting at this position has no closing ">".`,
      comment_double_dash: `XML 1.0 disallows "--" inside a comment body.`,
      cdata_terminator_in_text: `The literal "]]>" must only appear as the end of a CDATA section.`,
      pi_target_invalid: `A processing instruction must start with a Name; the XML declaration <?xml...?> is the special case.`,
      lt_in_attr_value: `Use the entity reference &lt; to include "<" in an attribute value.`,
      bad_entity_ref: `Replace literal "&" with &amp;, or terminate the entity reference with ";".`,
      duplicate_attribute: `Each attribute name in an open tag must be unique.`,
      invalid_xml_char: `Only #x9, #xA, #xD and code points >= #x20 are legal XML characters.`,
      reserved_namespace: `The "xml" prefix is fixed to ${XML_NS_URI}; the "xmlns" prefix cannot be redeclared, and neither URI may be bound to any other prefix or as the default namespace.`,
      unbound_prefix: `Declare the prefix with xmlns:prefix="..." on this element or one of its ancestors.`,
      invalid_namespace_uri: `A namespace name is a URI reference, and a URI reference cannot contain white space; note that a line break inside the declaration becomes a space under attribute-value normalisation.`,
      undeclared_entity: `Declare the entity in the DOCTYPE internal subset, add it to the customEntities option, or set strictEntities: false to allow unresolved references through.`,
      unparsed_entity_ref: `XML 1.0 §4.1 (WFC: Parsed Entity) — an entity declared with an NDATA notation is unparsed, and its name may only appear as the value of an ENTITY-typed attribute, never in an entity reference.`,
      external_entity_in_attr: `XML 1.0 §4.1 (WFC: No External Entity References) — an attribute value may not reference an entity whose replacement text lives in an external file.`,
      text_at_top_level: `An XML document is "prolog element Misc*": outside the single root element only comments, processing instructions and white space may appear.`,
    },
  })

  const refs: Record<string, Function> = {
    '@xml-bc': (r: Rule, ctx: Context) => {
      if (r.child && r.child.node) {
        const root = ctx.root()
        root.node = r.child.node
        // Mark the document as having seen its root so the
        // `@no-root-yet` condition gates any further attempts to
        // push a second root element.
        ctx.u.rootSeen = true
        if (options.namespaces !== false) {
          const nsErr = resolveNamespaces(root.node, {}, strictNamespaces)
          if (nsErr) {
            return ctx.t0.bad(nsErr)
          }
        }
      }
    },

    // Condition: only allow the xml rule to push an `element` if the
    // document hasn't already produced a root (XML 1.0 §2.1).
    '@no-root-yet': (_r: Rule, ctx: Context) => true !== ctx.u.rootSeen,

    // XML 1.0 §2.1 [1] document ::= prolog element Misc* — outside the
    // root element only Misc (comments, PIs, whitespace) may appear, so
    // character data before or after the root is not well-formed.
    '@doc-text-open': (r: Rule, ctx: Context) => checkDocText(r.o0, ctx),
    '@doc-text-close': (r: Rule, ctx: Context) => checkDocText(r.c0, ctx),

    '@element-open': (r: Rule, ctx: Context) => {
      const v = r.o0.val
      r.node = {
        name: v.name,
        localName: v.name,
        attributes: applyAttrDefaults(v.attributes, v.name, ctx),
        children: [],
      }
    },

    '@element-selfclose': (r: Rule, ctx: Context) => {
      const v = r.o0.val
      r.node = {
        name: v.name,
        localName: v.name,
        attributes: applyAttrDefaults(v.attributes, v.name, ctx),
        children: [],
      }
    },

    '@element-close': (r: Rule, _ctx: Context) => {
      const openName = r.node && r.node.name
      const closeName = r.c0.val
      if (openName !== closeName) {
        // Mark the offending close tag itself (not the lookahead
        // token) so the caret lands on `</b>`, and attach the two
        // names as `use` details — `RuleSpec.bad` spreads the bad
        // token's `use` into the error details, which is where the
        // `{openname}` / `{closename}` placeholders resolve from.
        return r.c0.bad('xml_mismatched_tag', {
          openname: openName,
          closename: closeName,
        })
      }
    },

    '@child-text': (r: Rule) => {
      r.node.children.push(r.o0.val)
      r.u.done = true
    },

    '@child-bc': (r: Rule) => {
      if (true !== r.u.done && r.child && r.child.node) {
        r.node.children.push(r.child.node)
      }
    },

    '@element-is-selfclosed': (r: Rule) => true === !!r.u.selfclose,
  }

  // Parse embedded grammar definition and wire refs. The embedded
  // relaxed-.jsonic grammar text is parsed by a jsonic-grammar engine,
  // then installed on this tabnas instance.
  const grammarDef = new Tabnas().use(jsonic).parse(grammarText)
  grammarDef.ref = refs
  tn.grammar(grammarDef)

  if (embed) {
    // Splice XML literals into the Jsonic `val` rule. When the parser
    // is looking for a value and sees an `#XOP` or `#XSC` token, it
    // pushes the `element` rule which builds the XML subtree. Backtrack
    // by 1 so `element.open` can read the same token and dispatch to
    // the correct branch.
    const XOP = tn.token('#XOP')
    const XSC = tn.token('#XSC')
    tn.rule('val', (rs: RuleSpec) => {
      return rs.open(
        [
          { s: [XOP], b: 1, p: 'element', g: 'xml' },
          { s: [XSC], b: 1, p: 'element', g: 'xml' },
        ],
      )
    })

    // In embed mode the top-level wrapper is Jsonic's `val` rule, so
    // the `@xml-bc` hook that copies the root element to `ctx.root().node`
    // is not invoked. Resolve namespaces after the full tree lands on
    // the element rule by hooking its close-state action.
    if (options.namespaces !== false) {
      tn.rule('element', (rs: RuleSpec) => {
        rs.bc((r: Rule) => {
          if (r.node && 'object' === typeof r.node && r.parent &&
              r.parent.name === 'val') {
            resolveNamespaces(r.node, {}, strictNamespaces)
          }
        })
      })
    }
  } else {
    // Pure XML mode: the `xml` start rule reaches only the XML rules
    // (element/content/child), so Jsonic's JSON value rules are now dead.
    // Remove them from the grammar definition so the parser — and the
    // generated railroad diagram — carries only the rules XML actually uses.
    for (const name of ['val', 'map', 'list', 'pair', 'elem']) {
      tn.rule(name, null)
    }
  }
}


// decodeBOM converts a byte sequence (either a Node Buffer / Uint8Array
// or a Latin-1-mapped "binary" JS string where each char code is one
// byte) into a decoded Unicode string, transcoding from whichever of
// UTF-8 / UTF-16-LE / UTF-16-BE / UTF-32-LE / UTF-32-BE the byte-order
// mark indicates. UTF-8 is the default when no BOM is present, so
// non-ASCII tag names in BOM-less UTF-8 files round-trip correctly.
//
// If the caller has already decoded the input to a Unicode JS string
// (any code unit > 0xFF) the function only strips a leading U+FEFF
// and returns the input otherwise unchanged.
//
// Use this when reading XML files of unknown encoding:
//
//   const body = decodeBOM(readFileSync(path))   // Node Buffer
//   const doc = jsonic(body)
function decodeBOM(src: any): string {
  // Already a decoded Unicode string: strip a leading BOM character.
  if (typeof src === 'string') {
    let isBinary = true
    for (let i = 0; i < src.length && i < 1024; i++) {
      if (src.charCodeAt(i) > 0xff) { isBinary = false; break }
    }
    if (!isBinary) {
      return src.charCodeAt(0) === 0xfeff ? src.substring(1) : src
    }
    // Binary string: convert to a byte array and reuse the buffer path.
    const bytes = new Uint8Array(src.length)
    for (let i = 0; i < src.length; i++) bytes[i] = src.charCodeAt(i) & 0xff
    return decodeBOMBytes(bytes)
  }
  // Buffer / Uint8Array / array-like.
  return decodeBOMBytes(src as Uint8Array)
}

function decodeBOMBytes(b: Uint8Array): string {
  const n = b.length
  if (n === 0) return ''

  // UTF-32 BE
  if (n >= 4 && b[0] === 0x00 && b[1] === 0x00 && b[2] === 0xfe && b[3] === 0xff) {
    return decodeUTF32(b, 4, true)
  }
  // UTF-32 LE (check before UTF-16 LE)
  if (n >= 4 && b[0] === 0xff && b[1] === 0xfe && b[2] === 0x00 && b[3] === 0x00) {
    return decodeUTF32(b, 4, false)
  }
  // UTF-16 BE
  if (n >= 2 && b[0] === 0xfe && b[1] === 0xff) {
    return decodeUTF16(b, 2, true)
  }
  // UTF-16 LE
  if (n >= 2 && b[0] === 0xff && b[1] === 0xfe) {
    return decodeUTF16(b, 2, false)
  }
  // UTF-8 BOM, then UTF-8 default
  let start = 0
  if (n >= 3 && b[0] === 0xef && b[1] === 0xbb && b[2] === 0xbf) start = 3
  return decodeUTF8(b, start)
}

function decodeUTF8(b: Uint8Array, start: number): string {
  let out = ''
  let i = start
  const n = b.length
  while (i < n) {
    const c = b[i]
    if (c < 0x80) {
      out += String.fromCharCode(c)
      i++
      continue
    }
    let cp = -1
    let advance = 1
    if ((c & 0xe0) === 0xc0 && i + 1 < n) {
      cp = ((c & 0x1f) << 6) | (b[i + 1] & 0x3f)
      advance = 2
    } else if ((c & 0xf0) === 0xe0 && i + 2 < n) {
      cp = ((c & 0x0f) << 12) | ((b[i + 1] & 0x3f) << 6) | (b[i + 2] & 0x3f)
      advance = 3
    } else if ((c & 0xf8) === 0xf0 && i + 3 < n) {
      cp = ((c & 0x07) << 18) |
        ((b[i + 1] & 0x3f) << 12) |
        ((b[i + 2] & 0x3f) << 6) |
        (b[i + 3] & 0x3f)
      advance = 4
    }
    // Reject malformed sequences (invalid lead byte, truncated tail,
    // or out-of-range code point) by emitting the raw byte and
    // advancing one position. The downstream XML check will then flag
    // the offending control / non-Char character.
    if (cp < 0 || cp > 0x10ffff) {
      out += String.fromCharCode(c)
      i++
    } else {
      out += String.fromCodePoint(cp)
      i += advance
    }
  }
  return out
}

function decodeUTF16(b: Uint8Array, start: number, big: boolean): string {
  const units: number[] = []
  for (let i = start; i + 1 < b.length; i += 2) {
    const a = b[i], c = b[i + 1]
    units.push(big ? (a << 8) | c : (c << 8) | a)
  }
  // Build in chunks: `String.fromCharCode(...units)` overflows the call
  // stack once a document has more than a few tens of thousands of code
  // units (RangeError), which would turn a large UTF-16 document into a
  // crash rather than a parse.
  const CHUNK = 4096
  if (units.length <= CHUNK) return String.fromCharCode(...units)
  let out = ''
  for (let i = 0; i < units.length; i += CHUNK) {
    out += String.fromCharCode(...units.slice(i, i + CHUNK))
  }
  return out
}

function decodeUTF32(b: Uint8Array, start: number, big: boolean): string {
  let out = ''
  for (let i = start; i + 3 < b.length; i += 4) {
    const a = b[i], c = b[i + 1], d = b[i + 2], e = b[i + 3]
    const cp = big
      ? (a << 24) | (c << 16) | (d << 8) | e
      : (e << 24) | (d << 16) | (c << 8) | a
    out += String.fromCodePoint(cp >>> 0)
  }
  return out
}


// The five predefined XML entities.
const predefinedEntities: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
}

// Build an entity decoder. The plugin-time entity map (predefined +
// customEntities) is closed over; per-parse entities declared in the
// DOCTYPE internal subset are passed in via the optional `dtd`
// argument and recursively expanded with cycle detection.
//
// Returned function signature:
//   decode(src, dtd?) -> string
// where `dtd` is a per-parse map { name -> raw value } that the
// matcher pulls from `lex.ctx.u.dtdEntities`.
function buildEntityDecoder(options: XmlOptions) {
  // Allocated without a prototype: `ref` comes from the document, so on a
  // plain literal `baseEntities['__proto__']` is Object.prototype rather than
  // undefined, and the `undefined !== ...` test below then "expands" a
  // declared &__proto__; to "[object Object]".
  const baseEntities: Record<string, string> = Object.assign(
    Object.create(null),
    predefinedEntities,
    options?.customEntities || {},
  )
  const entityRE = /&(#x[0-9a-fA-F]+|#[0-9]+|[A-Za-z_:][A-Za-z0-9_\-\.:]*);/g

  function expand(
    src: string,
    dtd: Record<string, string>,
    seen: Set<string>,
  ): string {
    if (src.indexOf('&') < 0) return src
    return src.replace(entityRE, (match, ref) => {
      if (ref[0] === '#') {
        // Lowercase `x` only, per XML 1.0 [66]; `&#X26;` never
        // reaches here because `entityRE` does not match it (and
        // `checkEntityRefs` rejects it as not well-formed).
        const code =
          ref[1] === 'x'
            ? parseInt(ref.substring(2), 16)
            : parseInt(ref.substring(1), 10)
        if (isNaN(code)) return match
        try {
          return String.fromCodePoint(code)
        } catch {
          return match
        }
      }
      // Predefined / option-supplied entities take precedence over
      // anything declared in the DTD (matches the XML 1.0 rule that
      // the five predefined entities are always available).
      if (undefined !== baseEntities[ref]) return baseEntities[ref]
      if (undefined !== dtd[ref]) {
        if (seen.has(ref)) {
          // Recursive entity reference is a WF violation. Fall through
          // and keep the unexpanded text so the upstream WF check can
          // catch the resulting bare `&` if the caller wants to treat
          // this as an error; here we simply break the cycle.
          return match
        }
        seen.add(ref)
        const out = expand(dtd[ref], dtd, seen)
        seen.delete(ref)
        return out
      }
      return match
    })
  }

  const decoder = function decodeEntities(src: string, dtd?: Record<string, string>): string {
    return expand(src, dtd || {}, new Set())
  } as DecodeEntitiesFn
  decoder.declared = baseEntities
  return decoder
}

type DecodeEntitiesFn = ((src: string, dtd?: Record<string, string>) => string) & {
  declared: Record<string, string>
}

// Parse the body of a DOCTYPE declaration (the text between the `[`
// and `]` of the internal subset) and extract every `<!ATTLIST>`
// declaration's default attribute values, keyed by element name and
// attribute name. Both literal defaults and `#FIXED "value"` defaults
// are returned; `#REQUIRED` and `#IMPLIED` declarations contribute
// nothing because they have no default value.
//
// Used by the matcher's element actions to fill in attributes that
// were not present on the element instance.
function parseDoctypeAttlists(body: string): Record<string, Record<string, string>> {
  const isSpace = (ch: string) =>
    ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r'
  const isUpperAscii = (ch: string) =>
    ch >= 'A' && ch <= 'Z'
  const skipSpace = (s: number): number => {
    while (s < body.length && isSpace(body[s])) s++
    return s
  }
  // Keyed by element name, which the document controls, so it is allocated
  // without a prototype. As a plain `{}` literal, an <!ATTLIST __proto__ ...>
  // declaration made `out['__proto__']` read back Object.prototype - truthy,
  // so the guard below reused it and the very next line wrote the attribute
  // default onto Object.prototype, polluting every object in the process.
  const out: Record<string, Record<string, string>> = Object.create(null)

  let i = 0
  while (i < body.length) {
    const idx = body.indexOf('<!ATTLIST', i)
    if (idx < 0) break
    let j = idx + '<!ATTLIST'.length
    j = skipSpace(j)
    const elemName = readNameInBody(body, j)
    if (!elemName) { i = j + 1; continue }
    j = elemName.end

    // Loop over AttDefs until '>' or EOF.
    while (j < body.length) {
      j = skipSpace(j)
      if (j >= body.length) break
      if (body[j] === '>') { j++; break }

      const attrName = readNameInBody(body, j)
      if (!attrName) { j++; continue }
      j = attrName.end
      j = skipSpace(j)

      // Skip AttType: enumeration `( ... )`, `NOTATION ( ... )`, or
      // a bare type identifier (CDATA, ID, IDREF, IDREFS, NMTOKEN,
      // NMTOKENS, ENTITY, ENTITIES).
      if (body[j] === '(') {
        const close = body.indexOf(')', j)
        if (close < 0) { j = body.length; break }
        j = close + 1
      } else if (body.startsWith('NOTATION', j)) {
        j += 'NOTATION'.length
        j = skipSpace(j)
        if (body[j] === '(') {
          const close = body.indexOf(')', j)
          if (close < 0) { j = body.length; break }
          j = close + 1
        }
      } else {
        while (j < body.length && isUpperAscii(body[j])) j++
      }
      j = skipSpace(j)

      // DefaultDecl.
      if (body.startsWith('#REQUIRED', j)) {
        j += '#REQUIRED'.length
        continue
      }
      if (body.startsWith('#IMPLIED', j)) {
        j += '#IMPLIED'.length
        continue
      }
      if (body.startsWith('#FIXED', j)) {
        j += '#FIXED'.length
        j = skipSpace(j)
      }
      if (body[j] === '"' || body[j] === "'") {
        const quote = body[j]
        j++
        const valStart = j
        while (j < body.length && body[j] !== quote) j++
        if (j >= body.length) break
        const value = body.substring(valStart, j)
        if (!out[elemName.name]) out[elemName.name] = Object.create(null)
        out[elemName.name][attrName.name] = value
        j++
      }
    }
    i = j
  }
  return out
}

// applyAttrDefaults merges in DOCTYPE-supplied default attribute
// values (`<!ATTLIST element attr ... "default">`) for any attribute
// missing from the parsed element instance. Returns the original
// attributes object if no defaults apply.
function applyAttrDefaults(
  attrs: Record<string, string>,
  elemName: string,
  ctx: Context,
): Record<string, string> {
  const defaults = ctx?.u?.dtdAttrDefaults?.[elemName]
  if (!defaults) return attrs
  const out = { ...attrs }
  for (const k of Object.keys(defaults)) {
    if (!Object.prototype.hasOwnProperty.call(out, k)) {
      out[k] = defaults[k]
    }
  }
  return out
}

// checkDocText enforces XML 1.0 §2.1 [1]
// `document ::= prolog element Misc*`: at document level (before the
// prolog's root element, and after it) only Misc — comments, PIs, and
// white space — is allowed. Anything else is character data outside the
// root element and is not well-formed.
//
// Comments/PIs/DOCTYPE arrive as #XIG and are ignored by the token set,
// so the only document-level token to police is #TX.
function checkDocText(tkn: any, ctx: Context): any {
  const val = tkn && tkn.val
  if ('string' !== typeof val) return
  for (let i = 0; i < val.length; i++) {
    const ch = val[i]
    if (' ' !== ch && '\t' !== ch && '\n' !== ch && '\r' !== ch) {
      return tkn.bad ? tkn.bad('text_at_top_level') : ctx.t0.bad('text_at_top_level')
    }
  }
}

// readNameInBody is a free-function counterpart to the matcher's
// `readName` closure used by the DTD parsers, which run before the
// matcher closure has been instantiated.
function readNameInBody(s: string, start: number): { name: string; end: number } | null {
  if (start >= s.length) return null
  const cp0 = s.codePointAt(start)!
  if (!isNameStartCP(cp0)) return null
  let i = start + (cp0 > 0xffff ? 2 : 1)
  while (i < s.length) {
    const cp = s.codePointAt(i)!
    if (!isNameCharCP(cp)) break
    i += cp > 0xffff ? 2 : 1
  }
  return { name: s.substring(start, i), end: i }
}

// Parse the body of a DOCTYPE declaration (the text between the `[`
// and `]` of the internal subset) and extract every general entity
// declaration. Parameter entity declarations (`<!ENTITY % name ...>`)
// and other declarations (`<!ELEMENT`, `<!ATTLIST`, `<!NOTATION`) are
// recognised but skipped.
//
// Two maps come back:
//   internal - `<!ENTITY name "value">`; values are stored verbatim,
//              character and entity references inside an entity value
//              are expanded only when the outer entity is referenced.
//   external - `<!ENTITY name SYSTEM "...">` / `PUBLIC ...`. These
//              entities ARE declared, so a reference to one satisfies
//              the §4.1 "Entity Declared" well-formedness constraint,
//              but we never fetch the replacement text, so the
//              reference is left unexpanded in the output (§4.4.3,
//              "not included": a non-validating processor may skip an
//              external entity).
//   unparsed - external entities carrying an `NDATA` notation. §4.1
//              WFC "Parsed Entity" forbids referencing these at all;
//              they may only appear as ENTITY-typed attribute values.
function parseDoctypeEntities(
  body: string,
): {
  internal: Record<string, string>
  external: Record<string, string>
  unparsed: Record<string, string>
} {
  // Keyed by entity name from the document. On a plain literal `<!ENTITY
  // __proto__ "...">` was silently dropped - the assignment ran the
  // Object.prototype setter instead of defining a key - and any lookup of an
  // inherited name resolved to an Object.prototype member.
  const internal: Record<string, string> = Object.create(null)
  const external: Record<string, string> = Object.create(null)
  const unparsed: Record<string, string> = Object.create(null)
  const isSpace = (ch: string) =>
    ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r'
  const isNm = (ch: string) => isNameCharCP(ch.charCodeAt(0))

  let i = 0
  while (i < body.length) {
    const idx = body.indexOf('<!ENTITY', i)
    if (idx < 0) break
    let j = idx + '<!ENTITY'.length
    while (j < body.length && isSpace(body[j])) j++
    // Parameter entity: skip.
    if (body[j] === '%') {
      const end = body.indexOf('>', j)
      i = end < 0 ? body.length : end + 1
      continue
    }
    // Read name.
    if (j >= body.length || !isNameStartCP(body.charCodeAt(j))) {
      i = j + 1
      continue
    }
    const nameStart = j
    j++
    while (j < body.length && isNm(body[j])) j++
    const name = body.substring(nameStart, j)
    while (j < body.length && isSpace(body[j])) j++
    // Quoted entity value -> internal entity.
    if (body[j] === '"' || body[j] === "'") {
      const quote = body[j]
      j++
      const valStart = j
      while (j < body.length && body[j] !== quote) j++
      if (j >= body.length) break
      internal[name] = body.substring(valStart, j)
      const end = body.indexOf('>', j)
      i = end < 0 ? body.length : end + 1
      continue
    }
    const end = body.indexOf('>', j)
    const tail = body.substring(j, end < 0 ? body.length : end)
    if (tail.startsWith('SYSTEM') || tail.startsWith('PUBLIC')) {
      // External entity: declared, but never fetched. An `NDATA`
      // notation makes it an *unparsed* entity, which may not be
      // referenced at all.
      if (/(^|[\s"'])NDATA([\s"']|$)/.test(tail)) {
        unparsed[name] = ''
      } else {
        external[name] = ''
      }
    }
    i = end < 0 ? body.length : end + 1
  }
  return { internal, external, unparsed }
}

// Collect what the current parse knows about entity declarations, for
// the §4.1 "Entity Declared" well-formedness check. `unread` is true
// only when part of the DTD went unread AND the document did not
// declare `standalone="yes"` — with `standalone="yes"` the document
// asserts that nothing outside the internal subset matters, so the
// constraint applies again.
function entityDeclState(ctx: any): EntityDeclState {
  const u = ctx?.u
  if (null == u) return {}
  return {
    external: u.dtdExternalEntities,
    unparsed: u.dtdUnparsedEntities,
    unread: true === u.dtdUnread && true !== u.xmlStandalone,
  }
}

type EntityDeclState = {
  external?: Record<string, string>
  unparsed?: Record<string, string>
  unread?: boolean
}

// Does the internal subset contain a parameter-entity reference?
// XML 1.0 §4.1 (WFC: Entity Declared) treats declarations inside a
// parameter entity exactly like declarations in an unread external
// subset: a non-validating processor need not process them, so an
// otherwise-undeclared entity is not a well-formedness error.
const peRefRE = /%[A-Za-z_:][A-Za-z0-9_\-\.:]*;/

// Does the DOCTYPE head (everything between `<!DOCTYPE` and the
// internal subset / closing `>`) carry an ExternalID? XML 1.0 §2.8
// [75]: `ExternalID ::= 'SYSTEM' S SystemLiteral | 'PUBLIC' S
// PubidLiteral S SystemLiteral`.
const externalIDRE = /(^|[\s>])(SYSTEM|PUBLIC)([\s"'])/

// XML declaration standalone document declaration (§2.9 [32]):
// `S 'standalone' Eq ("'" ("yes" | "no") "'" | '"' ("yes" | "no") '"')`.
const standaloneYesRE = /\bstandalone\s*=\s*(['"])yes\1/


// Build a lexer matcher that recognises all top-level XML constructs
// starting with `<`. In embed mode the matcher also claims any text
// between an open tag and its matching close tag so that Jsonic's own
// text/fixed matchers don't split XML character data on JSON-syntax
// characters (`,`, `:`, etc.).
//
// Emits one of:
//   <name attr="v" ...>     -> #XOP  val = { name, attributes }
//   <name attr="v" ... />   -> #XSC  val = { name, attributes }
//   </name>                 -> #XCL  val = name
//   <!-- comment -->        -> #XIG  (parser ignores)
//   <?target ...?>          -> #XIG  (parser ignores)
//   <!DOCTYPE ...>          -> #XIG  (parser ignores)
//   <![CDATA[ ... ]]>       -> #TX   (verbatim text, no entity decoding)
function buildXmlTagMatcher(
  decodeEntity: DecodeEntitiesFn,
  embed: boolean,
  options: XmlOptions,
) {
  const strict = options.strictEntities !== false
  const declared = decodeEntity.declared
  // Backwards-compatible single-char predicates retained for sites that
  // only need a simple character class check (e.g. peek before reading
  // a name). Multi-byte / surrogate pair handling is in `readName` /
  // `cpAt` below.
  const isNameStart = (ch: string) => isNameStartCP(ch.codePointAt(0)!)
  const isNameChar = (ch: string) => isNameCharCP(ch.codePointAt(0)!)
  const isSpace = (ch: string) =>
    ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r'

  // Read an XML Name starting at `start`. Returns the name and the
  // index after it, or null if the character at `start` is not a
  // valid NameStartChar. Handles UTF-16 surrogate pairs so non-BMP
  // code points are treated as single characters. Typed `any` so the
  // matcher's `lex.src` (declared as the boxed `String` upstream)
  // can be passed in without a cast.
  function readName(src: any, start: number): { name: string; end: number } | null {
    if (start >= src.length) return null
    const cp0 = src.codePointAt(start)!
    if (!isNameStartCP(cp0)) return null
    let i = start + (cp0 > 0xffff ? 2 : 1)
    while (i < src.length) {
      const cp = src.codePointAt(i)!
      if (!isNameCharCP(cp)) break
      i += cp > 0xffff ? 2 : 1
    }
    return { name: String(src).substring(start, i), end: i }
  }

  // Validate and decode a run of character data (non-CDATA). Enforces
  // the XML 1.0 well-formedness constraints applicable to text:
  //   - every code point must be a legal XML Char (no C0 controls
  //     other than tab, newline, carriage return);
  //   - the literal sequence "]]>" must not appear in character data;
  //   - every "&" must start a well-formed entity reference.
  // Returns either { val: string } on success or { err: string } if a
  // WF constraint is violated. Pure decoding (without validation) is
  // also available for CDATA bodies via decodeEntity().
  function processText(
    raw: string,
    dtd: Record<string, string>,
    entdecl: { external?: Record<string, string>; unread?: boolean },
  ): { val?: string; err?: string } {
    const ctrlErr = checkChars(raw)
    if (ctrlErr) return { err: ctrlErr }
    if (raw.indexOf(']]>') >= 0) {
      return { err: 'cdata_terminator_in_text' }
    }
    const ampErr = checkEntityRefs(raw, dtd, declared, strict, entdecl)
    if (ampErr) return { err: ampErr }
    // §2.11: normalise CR LF and lone CR to LF before downstream processing.
    const normalised = normaliseLineEndings(raw)
    return {
      val: options.entities !== false ? decodeEntity(normalised, dtd) : normalised,
    }
  }

  return function makeXmlTagMatcher(_cfg: Config, _opts: TabnasOptions) {
    return function xmlTagMatcher(lex: Lex) {
      const { pnt, src } = lex
      let sI = pnt.sI

      // Strip a UTF-8 byte-order mark at the very start of input.
      // After decoding, a UTF-8 BOM appears as a single U+FEFF
      // character; some toolchains pass through the raw bytes
      // (EF BB BF) as three separate Latin-1 code units. The BOM is
      // not part of the document, so skip it and carry on matching in
      // this same call — returning `undefined` here would hand control
      // back to a lexer that has no other matcher enabled in pure
      // mode, and the following `<?xml` would be reported as an
      // unexpected character.
      if (0 === sI && 0 < src.length) {
        let bomLen = 0
        if (0xfeff === src.charCodeAt(0)) {
          bomLen = 1
        } else if (3 <= src.length &&
          0xef === src.charCodeAt(0) &&
          0xbb === src.charCodeAt(1) &&
          0xbf === src.charCodeAt(2)) {
          bomLen = 3
        }
        if (0 < bomLen) {
          // The BOM occupies no display column, so only the source
          // index advances; token positions stay aligned with the
          // document text proper.
          pnt.sI = sI = bomLen
        }
      }

      // Inside an open XML element (depth > 0), consume characters up
      // to the next `<` as a single #TX text token so that Jsonic's
      // own matchers don't reinterpret commas/colons/etc. as JSON
      // separators in embed mode, and so we can apply XML text
      // validation in pure mode too.
      if (sI < src.length && src[sI] !== '<') {
        const depth = (lex.ctx?.u?.xmlDepth | 0) || 0
        if (depth > 0) {
          let i = sI
          while (i < src.length && src[i] !== '<') i++
          if (i === sI) return undefined
          const raw = src.substring(sI, i)
          const dtd = (lex.ctx?.u?.dtdEntities) || {}
          const result = processText(raw, dtd, entityDeclState(lex.ctx))
          if (result.err) {
            return lex.bad(result.err, sI, i)
          }
          const tkn = lex.token('#TX', result.val, raw, pnt)
          pnt.sI = i
          pnt.cI += i - sI
          return tkn
        }
      }

      if (sI >= src.length || src[sI] !== '<') return undefined

      // Comment: <!-- ... -->
      if (src.startsWith('<!--', sI)) {
        const endIdx = src.indexOf('-->', sI + 4)
        if (endIdx === -1) {
          return lex.bad('unterminated_comment', sI, src.length)
        }
        const body = src.substring(sI + 4, endIdx)
        // WF constraint: "--" must not occur in a comment body.
        if (body.indexOf('--') >= 0) {
          return lex.bad('comment_double_dash', sI, endIdx + 3)
        }
        if (checkChars(body)) {
          return lex.bad('invalid_xml_char', sI, endIdx + 3)
        }
        const end = endIdx + 3
        const tkn = lex.token('#XIG', src.substring(sI, end), src.substring(sI, end), pnt)
        pnt.sI = end
        pnt.cI += end - sI
        return tkn
      }

      // CDATA: <![CDATA[ ... ]]>
      if (src.startsWith('<![CDATA[', sI)) {
        const endIdx = src.indexOf(']]>', sI + 9)
        if (endIdx === -1) {
          return lex.bad('unterminated_cdata', sI, src.length)
        }
        const end = endIdx + 3
        const text = src.substring(sI + 9, endIdx)
        if (checkChars(text)) {
          return lex.bad('invalid_xml_char', sI, end)
        }
        // §2.11 line-end normalisation applies to CDATA too.
        const tkn = lex.token('#TX', normaliseLineEndings(text), src.substring(sI, end), pnt)
        pnt.sI = end
        pnt.cI += end - sI
        return tkn
      }

      // DOCTYPE: <!DOCTYPE ... [...] >
      if (src.startsWith('<!DOCTYPE', sI)) {
        let i = sI + 9
        let depth = 0
        let subsetStart = -1
        let subsetEnd = -1
        while (i < src.length) {
          const ch = src[i]
          // Skip over comments and processing instructions first: their
          // bodies are opaque text, so an apostrophe (`doesn't`) or a
          // `>` inside one must not be read as markup.
          if (src.startsWith('<!--', i)) {
            const ce = src.indexOf('-->', i + 4)
            if (ce === -1) { i = src.length; break }
            i = ce + 3
            continue
          }
          if (src.startsWith('<?', i)) {
            const pe = src.indexOf('?>', i + 2)
            if (pe === -1) { i = src.length; break }
            i = pe + 2
            continue
          }
          // Skip over quoted strings so `]` and `>` inside an
          // entity value or attribute default cannot terminate the
          // subset prematurely.
          if (ch === '"' || ch === "'") {
            i++
            while (i < src.length && src[i] !== ch) i++
            if (i < src.length) i++
            continue
          }
          if (ch === '[') {
            if (depth === 0) subsetStart = i + 1
            depth++
          } else if (ch === ']') {
            depth--
            if (depth === 0) subsetEnd = i
          } else if (ch === '>' && depth <= 0) break
          i++
        }
        if (i >= src.length) {
          return lex.bad('unterminated_doctype', sI, src.length)
        }
        const end = i + 1
        if (lex.ctx) {
          const u: any = lex.ctx.u || (lex.ctx.u = {})

          // XML 1.0 §4.1, WFC "Entity Declared": the constraint that a
          // referenced entity be declared applies only when the
          // processor has seen every declaration. If the DOCTYPE names
          // an external subset — which we never fetch — then (unless
          // the document declares `standalone="yes"`) an undeclared
          // entity is NOT a well-formedness error. The head of the
          // DOCTYPE is everything up to the internal subset's `[`, or
          // up to the closing `>` when there is no internal subset.
          const headEnd = 0 <= subsetStart ? subsetStart - 1 : i
          if (externalIDRE.test(src.substring(sI + 9, headEnd))) {
            u.dtdUnread = true
          }

          // Extract internal-subset declarations and stash them on
          // the per-parse context. The matcher's text/attribute paths
          // and the element actions read these back via lex.ctx.u.
          if (subsetStart >= 0 && subsetEnd > subsetStart) {
            const subset = src.substring(subsetStart, subsetEnd)

            // A parameter-entity reference in the internal subset hides
            // declarations from a non-validating processor just as an
            // external subset does.
            if (peRefRE.test(subset)) {
              u.dtdUnread = true
            }

            const ents = parseDoctypeEntities(subset)
            if (Object.keys(ents.internal).length > 0) {
              u.dtdEntities = Object.assign(
                Object.create(null), u.dtdEntities || {}, ents.internal)
            }
            // Externally-declared general entities are declared (so a
            // reference to one is well-formed) but unresolvable, so
            // they are tracked apart from the expandable ones.
            if (Object.keys(ents.external).length > 0) {
              u.dtdExternalEntities = Object.assign(
                Object.create(null), u.dtdExternalEntities || {}, ents.external)
            }
            if (Object.keys(ents.unparsed).length > 0) {
              u.dtdUnparsedEntities = Object.assign(
                Object.create(null), u.dtdUnparsedEntities || {}, ents.unparsed)
            }
            const atts = parseDoctypeAttlists(subset)
            if (Object.keys(atts).length > 0) {
              const merged: Record<string, any> =
                Object.assign(Object.create(null), u.dtdAttrDefaults || {})
              for (const elem of Object.keys(atts)) {
                merged[elem] = Object.assign(
                  Object.create(null), merged[elem] || {}, atts[elem])
              }
              u.dtdAttrDefaults = merged
            }
          }
        }
        const tkn = lex.token('#XIG', src.substring(sI, end), src.substring(sI, end), pnt)
        pnt.sI = end
        pnt.cI += end - sI
        return tkn
      }

      // Processing instruction: <? ... ?>
      if (src[sI + 1] === '?') {
        const endIdx = src.indexOf('?>', sI + 2)
        if (endIdx === -1) {
          return lex.bad('unterminated_pi', sI, src.length)
        }
        // WF constraint: PI target must be a Name (and not empty).
        const piTargetRes = readName(src, sI + 2)
        if (piTargetRes == null || piTargetRes.end > endIdx) {
          return lex.bad('pi_target_invalid', sI, endIdx + 2)
        }
        const i = piTargetRes.end
        // After the target, only whitespace then content is allowed.
        if (i < endIdx && !isSpace(src[i])) {
          return lex.bad('pi_target_invalid', sI, endIdx + 2)
        }
        if (checkChars(src.substring(sI + 2, endIdx))) {
          return lex.bad('invalid_xml_char', sI, endIdx + 2)
        }
        // The XML declaration's standalone document declaration
        // (§2.9) decides whether the §4.1 "Entity Declared" constraint
        // still applies when part of the DTD went unread — with
        // `standalone="yes"` the document promises there is nothing
        // relevant out there, so an undeclared entity is an error again.
        if ('xml' === piTargetRes.name && lex.ctx &&
          standaloneYesRE.test(src.substring(i, endIdx))) {
          const u: any = lex.ctx.u || (lex.ctx.u = {})
          u.xmlStandalone = true
        }
        const end = endIdx + 2
        const tkn = lex.token('#XIG', src.substring(sI, end), src.substring(sI, end), pnt)
        pnt.sI = end
        pnt.cI += end - sI
        return tkn
      }

      // Closing tag: </name>
      if (src[sI + 1] === '/') {
        const nameRes = readName(src, sI + 2)
        // WF: empty close tag `</>` is invalid.
        if (nameRes == null) {
          return lex.bad('xml_invalid_tag', sI, Math.min(src.length, sI + 3))
        }
        const name = nameRes.name
        let i = nameRes.end
        while (i < src.length && isSpace(src[i])) i++
        if (src[i] !== '>') {
          return lex.bad('xml_invalid_tag', sI, i + 1)
        }
        const end = i + 1
        const tkn = lex.token('#XCL', name, src.substring(sI, end), pnt)
        pnt.sI = end
        pnt.cI += end - sI
        if (lex.ctx) {
          const u: any = lex.ctx.u || (lex.ctx.u = {})
          u.xmlDepth = Math.max(0, (u.xmlDepth | 0) - 1)
        }
        return tkn
      }

      // Opening or self-close tag: <name attr="v" .../>
      const elemNameRes = readName(src, sI + 1)
      if (elemNameRes == null) return undefined
      const name = elemNameRes.name
      let i = elemNameRes.end
      // Keyed by attribute names the document controls: as a plain `{}` an
      // attribute called __proto__ was silently discarded rather than kept as
      // an ordinary key, and a later write of it would reparent this map.
      const attributes: Record<string, string> = Object.create(null)

      while (true) {
        const wsStart = i
        while (i < src.length && isSpace(src[i])) i++
        if (i >= src.length) {
          return lex.bad('xml_invalid_tag', sI, src.length)
        }

        if (src[i] === '>') {
          const end = i + 1
          const tkn = lex.token('#XOP', { name, attributes }, src.substring(sI, end), pnt)
          pnt.sI = end
          pnt.cI += end - sI
          if (lex.ctx) {
            const u: any = lex.ctx.u || (lex.ctx.u = {})
            u.xmlDepth = (u.xmlDepth | 0) + 1
          }
          return tkn
        }
        if (src[i] === '/' && src[i + 1] === '>') {
          const end = i + 2
          const tkn = lex.token('#XSC', { name, attributes }, src.substring(sI, end), pnt)
          pnt.sI = end
          pnt.cI += end - sI
          // #XSC is an instantly-closed element, so depth is unchanged.
          return tkn
        }

        if (wsStart === i) {
          return lex.bad('xml_invalid_tag', sI, i + 1)
        }

        const attrNameRes = readName(src, i)
        if (attrNameRes == null) {
          return lex.bad('xml_invalid_tag', sI, i + 1)
        }
        const attrName = attrNameRes.name
        i = attrNameRes.end

        while (i < src.length && isSpace(src[i])) i++
        if (src[i] !== '=') {
          return lex.bad('xml_invalid_tag', sI, i + 1)
        }
        i++
        while (i < src.length && isSpace(src[i])) i++

        const quote = src[i]
        if (quote !== '"' && quote !== "'") {
          return lex.bad('xml_invalid_tag', sI, i + 1)
        }
        i++
        const valStart = i
        // Per the XML 1.0 spec, attribute values cannot contain a
        // literal `<`. Tracking the position lets us also validate
        // entity references in the value.
        while (i < src.length && src[i] !== quote) {
          if (src[i] === '<') {
            return lex.bad('lt_in_attr_value', sI, i + 1)
          }
          i++
        }
        if (i >= src.length) {
          return lex.bad('xml_invalid_tag', sI, src.length)
        }
        const rawVal = src.substring(valStart, i)
        i++

        const charErr = checkChars(rawVal)
        if (charErr) {
          return lex.bad(charErr, valStart, i)
        }
        const dtd = (lex.ctx?.u?.dtdEntities) || {}
        const ampErr = checkEntityRefs(
          rawVal, dtd, declared, strict, entityDeclState(lex.ctx), true)
        if (ampErr) {
          return lex.bad(ampErr, valStart, i)
        }
        if (Object.prototype.hasOwnProperty.call(attributes, attrName)) {
          return lex.bad('duplicate_attribute', sI, i)
        }
        // §3.3.3 attribute-value normalisation: literal whitespace
        // (TAB, LF, CR, CRLF) becomes a single SPACE before any
        // entity references are decoded. We do not have DTD-supplied
        // attribute types, so all attributes are treated as CDATA-
        // typed (no further whitespace collapsing or trimming).
        const normalised = normaliseAttrWhitespace(rawVal)
        attributes[attrName] = decodeEntity(normalised, dtd)
      }
    }
  }
}


// §2.11 End-of-line handling: any literal CR (#xD) or CR-LF
// (#xD #xA) is normalised to a single LF (#xA) before parsing
// proceeds. Applies to character data, CDATA section bodies, and is
// the precondition for §3.3.3 attribute-value normalisation.
function normaliseLineEndings(s: string): string {
  if (s.indexOf('\r') < 0) return s
  return s.replace(/\r\n?/g, '\n')
}

// §3.3.3 attribute-value normalisation for CDATA-typed attributes
// (the default in the absence of a DTD). All TAB, LF, CR, and CRLF
// occurrences in the source are replaced by a single SPACE; runs are
// not further collapsed and the value is not trimmed.
function normaliseAttrWhitespace(s: string): string {
  if (!/[\r\n\t]/.test(s)) return s
  return s.replace(/\r\n?|[\t\n]/g, ' ')
}

// XML 1.0 Fifth Edition NameStartChar (§2.3 [4]). The non-Latin
// ranges below cover the characters allowed at the start of an
// element / attribute / entity / PI-target name.
function isNameStartCP(cp: number): boolean {
  return cp === 0x3a || // ':'
    cp === 0x5f ||      // '_'
    (cp >= 0x41 && cp <= 0x5a) ||
    (cp >= 0x61 && cp <= 0x7a) ||
    (cp >= 0xc0 && cp <= 0xd6) ||
    (cp >= 0xd8 && cp <= 0xf6) ||
    (cp >= 0xf8 && cp <= 0x2ff) ||
    (cp >= 0x370 && cp <= 0x37d) ||
    (cp >= 0x37f && cp <= 0x1fff) ||
    (cp >= 0x200c && cp <= 0x200d) ||
    (cp >= 0x2070 && cp <= 0x218f) ||
    (cp >= 0x2c00 && cp <= 0x2fef) ||
    (cp >= 0x3001 && cp <= 0xd7ff) ||
    (cp >= 0xf900 && cp <= 0xfdcf) ||
    (cp >= 0xfdf0 && cp <= 0xfffd) ||
    (cp >= 0x10000 && cp <= 0xeffff)
}

// XML 1.0 NameChar (§2.3 [4a]) — NameStartChar plus the digits,
// hyphen, full stop, the middle dot and the combining-mark blocks.
function isNameCharCP(cp: number): boolean {
  return isNameStartCP(cp) ||
    cp === 0x2d || cp === 0x2e || // '-' '.'
    (cp >= 0x30 && cp <= 0x39) || // '0'-'9'
    cp === 0xb7 ||
    (cp >= 0x300 && cp <= 0x36f) ||
    (cp >= 0x203f && cp <= 0x2040)
}

// Validate that every code unit in `s` is a legal XML 1.0 Char.
// Returns 'invalid_xml_char' on the first illegal character, '' if all
// characters are legal. Only the C0 control band is checked here; the
// full Char production (which excludes #xFFFE/#xFFFF and unpaired
// surrogates) is not enforced.
function checkChars(s: string): string {
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i)
    if (c < 0x20 && c !== 0x09 && c !== 0x0a && c !== 0x0d) {
      return 'invalid_xml_char'
    }
  }
  return ''
}

// Validate entity references in a run of character data. Returns an
// error code on the first malformed reference, or '' if every `&`
// in the input is part of a well-formed reference. The `dtd` map
// supplies DOCTYPE-declared entity names; `extra` adds named
// entities to consider declared (typically the predefined and
// caller-supplied entities). When `strict` is true, references to
// unknown names trigger `bad_entity_ref`; when false (legacy mode),
// the syntactic check still runs but unknown names pass through.
//
// `entdecl` describes what the processor knows about the DTD:
//   external - general entities declared by an `<!ENTITY name SYSTEM
//              ...>` in the internal subset. Declared, so referencing
//              one is well-formed, but never fetched.
//   unparsed - external entities with an `NDATA` notation. §4.1 WFC
//              "Parsed Entity": these must never be referenced.
//   unread   - true when part of the DTD went unread (an external
//              subset, or declarations behind a parameter-entity
//              reference) and the document is not `standalone="yes"`.
//              XML 1.0 §4.1 WFC "Entity Declared" then does not apply,
//              so an unknown entity name must be let through even in
//              strict mode.
//
// `inAttr` marks a scan of an attribute value, where §4.1 WFC "No
// External Entity References" additionally forbids referencing any
// external entity.
//
// Well-formed forms:
//   &name;       — name must start with a NameStartChar
//   &#nnnn;      — decimal numeric character reference
//   &#xhhhh;     — hexadecimal numeric character reference (lowercase
//                  `x` only, per [66])
function checkEntityRefs(
  s: string,
  dtd?: Record<string, string>,
  extra?: Record<string, string>,
  strict?: boolean,
  entdecl?: EntityDeclState,
  inAttr?: boolean,
): string {
  for (let i = 0; i < s.length; i++) {
    if (s[i] !== '&') continue
    const semi = s.indexOf(';', i + 1)
    if (semi < 0) return 'bad_entity_ref'
    const ref = s.substring(i + 1, semi)
    if (ref.length === 0) return 'bad_entity_ref'
    if (ref[0] === '#') {
      if (ref.length < 2) return 'bad_entity_ref'
      // XML 1.0 §4.1 [66]:
      //   CharRef ::= '&#' [0-9]+ ';' | '&#x' [0-9a-fA-F]+ ';'
      // The `x` marker is lowercase-only — `&#X26;` is NOT a character
      // reference, and since `X26` is not a Name it is not an entity
      // reference either, so it is not well-formed.
      const hex = 'x' === ref[1]
      const digits = hex ? ref.substring(2) : ref.substring(1)
      if (digits.length === 0) return 'bad_entity_ref'
      const valid = hex
        ? /^[0-9a-fA-F]+$/.test(digits)
        : /^[0-9]+$/.test(digits)
      if (!valid) return 'bad_entity_ref'
    } else {
      // Entity name must be a Name (NameStartChar followed by NameChars).
      let j = 0
      const startCP = ref.codePointAt(0)
      if (startCP === undefined || !isNameStartCP(startCP)) {
        return 'bad_entity_ref'
      }
      j += startCP > 0xffff ? 2 : 1
      while (j < ref.length) {
        const cp = ref.codePointAt(j)!
        if (!isNameCharCP(cp)) return 'bad_entity_ref'
        j += cp > 0xffff ? 2 : 1
      }
      const has = (m: Record<string, string> | undefined) =>
        !!m && Object.prototype.hasOwnProperty.call(m, ref)

      // §4.1 WFC "Parsed Entity": an entity reference must not name an
      // unparsed (NDATA) entity, anywhere.
      if (has(entdecl?.unparsed)) return 'unparsed_entity_ref'

      if (has(entdecl?.external)) {
        // Declared externally: the "Entity Declared" WFC is satisfied
        // and the replacement text is simply not included. But §4.1
        // WFC "No External Entity References" forbids the reference in
        // an attribute value.
        if (inAttr) return 'external_entity_in_attr'
      } else if (strict &&
        true !== entdecl?.unread &&
        !has(extra) && !has(dtd)) {
        // §4.1 WFC "Entity Declared" — suspended when the processor
        // never got to see the whole DTD (see `entdecl.unread`).
        return 'undeclared_entity'
      }
    }
    i = semi
  }
  return ''
}


// Resolve namespaces on an element tree. Walks the tree once,
// maintaining four kinds of inherited state:
//
//   ns      - prefix → namespace URI (empty key = default ns), per
//             XML Namespaces 1.0
//   space   - active xml:space value ('default' or 'preserve'),
//             inherited per XML 1.0 §2.10
//   lang    - active xml:lang value, inherited per XML 1.0 §2.12
//
// `space` and `lang` are recorded on each element only when they are
// non-default (so plain documents don't sprout extra fields).
type XmlScope = {
  ns: Record<string, string>
  space: string
  lang: string
}

// Per Namespaces in XML 1.0 §2 "Reserved prefixes and namespace names":
// the xml prefix is bound to the URI below and may be used implicitly.
const XML_NS_URI = 'http://www.w3.org/XML/1998/namespace'
// The xmlns prefix is reserved and must never be declared.
const XMLNS_NS_URI = 'http://www.w3.org/2000/xmlns/'

function resolveNamespaces(
  element: XmlElement,
  scope: Record<string, string>,
  strict?: boolean,
): string {
  // Pre-bind the xml prefix to its reserved URI so xml:lang / xml:space
  // qualify correctly without an explicit declaration.
  return resolveScope(element, {
    ns: { ...scope, xml: XML_NS_URI },
    space: 'default',
    lang: '',
  }, true === strict)
}

// A namespace name is a URI reference (Namespaces in XML 1.0 §2, RFC
// 3986). White space is not permitted in a URI reference, and
// attribute-value normalisation (§3.3.3) has already turned any
// literal TAB/LF/CR in the declaration into a space by the time we see
// it, so a space here means the source held white space inside the
// namespace name.
function invalidNamespaceURI(uri: string): boolean {
  for (let i = 0; i < uri.length; i++) {
    const ch = uri[i]
    if (' ' === ch || '\t' === ch || '\n' === ch || '\r' === ch) return true
  }
  return false
}

// Returns '' on success or an XML namespace error code on the first
// violation (reserved-prefix misuse, unbound prefix). On error the
// tree may be partly annotated; callers should treat that as undefined.
function resolveScope(
  element: XmlElement, scope: XmlScope, strict: boolean,
): string {
  // Keyed by namespace prefixes the document controls, so it is allocated
  // without a prototype: `xmlns:__proto__` would otherwise reparent the scope
  // map, and prefix lookups would then resolve through Object.prototype.
  const ns: Record<string, any> = Object.assign(Object.create(null), scope.ns)
  let space = scope.space
  let lang = scope.lang

  const attrKeys = Object.keys(element.attributes || {})

  // Pass 1: every namespace declaration on this element, plus xml:space
  // / xml:lang. Namespaces in XML 1.0 §5.2 scopes a declaration over the
  // whole element it appears on, *including that element's own other
  // attributes* — so all declarations must be in hand before any
  // prefixed name is resolved. Resolving in a single pass made binding
  // depend on attribute order, wrongly rejecting
  // `<a p:x="1" xmlns:p="..."/>`.
  for (const key of attrKeys) {
    const val = element.attributes[key]
    if (key === 'xmlns') {
      if (val === XML_NS_URI || val === XMLNS_NS_URI) {
        return 'reserved_namespace'
      }
      if (invalidNamespaceURI(val)) return 'invalid_namespace_uri'
      ns[''] = val
    } else if (key.startsWith('xmlns:')) {
      const prefix = key.substring(6)
      if (prefix === 'xml') {
        if (val !== XML_NS_URI) return 'reserved_namespace'
      } else if (prefix === 'xmlns') {
        return 'reserved_namespace'
      } else if (val === XML_NS_URI || val === XMLNS_NS_URI) {
        return 'reserved_namespace'
      }
      if (invalidNamespaceURI(val)) return 'invalid_namespace_uri'
      ns[prefix] = val
    } else if (key === 'xml:space') {
      space = val
    } else if (key === 'xml:lang') {
      lang = val
    }
  }

  // Pass 2: resolve prefixed attribute names against the completed
  // in-scope declarations.
  if (strict) {
    for (const key of attrKeys) {
      if (key === 'xmlns' || key.startsWith('xmlns:')) continue
      const colon = key.indexOf(':')
      if (colon > 0) {
        const ap = key.substring(0, colon)
        // Namespace-constraint only (see XmlOptions.strictNamespaces):
        // an unbound prefix on an attribute name leaves the attribute
        // unqualified but keeps the document XML 1.0 well-formed.
        if (!Object.prototype.hasOwnProperty.call(ns, ap)) {
          return 'unbound_prefix'
        }
      }
    }
  }

  const colonIdx = element.name.indexOf(':')
  if (colonIdx >= 0) {
    const prefix = element.name.substring(0, colonIdx)
    element.prefix = prefix
    element.localName = element.name.substring(colonIdx + 1)
    if (Object.prototype.hasOwnProperty.call(ns, prefix)) {
      element.namespace = ns[prefix]
    } else if (strict) {
      return 'unbound_prefix'
    }
    // Not strict: leave `namespace` unset — the element is named but
    // unqualified, which is what a namespace-unaware (yet XML 1.0
    // conformant) consumer sees.
  } else {
    element.localName = element.name
    if (ns['']) {
      element.namespace = ns['']
    }
  }

  if (space !== 'default') (element as any).space = space
  if (lang !== '') (element as any).lang = lang

  const childScope: XmlScope = { ns, space, lang }
  for (const child of element.children) {
    if (child && 'object' === typeof child) {
      const err = resolveScope(child, childScope, strict)
      if (err) return err
    }
  }
  return ''
}


Xml.defaults = {
  namespaces: true,
  entities: true,
  customEntities: {},
  strictEntities: true,
  strictNamespaces: false,
  embed: false,
} as XmlOptions

// VERSION is this package's version. It MUST equal package.json "version":
// the release orchestrator rewrites both, and the version test fails the
// build if they drift. Mirrors `const VERSION` in go/xml.go.
const VERSION = '0.7.7'

export { Xml, decodeBOM, VERSION }

export type { XmlOptions, XmlElement }
