# Concepts

Background on how `@tabnas/xml` works, and why it is built the way it is.
This is understanding-oriented reading — for steps see the
[tutorial](tutorial.md) and [how-to guide](guide.md); for exact
signatures and options see the [reference](reference.md).

## A grammar plugin on the Jsonic engine

This package is not a standalone XML parser. It is a **plugin** for the
`tabnas` parsing engine — the same engine that drives
[`@tabnas/jsonic`](https://github.com/tabnas/jsonic), the relaxed-JSON
grammar. Jsonic itself is "a grammar on an engine": a configurable,
matcher-based lexer plus a rule-based parser. This plugin adds XML by
configuring that same machinery — a custom lexer matcher, a handful of
grammar rules, and some option-driven reconfiguration — rather than
hand-writing a parser.

The payoff is reuse: error reporting, source-location tracking, the
railroad-diagram tooling, and the option/derivation system all come from
the engine for free. The XML grammar is just data and rules fed to it.

## Two stages: a custom lexer, then four rules

A parse runs in the engine's two cooperating stages.

The **lexer** turns source text into tokens. This plugin registers one
custom matcher (`xmltag`, at a high priority order) that recognises
everything starting with `<`: open tags, self-closing tags, close tags,
comments, processing instructions, DOCTYPE, and CDATA. It emits five
token kinds:

- `#XOP` — an open tag, carrying `{ name, attributes }`
- `#XSC` — a self-closing tag, carrying `{ name, attributes }`
- `#XCL` — a close tag, carrying the name
- `#TX` — a run of character data (or a CDATA body)
- `#XIG` — an *ignored* construct (comment, PI, DOCTYPE), which the
  parser's IGNORE set drops

The matcher does the lexical heavy lifting: name scanning (Unicode-aware,
including non-BMP code points), attribute parsing, entity decoding,
end-of-line and attribute-value normalisation, and the well-formedness
checks (illegal characters, `]]>` in text, `--` in comments, `<` in
attribute values, malformed `&` references). It also tracks XML nesting
depth so that while inside an open element it claims the whole text run up
to the next `<` as a single `#TX` token — keeping JSON-syntax characters
like `,` and `:` from being reinterpreted, which matters in embed mode.

The **parser** then consumes those tokens with four small rules:

- `xml` — the document: optional leading text, then one `element`,
  gated so a *second* root cannot start.
- `element` — `#XOP … #XCL` or `#XSC`; builds the element node.
- `content` — loops over children until the matching `#XCL`.
- `child` — one child: `#TX` text, or a nested `element`.

Each rule has open/close phases and short alternates with at most two
tokens of lookahead — the engine's deterministic, no-backtracking model.
The full grammar is small enough to read in one screen; it lives in the
repository's top-level `xml-grammar.jsonic` and is embedded verbatim into
the source. The [railroad diagram](grammar.svg) is generated from this
live grammar.

## The grammar is shared data

The grammar text is authored once, in relaxed-JSON, in
`xml-grammar.jsonic`. A build step (`embed-grammar.js`) splices it
verbatim into `src/xml.ts`; the Go port mirrors the same rules. The
function references in the grammar (the `@`-prefixed names like
`@element-open`, `@child-text`, `@element-close`) are resolved at plugin
time against a map of JavaScript callbacks that build the result tree and
enforce the structural constraints (single root, matching close tags).

Keeping the grammar as data, not code, is what lets a diagram be drawn
from it and lets two language ports stay in lock-step.

## Two modes from one grammar

The same element grammar serves both modes; the difference is how the
surrounding parser is configured.

**Pure-XML mode** (`embed: false`, the default) reconfigures the engine
*around* the XML rules: the start rule becomes `xml`, the JSON structural
tokens (`{ } [ ] : ,`) are unbound, the number/string/value/comment/space
lexers are turned off, and Jsonic's now-unreachable value rules (`val`,
`map`, `list`, `pair`, `elem`) are deleted from the grammar — so the
parser, and the generated diagram, carry only what XML uses. The input is
then pure XML.

**Embed mode** (`embed: true`) leaves Jsonic's grammar intact and adds an
XML literal as an alternate of the `val` rule. When the parser is looking
for a value and sees `#XOP`/`#XSC`, it backtracks one token and pushes
the `element` rule, building an XML subtree wherever a value was
expected. This makes XML a first-class value type inside relaxed-JSON
documents.

## Namespaces, space, and lang as a post-pass

Lexing and the four rules build the raw element tree with names and
attributes verbatim. Namespace resolution is a separate single walk over
the finished tree (run by the `@xml-bc` hook in pure mode, or an
`element` close hook in embed mode). That walk threads three pieces of
inherited scope down the tree: the prefix→URI bindings, the active
`xml:space`, and the active `xml:lang`. It pre-binds the reserved `xml`
prefix, rejects reserved-prefix/URI misuse and unbound prefixes, and
records `prefix` / `namespace` / `space` / `lang` on elements — but only
where they actually apply, so plain documents stay clean. Turning
`namespaces` off simply skips this pass.

## Design choices and their edges

A few decisions shape what is accepted and what is rejected.

- **Well-formedness, not full validation.** The parser enforces the XML
  1.0 well-formedness constraints it can check locally (matching tags,
  legal characters, single root, no `]]>` in text, no `--` in comments,
  no `<` in attribute values, resolvable strict entities, unbound
  prefixes). It does *not* validate against a schema or DTD content
  model, and the `Char` production is only checked for the C0 control
  band, not the full excluded set.
- **DOCTYPE is read, not honoured wholesale.** The internal subset is
  scanned for `<!ENTITY>` and `<!ATTLIST>` declarations, which affect the
  parse (entity expansion, attribute defaults). External and parameter
  entities are recognised but never fetched or expanded; the DOCTYPE
  declaration itself is dropped.
- **CDATA is verbatim.** A `<![CDATA[…]]>` body becomes a text child with
  no entity decoding — `&amp;` inside CDATA stays `&amp;` — matching XML
  semantics.
- **Strict entities by default.** Per XML 1.0 §4.1, an undeclared named
  entity is an error. `strictEntities: false` relaxes this for templating
  input, leaving unknown `&name;` references in place.
- **Predefined entities win.** A DOCTYPE `<!ENTITY amp "Z">` does not
  override `&amp;` → `&`; the five predefined entities (and any
  `customEntities`) take precedence.

For the exact error codes and the full accepted-syntax table, see the
[reference](reference.md). The Go port mirrors all of the above; its
[concepts doc](../../go/doc/concepts.md) lists the few host-language
differences.
