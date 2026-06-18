# Reference

Complete, dry reference for the `@tabnas/xml` plugin: the public API, the
result shape, every option, the accepted XML syntax, and the error codes.
For a learning path see the [tutorial](tutorial.md); for task recipes see
the [how-to guide](guide.md).

## Install

```bash
npm install @tabnas/parser @tabnas/jsonic @tabnas/xml
```

`@tabnas/parser` (the engine) and `@tabnas/jsonic` (the base grammar) are
peer dependencies; the plugin is applied on top of them.

## Exports

```ts
import { Xml, decodeBOM } from '@tabnas/xml'
import type { XmlOptions, XmlElement } from '@tabnas/xml'
```

| Export       | Kind                | Purpose                                             |
| ------------ | ------------------- | --------------------------------------------------- |
| `Xml`        | `Plugin`            | The plugin. Apply with `.use(Xml, options?)`.       |
| `decodeBOM`  | `(src) => string`   | Strip/transcode a byte-order mark before parsing.   |
| `XmlOptions` | type                | The options object accepted by `Xml`.               |
| `XmlElement` | type                | The shape of a parsed element node.                  |

`Xml.defaults` holds the default options object (see [Options](#options)).

## Parse entry

The plugin has no standalone parse function. Build a parser by applying
`jsonic` and then `Xml` to a `Tabnas` engine, and call its `parse`
method:

```ts
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml } from '@tabnas/xml'

const xml = new Tabnas().use(jsonic).use(Xml /*, options */)

const result = xml.parse(source) // source: string
```

`parse` returns the parsed result and throws on a malformed document.
In the default (pure-XML) mode the result is the root `XmlElement`. In
embed mode the result is whatever the outer Jsonic document evaluates to
(see [`embed`](#embed)).

The engine instance is reusable; build it once and call `parse` per
document. Deriving a child with different options re-runs the plugin, so
options are fixed at `use` time.

## `decodeBOM(src)`

`decodeBOM(src: Buffer | Uint8Array | string): string`

Detects a leading byte-order mark and returns a decoded Unicode JS
string ready for `parse`:

- UTF-16 LE/BE and UTF-32 LE/BE input is transcoded to a JS string.
- A UTF-8 BOM is stripped.
- With no BOM, UTF-8 is assumed (so BOM-less UTF-8 files with non-ASCII
  names round-trip correctly).
- A string argument that is already decoded Unicode only has a leading
  `U+FEFF` stripped; a Latin-1 "binary" string is treated as bytes.

## The result: `XmlElement`

```ts
type XmlElement = {
  name: string                      // qualified name as written, e.g. "ns:tag"
  prefix?: string                   // namespace prefix, if the name has one
  localName: string                 // local part of the name, e.g. "tag"
  namespace?: string                // resolved namespace URI, if in scope
  space?: string                    // effective xml:space, only when non-default
  lang?: string                     // effective xml:lang, only when set
  attributes: Record<string, string>
  children: Array<XmlElement | string>
}
```

Notes:

- `name` is exactly the source tag name. `localName` is the part after a
  `prefix:`, or the whole name if unprefixed.
- `prefix`, `namespace`, `space`, `lang` are **present only when they
  apply**. Plain documents do not sprout these fields. (After a
  `JSON.stringify` round-trip, absent fields simply do not appear.)
- `attributes` is a string-to-string map. Namespace declarations
  (`xmlns`, `xmlns:*`) and the special `xml:space` / `xml:lang`
  attributes remain in this map; their *effects* are surfaced via
  `namespace` / `space` / `lang`.
- `children` is an ordered, mixed array: text runs are strings, nested
  elements are `XmlElement` objects, interleaved in source order.

## Options

Pass options as the second argument to `.use(Xml, options)`. All are
optional; defaults shown.

```ts
type XmlOptions = {
  namespaces: boolean                     // default: true
  entities: boolean                       // default: true
  customEntities: Record<string, string>  // default: {}
  strictEntities: boolean                 // default: true
  embed: boolean                          // default: false
}
```

### `namespaces`

`boolean`, default `true`.

When `true`, the tree is walked after parsing and each element is
annotated with `prefix`, `localName`, and the resolved `namespace` URI
from `xmlns` / `xmlns:*` declarations in scope. The reserved `xml`
prefix is pre-bound; `xml:space` and `xml:lang` are interpreted and
surfaced as `space` / `lang`. Misuse of a reserved prefix or URI raises
`reserved_namespace`; a name using an undeclared prefix raises
`unbound_prefix`.

When `false`, no resolution runs: `xmlns` declarations stay as plain
attributes, no `namespace` field is added, and unbound prefixes are not
errors.

### `entities`

`boolean`, default `true`.

When `true`, the five predefined entities and numeric character
references (`&#NNN;`, `&#xHHH;`) are decoded in text and attribute
values, along with custom and DOCTYPE-declared named entities. When
`false`, no decoding happens and the source bytes are preserved (e.g.
`&amp;` stays `&amp;`). Well-formedness checks on `&` references still
run.

### `customEntities`

`Record<string, string>`, default `{}`.

Extra named entities recognised beyond the five predefined ones, given
as `name → replacement`. These take precedence over DOCTYPE-declared
entities of the same name (matching the rule that predefined entities are
always available). Custom entities also count as "declared" for strict
validation.

### `strictEntities`

`boolean`, default `true`.

When `true`, every named entity reference must resolve to a declared
entity (predefined, `customEntities`, or a DOCTYPE `<!ENTITY>`); an
unknown name raises `undeclared_entity` (XML 1.0 §4.1). When `false`,
references to unknown names are left verbatim in the output — useful for
templating. Numeric references and the syntactic check are unaffected.

### `embed`

`boolean`, default `false`.

When `false` (pure-XML mode), the plugin reconfigures the parser to parse
XML only: the start rule becomes `xml`, Jsonic's JSON structural tokens
and value/number/string lexers are disabled, and the JSON value rules are
removed from the grammar.

When `true`, Jsonic's full relaxed-JSON grammar stays in place and an XML
literal (`<tag>…</tag>` or `<tag/>`) is added as an alternate to the
`val` rule, so an XML element may appear anywhere a Jsonic value is
expected. Plain Jsonic input parses normally; an XML literal builds an
`XmlElement` subtree in place.

## Accepted syntax

The parser accepts well-formed XML 1.0 documents and the following
constructs.

### Elements

| Form                      | Notes                                        |
| ------------------------- | -------------------------------------------- |
| `<name>…</name>`          | Open + close; children in between.           |
| `<name/>` / `<name />`    | Self-closing empty element.                  |
| `</name>`                 | Close tag; must match the open name.         |

A document has exactly one root element (XML 1.0 §2.1). A close tag must
match the corresponding open tag name or `xml_mismatched_tag` is raised.

### Names

Element and attribute names follow XML 1.0 Fifth Edition `NameStartChar`
/ `NameChar`, including the Unicode letter and ideograph blocks (so
non-ASCII names such as `<เจมส์>` or `<Ωmega>` are accepted), plus `-`,
`.`, `_`, and `:`.

### Attributes

`name="value"` or `name='value'`, whitespace-separated, inside an open or
self-closing tag. Rules enforced:

- Values may use single or double quotes; the other quote may appear
  unescaped inside.
- A literal `<` in a value is rejected (`lt_in_attr_value`).
- A duplicate attribute name in one tag is rejected
  (`duplicate_attribute`).
- Entity references in values are decoded (subject to `entities`); `&`
  must begin a well-formed reference.
- Whitespace in values is normalised per XML 1.0 §3.3.3: TAB, LF, CR, and
  CRLF each become a single space.

### Text and character data

Text between tags is a child string. Applied to text:

- End-of-line normalisation (§2.11): CR and CRLF become LF.
- Entity references decoded (subject to `entities`).
- The literal `]]>` is rejected outside CDATA
  (`cdata_terminator_in_text`).
- Illegal control characters are rejected (`invalid_xml_char`).

`<![CDATA[ … ]]>` sections are preserved verbatim as a text child — no
entity decoding — with line endings normalised.

### Entity references

| Form        | Meaning                                  |
| ----------- | ---------------------------------------- |
| `&amp;` etc. | The five predefined entities.           |
| `&name;`    | Custom or DOCTYPE-declared named entity. |
| `&#NNN;`    | Decimal numeric character reference.     |
| `&#xHHH;`   | Hexadecimal numeric character reference. |

### Prolog and ignored markup

These are recognised and **dropped** from the output:

- `<?xml … ?>` declaration and other processing instructions `<?… ?>`
  (the target must be a valid Name).
- Comments `<!-- … -->` (a `--` inside the body is rejected,
  `comment_double_dash`).
- `<!DOCTYPE … >`, including a `[ … ]` internal subset.

From a DOCTYPE internal subset the parser additionally reads:

- `<!ENTITY name "value">` — internal general entities, usable as
  `&name;` for that parse (recursively expanded, with cycle detection).
  Parameter (`% name`) and external (`SYSTEM`/`PUBLIC`) entities are
  recognised and skipped.
- `<!ATTLIST element attr type default>` — default attribute values.
  Bare quoted defaults and `#FIXED "value"` are applied to elements that
  omit the attribute; `#REQUIRED` / `#IMPLIED` contribute no default.

### Namespaces

With `namespaces: true` (default): `xmlns="uri"` sets the default
namespace; `xmlns:prefix="uri"` binds a prefix; both are inherited by
descendants. The `xml` prefix is reserved to
`http://www.w3.org/XML/1998/namespace`; the `xmlns` prefix and the two
reserved URIs may not be (re)declared. See [`namespaces`](#namespaces).

## Errors

A malformed document throws the engine error type. `err.message` is a
formatted, multi-line report including the source location, a one-line
message, and a longer hint; the specific error code appears in the text.

| Code                       | Raised when                                                |
| -------------------------- | ---------------------------------------------------------- |
| `xml_mismatched_tag`       | A close tag does not match its open tag.                   |
| `xml_invalid_tag`          | A tag's syntax is not valid XML.                           |
| `xml_unterminated`         | A construct is not terminated (also `unterminated_*`).     |
| `comment_double_dash`      | `--` appears inside a comment body.                        |
| `cdata_terminator_in_text` | `]]>` appears in character data outside CDATA.             |
| `pi_target_invalid`        | A processing instruction has a missing/invalid target.     |
| `lt_in_attr_value`         | A literal `<` appears in an attribute value.               |
| `bad_entity_ref`           | A malformed `&…;` entity reference.                        |
| `duplicate_attribute`      | A duplicate attribute name in one tag.                     |
| `invalid_xml_char`         | An illegal control character in XML data.                  |
| `reserved_namespace`       | Misuse of a reserved namespace prefix or URI.              |
| `unbound_prefix`           | An element/attribute uses an undeclared prefix.            |
| `undeclared_entity`        | A reference to an undeclared entity (strict mode).         |

## Token model

The plugin's custom lexer emits these tokens (visible in the railroad
diagram legend); the grammar rules consume them:

| Token | Description                                              |
| ----- | ------------------------------------------------------- |
| `#XOP` | XML open tag `<name …>`                                 |
| `#XSC` | XML self-closing tag `<name …/>`                        |
| `#XCL` | XML closing tag `</name>`                               |
| `#XIG` | Ignored markup: comment, PI, or DOCTYPE                 |
| `#TX`  | Text / character data between tags (entities decoded)   |

For how these fit together as a grammar on the engine, see
[concepts](concepts.md).
