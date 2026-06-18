# Concepts (Go)

Background on how the Go `xml` package works, and why it is built the way
it is. This is understanding-oriented reading — for steps see the
[tutorial](tutorial.md) and [how-to guide](guide.md); for exact
signatures and options see the [reference](reference.md).

## A grammar plugin on the Jsonic engine

This package is not a standalone XML parser. It is a **plugin** for the
`jsonic` engine (`github.com/tabnas/jsonic/go`) — the relaxed-JSON
parser. Jsonic is "a grammar on an engine": a configurable,
matcher-based lexer plus a rule-based parser. This plugin adds XML by
configuring that same machinery — a custom lexer matcher, four grammar
rules, and option-driven reconfiguration — rather than hand-writing a
parser. Error reporting, source-location tracking, and the option system
all come from the engine.

## Two stages: a custom lexer, then four rules

A parse runs in the engine's two cooperating stages.

The **lexer** turns source text into tokens. The plugin registers one
custom matcher (`xmltag`, at a high priority order) that recognises
everything starting with `<` and emits five token kinds:

- `#XOP` — open tag, carrying `map[string]any{"name", "attributes"}`
- `#XSC` — self-closing tag, carrying the same
- `#XCL` — close tag, carrying the name `string`
- `#TX` — a run of character data (or a CDATA body)
- `#XIG` — an *ignored* construct (comment, PI, DOCTYPE), dropped via the
  parser's IGNORE token set

The matcher does the lexical work: rune-aware name scanning (Unicode
`NameStartChar` / `NameChar`, including non-BMP runes), attribute
parsing, entity decoding, end-of-line and attribute-value normalisation,
and the well-formedness checks (illegal characters, `]]>` in text, `--`
in comments, `<` in attribute values, malformed `&` references). It
tracks XML nesting depth so that while inside an open element it claims
the whole run up to the next `<` as a single `#TX` token.

The **parser** then consumes those tokens with four rules — `xml`,
`element`, `content`, `child` — each with open/close phases and short
alternates with at most two tokens of lookahead. The grammar is small
enough to read in one screen; it lives in the repository's top-level
`xml-grammar.jsonic` (authored once, in relaxed-JSON) and is mirrored
here as a `jsonic.GrammarSpec`. The `@`-prefixed function references in
the grammar are resolved at plugin time against Go callbacks that build
the result tree and enforce the structural constraints (single root,
matching close tags).

## Two modes from one grammar

**Pure-XML mode** (`embed: false`, the default) reconfigures the engine
around the XML rules: the start rule becomes `xml`, the JSON structural
tokens are unbound, the number/string/value/comment/space lexers are
turned off, and Jsonic's now-unreachable value rules (`val`, `map`,
`list`, `pair`, `elem`) are deleted. A dummy fixed token bound to an
illegal XML character is registered so the lexer keeps a non-empty fixed
table — without it, XML text containing a comma would be truncated at the
comma. The input is then pure XML.

**Embed mode** (`embed: true`) leaves Jsonic's grammar intact and adds an
XML literal as an alternate of the `val` rule. When the parser is looking
for a value and sees `#XOP`/`#XSC`, it backtracks one token and pushes
the `element` rule, building an XML subtree wherever a value was expected.

## Namespaces, space, and lang as a post-pass

Lexing and the four rules build the raw tree verbatim. Namespace
resolution is a separate single walk over the finished tree (the
`@xml-bc` hook in pure mode, or an `element` close hook in embed mode).
It threads three pieces of inherited scope down the tree — the
prefix→URI bindings, the active `xml:space`, the active `xml:lang` —
pre-binds the reserved `xml` prefix, rejects reserved-prefix/URI misuse
and unbound prefixes, and records `prefix` / `namespace` / `space` /
`lang` only where they apply. Turning `namespaces` off skips this pass.

## Design choices and their edges

- **Well-formedness, not full validation.** The parser enforces the XML
  1.0 well-formedness constraints it can check locally; it does not
  validate against a schema or DTD content model, and the `Char`
  production is only checked for the C0 control band.
- **DOCTYPE is read, not honoured wholesale.** The internal subset is
  scanned for `<!ENTITY>` and `<!ATTLIST>` (which affect the parse);
  external and parameter entities are recognised but never fetched, and
  the DOCTYPE declaration itself is dropped.
- **CDATA is verbatim.** A `<![CDATA[…]]>` body becomes a text child with
  no entity decoding.
- **Strict entities by default.** An undeclared named entity is an error
  (XML 1.0 §4.1); `strictEntities: false` relaxes this for templating.
- **Predefined entities win.** A DOCTYPE `<!ENTITY amp "Z">` does not
  override `&amp;` → `&`.

## Differences from the TS version

The TypeScript package (`@tabnas/xml`) is the canonical implementation;
this Go module is a faithful port. Both produce identical parse results
for the shared conformance fixtures (`test/spec/*.tsv`, run by both
suites). The differences are host-language shape, not parse semantics.

### API shape

| Aspect            | TypeScript                                  | Go                                                         |
| ----------------- | ------------------------------------------- | ---------------------------------------------------------- |
| Build a parser    | `new Tabnas().use(jsonic).use(Xml, opts?)`  | `j := jsonic.Make(); j.UseDefaults(xml.Xml, xml.Defaults, opts...)` |
| Parse entry       | `instance.parse(src)` (returns the result)  | `j.Parse(src)` (returns `(any, error)`)                    |
| Plugin signature  | `(tn, options) => void`                     | `func(*jsonic.Jsonic, map[string]any) error`               |
| Options type      | `XmlOptions` object                         | `map[string]any` (keys match `xml.Defaults`)               |
| `customEntities`  | `Record<string, string>`                    | `map[string]string`                                        |
| BOM helper        | `decodeBOM(Buffer \| string)`               | `xml.DecodeBOM(string)`                                    |

### Value types

The TypeScript result is the `XmlElement` interface; the Go result is an
untyped tree of plain values:

| Tree value   | TypeScript                       | Go                 |
| ------------ | -------------------------------- | ------------------ |
| an element   | `XmlElement` object              | `map[string]any`   |
| `children`   | `Array<XmlElement \| string>`    | `[]any`            |
| `attributes` | `Record<string, string>`         | `map[string]any` (string values) |
| a text child | `string`                         | `string`           |
| optional fields | absent properties             | absent map keys    |

In embed mode, a number value inside a Jsonic document is a `float64` in
Go (matching `encoding/json`), e.g. `{a:1}` → `map[string]any{"a":
float64(1)}`.

### Error reporting

In TypeScript a malformed parse **throws** the engine error; the specific
code (e.g. `xml_mismatched_tag`) appears in `err.message`. In Go `Parse`
**returns** an `error` and never panics. The Go engine surfaces parse
errors under a single top-level "unexpected" condition, so the specific
code is encoded into the error message text rather than a typed field —
branch on `strings.Contains(err.Error(), code)`. Both runtimes report the
same row/column and the same set of error codes.

### BOM decoding

`decodeBOM` (TS) accepts a Node `Buffer`/`Uint8Array` or a string and
transcodes UTF-8/16/32. `xml.DecodeBOM` (Go) takes and returns a
`string`, transcoding UTF-16/32 to UTF-8 and stripping a UTF-8 BOM. Both
assume UTF-8 when no BOM is present.

For the canonical behaviour and the full option list, see the
[reference](reference.md).
