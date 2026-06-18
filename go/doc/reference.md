# Reference (Go)

Complete, dry reference for the `github.com/tabnas/xml/go` package: the
public API, the result shape, every option, the accepted XML syntax, and
the error codes. For a learning path see the [tutorial](tutorial.md); for
task recipes see the [how-to guide](guide.md).

## Install

```bash
go get github.com/tabnas/xml/go@latest
```

The `jsonic` engine (`github.com/tabnas/jsonic/go`) is pulled in as a
dependency.

## Public API

```go
import (
	jsonic "github.com/tabnas/jsonic/go"
	xml "github.com/tabnas/xml/go"
)
```

| Symbol                | Kind                                              | Purpose                                          |
| --------------------- | ------------------------------------------------- | ------------------------------------------------ |
| `xml.Xml`             | `func(*jsonic.Jsonic, map[string]any) error`      | The plugin. Register with `UseDefaults`.         |
| `xml.Defaults`        | `map[string]any`                                  | Default option values to pass to `UseDefaults`.  |
| `xml.DecodeBOM`       | `func(string) string`                             | Strip/transcode a byte-order mark before parse.  |
| `xml.EntityDecoder`   | `func(string, map[string]string) string`          | The entity-decoder function type (advanced).     |
| `xml.Version`         | `string`                                          | The module version string.                       |

## Parse entry

The plugin has no standalone parse function. Make a `jsonic` instance,
register the plugin with `UseDefaults`, and call `Parse`:

```go
j := jsonic.Make()
if err := j.UseDefaults(xml.Xml, xml.Defaults /*, overrides */); err != nil {
	// plugin init failed
}
result, err := j.Parse(source) // source: string
```

`UseDefaults(plugin, defaults, overrides...)` registers the plugin with
its default options, merged with any `map[string]any` overrides. `Parse`
returns `(any, error)`; it returns an `error` on a malformed document and
never panics.

In the default (pure-XML) mode the result is the root element as a
`map[string]any`. In embed mode the result is whatever the outer Jsonic
document evaluates to (see [`embed`](#embed)). The instance is reusable.

## `DecodeBOM(src string) string`

Detects a leading byte-order mark and returns a UTF-8 string ready for
`Parse`:

- UTF-16 LE/BE and UTF-32 LE/BE input is transcoded to UTF-8.
- A UTF-8 BOM is stripped.
- With no recognised BOM the input is returned unchanged (UTF-8 assumed),
  so BOM-less UTF-8 files round-trip correctly.

## The result tree

A parsed document is a tree of plain Go values:

| Tree value      | Go type          | Notes                                        |
| --------------- | ---------------- | -------------------------------------------- |
| an element      | `map[string]any` | keys below                                   |
| `children`      | `[]any`          | mixed: element maps and text strings         |
| a text child    | `string`         |                                              |
| `attributes`    | `map[string]any` | values are `string`                          |

Element map keys:

| Key          | Type     | Present when                                       |
| ------------ | -------- | -------------------------------------------------- |
| `name`       | `string` | always — the qualified name as written             |
| `localName`  | `string` | always — the part after any `prefix:`              |
| `prefix`     | `string` | the name has a prefix (namespaces on)              |
| `namespace`  | `string` | a namespace is in scope (namespaces on)            |
| `space`      | `string` | effective `xml:space` is non-default               |
| `lang`       | `string` | effective `xml:lang` is set                        |
| `attributes` | `map[string]any` | always                                      |
| `children`   | `[]any`  | always                                             |

Namespace declarations (`xmlns`, `xmlns:*`) and `xml:space` / `xml:lang`
remain in `attributes`; their effects are surfaced via `namespace` /
`space` / `lang`.

## Options

Pass options as a `map[string]any` to `UseDefaults` after `xml.Defaults`.
All are optional; defaults shown.

| Key              | Type                | Default             | Effect                                                                 |
| ---------------- | ------------------- | ------------------- | --------------------------------------------------------------------- |
| `namespaces`     | `bool`              | `true`              | Resolve `xmlns`/`xmlns:*` into `prefix`/`localName`/`namespace`.       |
| `entities`       | `bool`              | `true`              | Decode predefined + numeric (and custom/DTD) entities.                |
| `customEntities` | `map[string]string` | `map[string]string{}` | Extra named entities to recognise.                                 |
| `strictEntities` | `bool`              | `true`              | Require every named reference to resolve (XML 1.0 §4.1).               |
| `embed`          | `bool`              | `false`             | Keep Jsonic's grammar and allow XML literals as values.               |

### `namespaces`

When `true`, the tree is walked after parsing and each element is
annotated with `prefix`, `localName`, and the resolved `namespace` from
`xmlns` / `xmlns:*` declarations in scope. The reserved `xml` prefix is
pre-bound; `xml:space` / `xml:lang` are interpreted into `space` / `lang`.
Reserved-prefix/URI misuse raises `reserved_namespace`; a name using an
undeclared prefix raises `unbound_prefix`. When `false`, no resolution
runs.

### `entities`

When `true`, the five predefined entities and numeric character
references (`&#NNN;`, `&#xHHH;`) are decoded in text and attribute values,
along with custom and DOCTYPE-declared named entities. When `false`, no
decoding happens; the source bytes are preserved. Well-formedness checks
on `&` still run.

### `customEntities`

Extra named entities (`name → replacement`) recognised beyond the five
predefined ones. They take precedence over DOCTYPE-declared entities of
the same name and count as "declared" for strict validation.

### `strictEntities`

When `true`, an unknown named entity raises `undeclared_entity`. When
`false`, references to unknown names are left verbatim — useful for
templating. Numeric references and the syntactic check are unaffected.

### `embed`

When `false` (pure-XML mode), the parser is reconfigured to parse XML
only: the start rule becomes `xml`, Jsonic's JSON structural tokens and
value/number/string lexers are disabled, and the JSON value rules are
removed. When `true`, Jsonic's full grammar stays and an XML literal
(`<tag>…</tag>` or `<tag/>`) is added as an alternate to the `val` rule,
so XML elements may appear anywhere a Jsonic value is expected.

## Accepted syntax

The parser accepts well-formed XML 1.0 documents.

- **Elements.** `<name>…</name>`, self-closing `<name/>` / `<name />`,
  and close `</name>`. Exactly one root element (XML 1.0 §2.1); a close
  tag must match its open name (`xml_mismatched_tag`).
- **Names.** XML 1.0 Fifth Edition `NameStartChar` / `NameChar`,
  including Unicode letter/ideograph blocks (so `<เจมส์>`, `<Ωmega>` are
  accepted) plus `-`, `.`, `_`, `:`.
- **Attributes.** `name="value"` or `name='value'`,
  whitespace-separated. A literal `<` in a value is rejected
  (`lt_in_attr_value`); a duplicate name in one tag is rejected
  (`duplicate_attribute`); whitespace in values is normalised to single
  spaces (§3.3.3); entity references are decoded.
- **Text.** Character data becomes a string child, with CR/CRLF→LF
  normalisation (§2.11) and entity decoding. A literal `]]>` outside
  CDATA is rejected (`cdata_terminator_in_text`); illegal control
  characters are rejected (`invalid_xml_char`).
- **CDATA.** `<![CDATA[ … ]]>` is preserved verbatim as a text child (no
  entity decoding), with line endings normalised.
- **Entity references.** `&amp; &lt; &gt; &quot; &apos;`, custom/DTD
  `&name;`, decimal `&#NNN;`, and hex `&#xHHH;`.
- **Ignored markup (dropped):** `<?xml … ?>` and other PIs `<?… ?>`,
  comments `<!-- … -->` (a `--` inside the body is rejected,
  `comment_double_dash`), and `<!DOCTYPE … >`.
- **DOCTYPE internal subset:** `<!ENTITY name "value">` internal general
  entities (usable as `&name;`, recursively expanded with cycle
  detection; parameter/external entities recognised but skipped) and
  `<!ATTLIST element attr type default>` default attribute values (bare
  quoted and `#FIXED "value"` applied to elements that omit the
  attribute; `#REQUIRED`/`#IMPLIED` contribute no default).
- **Namespaces:** `xmlns="uri"` (default namespace) and
  `xmlns:prefix="uri"` (prefix binding), inherited by descendants. The
  `xml` prefix is reserved to `http://www.w3.org/XML/1998/namespace`; the
  `xmlns` prefix and the two reserved URIs may not be (re)declared.

## Errors

A malformed document returns an `error`. `err.Error()` is a formatted
message including the source location and the specific error code.

| Code                       | Raised when                                            |
| -------------------------- | ------------------------------------------------------ |
| `xml_mismatched_tag`       | A close tag does not match its open tag.               |
| `xml_invalid_tag`          | A tag's syntax is not valid XML.                       |
| `xml_unterminated`         | A construct is not terminated (also `unterminated_*`). |
| `comment_double_dash`      | `--` appears inside a comment body.                    |
| `cdata_terminator_in_text` | `]]>` appears in character data outside CDATA.         |
| `pi_target_invalid`        | A processing instruction has a missing/invalid target. |
| `lt_in_attr_value`         | A literal `<` appears in an attribute value.           |
| `bad_entity_ref`           | A malformed `&…;` entity reference.                    |
| `duplicate_attribute`      | A duplicate attribute name in one tag.                 |
| `invalid_xml_char`         | An illegal control character in XML data.              |
| `reserved_namespace`       | Misuse of a reserved namespace prefix or URI.          |
| `unbound_prefix`           | An element/attribute uses an undeclared prefix.        |
| `undeclared_entity`        | A reference to an undeclared entity (strict mode).     |

> Note: the Go engine reports parse errors under a single top-level
> "unexpected" condition, so the specific code is carried in the message
> text rather than a typed field. Branch on `strings.Contains(err.Error(),
> code)`. See [concepts → Differences from the TS version](concepts.md#differences-from-the-ts-version).

## Token model

The plugin's custom lexer emits these tokens, consumed by the grammar
rules:

| Token  | Description                                            |
| ------ | ----------------------------------------------------- |
| `#XOP` | XML open tag `<name …>`                                |
| `#XSC` | XML self-closing tag `<name …/>`                       |
| `#XCL` | XML closing tag `</name>`                              |
| `#XIG` | Ignored markup: comment, PI, or DOCTYPE               |
| `#TX`  | Text / character data between tags (entities decoded) |

For how these fit together as a grammar on the engine — and how the Go
port differs from the TypeScript version — see [concepts](concepts.md).
