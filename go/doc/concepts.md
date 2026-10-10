# Concepts (Go)

Background on how the Go `xml` package works, and why it is built the way
it is. This is understanding-oriented reading; for steps see the
[tutorial](tutorial.md) and [how-to guide](guide.md); for exact
signatures and options see the [reference](reference.md).

## A grammar plugin on the tabnas engine

This package is not a standalone XML parser. It is a **plugin** for the
`tabnas` engine (`github.com/tabnas/parser/go`), the engine under the
relaxed-JSON `jsonic` parser too. The engine is a configurable,
matcher-based lexer plus a rule-based parser. This plugin adds XML by
configuring that same machinery (a custom lexer matcher, four grammar
rules, and option-driven reconfiguration) rather than hand-writing a
parser. Error reporting, source-location tracking, and the option system
all come from the engine.

## Two stages: a custom lexer, then six rules

A parse runs in the engine's two cooperating stages.

The **lexer** turns source text into tokens. The plugin registers one
custom matcher (`xmltag`, at a high priority order) that recognises
everything starting with `<` and emits five token kinds:

- `#XOP`. Open tag, carrying `map[string]any{"name", "attributes"}`,
  with the attributes in a `*tabnas.OrderedMap` in source order
- `#XSC`. Self-closing tag, carrying the same
- `#XCL`. Close tag, carrying the name `string`
- `#TX`. A run of character data (or a CDATA body)
- `#XIG`. An *ignored* construct (comment, PI, DOCTYPE), dropped via the
  parser's IGNORE token set

The matcher does the lexical work: rune-aware name scanning (Unicode
`NameStartChar` / `NameChar`, including non-BMP runes), attribute
parsing, entity decoding, end-of-line and attribute-value normalisation,
and the well-formedness checks (illegal characters, `]]>` in text, `--`
in comments, `<` in attribute values, malformed `&` references). It
tracks XML nesting depth so that while inside an open element it claims
the whole run up to the next `<` as a single `#TX` token.

The **parser** then consumes those tokens with six rules (`xml`,
`element`, `head`, `content`, `children`, `child`) each with open/close
phases and short alternates with at most two tokens of lookahead. The
next `child` replaces the one before it, so a run of siblings loops in
place rather than nesting rule inside rule. The grammar is small enough
to read in one screen; it lives in the repository's top-level
`xml-grammar.jsonic` (authored once, in relaxed-JSON) and is mirrored
here as a `tabnas.GrammarSpec`. The `@`-prefixed function references in
the grammar are resolved at plugin time against Go callbacks that build
the result tree and enforce the structural constraints (single root,
matching close tags).

## Two modes from one grammar

**Pure-XML mode** (`embed: false`, the default) reconfigures the engine
around the XML rules: the start rule becomes `xml`, the JSON structural
tokens are unbound, the number/string/value/comment/space lexers are
turned off, and any value rules jsonic installed first (`val`, `map`,
`list`, `pair`, `elem`), unreachable now, are deleted. A dummy fixed token bound to an
illegal XML character is registered so the lexer keeps a non-empty fixed
table; without it, XML text containing a comma would be truncated at the
comma. The input is then pure XML.

**Embed mode** (`embed: true`) runs on a jsonic instance, and the plugin
refuses to install on any other. It leaves
jsonic's grammar intact and adds an XML literal as an alternate of the
`val` rule. When the parser is looking
for a value and sees `#XOP`/`#XSC`, it backtracks one token and pushes
the `element` rule, building an XML subtree wherever a value was expected.

## The rules build the value in document order

The rules build each element in the order its value lists the members:
`name`, `localName`, and `attributes` when the parser reads the start
tag, then `children` one child at a time, then whichever of `prefix`,
`namespace`, `space`, and `lang` apply. Every element and every list of
children gets a node of its own when it starts. So the rule events of a
parse show the tree in the order a writer would emit it, and a streaming
consumer can follow a document as the parser reads it, without waiting
for the whole value.
A Go map keeps no order of its own, so the plugin declares that order
for each parse in `ctx.Meta["fields"]`.

## Namespaces, space, and lang, resolved at each start tag

The parser resolves the names of an element when it reads the start tag,
against a scope the element inherits from its parent (the prefix→URI
bindings, the active `xml:space`, the active `xml:lang`). The scope
starts with the reserved `xml` prefix bound, and the declarations on an
element apply to the element itself and everything inside it.
Resolution rejects reserved-prefix/URI misuse and unbound prefixes, and
records `prefix` / `namespace` / `space` / `lang` only where they apply.

Start tags arrive in document order, so the first violation resolution
meets is the first in the document. In pure mode it fails the parse once
the root element is complete, and a document that breaks a
well-formedness rule fails for that reason first. In embed mode the
parser resolves each XML literal on its own, a violation fails nothing,
and the elements after the first violation stay unresolved. Turning
`namespaces` off skips resolution.

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
| Build a parser    | `new Tabnas().use(Xml, opts?)`              | `j := tabnas.Make(); j.UseDefaults(tabnasxml.Xml, tabnasxml.Defaults, opts...)` |
| Parse entry       | `instance.parse(src)` (returns the result)  | `j.Parse(src)` (returns `(any, error)`)                    |
| Plugin signature  | `(tn, options) => void`                     | `func(*tabnas.Tabnas, map[string]any) error`                     |
| Options type      | `XmlOptions` object                         | `map[string]any` (keys match `tabnasxml.Defaults`)               |
| `customEntities`  | `Record<string, string>`                    | `map[string]string`                                        |
| BOM helper        | `decodeBOM(Buffer \| string)`               | `tabnasxml.DecodeBOM(string)`                                    |

### Value types

The TypeScript result is the `XmlElement` interface; the Go result is an
untyped tree of values:

| Tree value   | TypeScript                       | Go                 |
| ------------ | -------------------------------- | ------------------ |
| an element   | `XmlElement` object              | `map[string]any`   |
| `children`   | `Array<XmlElement \| string>`    | `[]any`            |
| `attributes` | `Record<string, string>`         | `*tabnas.OrderedMap` (string values) |
| a text child | `string`                         | `string`           |
| optional fields | absent properties             | absent map keys    |

Both keep an element's attributes in the order the tag writes them, with
any DOCTYPE defaults after them. A Go map has no order, which is why the
attributes are the engine's ordered map rather than a `map[string]any`.

In embed mode, a Jsonic object is a `*tabnas.OrderedMap` in Go, with its
keys in source order, and a number value is a `float64` (matching
`encoding/json`): `{a:1}` gives an ordered map whose `a` is `float64(1)`.

### Error reporting

In TypeScript a malformed parse **throws** the engine error; the specific
code (for example `xml_mismatched_tag`) appears in `err.message`. In Go `Parse`
**returns** an `error` and never panics. The Go engine surfaces parse
errors under a single top-level "unexpected" condition, so the specific
code is encoded into the error message text rather than a typed field;
branch on `strings.Contains(err.Error(), code)`. Both runtimes report the
same row/column and the same set of error codes.

### BOM decoding

`decodeBOM` (TS) accepts a Node `Buffer`/`Uint8Array` or a string and
transcodes UTF-8/16/32. `tabnasxml.DecodeBOM` (Go) takes and returns a
`string`, transcoding UTF-16/32 to UTF-8 and stripping a UTF-8 BOM. Both
assume UTF-8 when no BOM is present.

For the canonical behaviour and the full option list, see the
[reference](reference.md).
