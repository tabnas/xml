# How-to guide (Go)

Short, task-focused recipes. Each is self-contained and assumes you have
the module installed (see the [tutorial](tutorial.md) for the basics).
For the full option list and accepted syntax, see the
[reference](reference.md).

Every recipe starts from a parser built like this:

```go
import (
	jsonic "github.com/tabnas/jsonic/go"
	xml "github.com/tabnas/xml/go"
)

j := jsonic.Make()
if err := j.UseDefaults(xml.Xml, xml.Defaults); err != nil {
	panic(err)
}
```

To change options, pass a third argument to `UseDefaults` — a
`map[string]any` of overrides (shown per recipe below).

## Parse a document

Call `Parse`; the result is the root element as a tree of
`map[string]any` / `[]any` / `string` values:

```go
result, err := j.Parse(`<doc><child1/><child2><nested>text</nested></child2></doc>`)
if err != nil {
	panic(err)
}
doc := result.(map[string]any)
children := doc["children"].([]any) // child1 and child2 elements
```

A single instance is reusable; build it once and call `Parse` many
times.

## Read attributes

Attributes arrive as a `map[string]any` (string values) on each element.
Quotes (single or double) and entity references in values are handled:

```go
result, _ := j.Parse(`<doc attr1="value1" attr2="value2"/>`)
attrs := result.(map[string]any)["attributes"].(map[string]any)
// attrs["attr1"] == "value1", attrs["attr2"] == "value2"
```

## Add custom entities

Declare extra named entities with the `customEntities` option (itself a
`map[string]string`). Their replacement text is substituted wherever the
named reference appears:

```go
j := jsonic.Make()
j.UseDefaults(xml.Xml, xml.Defaults, map[string]any{
	"customEntities": map[string]string{"nbsp": " ", "copy": "©"},
})

result, _ := j.Parse(`<a>&copy; 2025&nbsp;all rights</a>`)
children := result.(map[string]any)["children"].([]any)
// children[0] == "© 2025 all rights"
```

## Allow unresolved entity references

By default a reference to an undeclared named entity is a hard error
(XML 1.0 §4.1). For templating-style input where unknown `&name;`
sequences should pass through untouched, set `strictEntities: false`:

```go
j := jsonic.Make()
j.UseDefaults(xml.Xml, xml.Defaults, map[string]any{"strictEntities": false})

result, _ := j.Parse(`<a>&unknown;</a>`)
children := result.(map[string]any)["children"].([]any)
// children[0] == "&unknown;"
```

The predefined entities and numeric references are still decoded; only
unknown named references are left verbatim.

## Turn entity decoding off entirely

To keep text and attribute values byte-for-byte as written, set
`entities: false`:

```go
j := jsonic.Make()
j.UseDefaults(xml.Xml, xml.Defaults, map[string]any{"entities": false})

result, _ := j.Parse(`<a>&amp;</a>`)
children := result.(map[string]any)["children"].([]any)
// children[0] == "&amp;"
```

## Turn namespace resolution off

Namespace resolution annotates elements with `prefix` / `namespace` and
rejects unbound prefixes. To skip it — leaving `xmlns` declarations as
plain attributes — set `namespaces: false`:

```go
j := jsonic.Make()
j.UseDefaults(xml.Xml, xml.Defaults, map[string]any{"namespaces": false})

result, _ := j.Parse(`<a xmlns="http://example.com"/>`)
el := result.(map[string]any)
// el has no "namespace" key; the xmlns declaration is in el["attributes"].
```

## Use DOCTYPE entities and defaults

The plugin reads the internal subset of a `<!DOCTYPE ...>` declaration.
`<!ENTITY name "value">` declarations become usable entity references for
that parse, and `<!ATTLIST>` default values are filled in on elements
that omit the attribute:

```go
r1, _ := j.Parse(`<!DOCTYPE doc [<!ENTITY x "world">]><doc>hello &x;!</doc>`)
// r1.children == ["hello world!"]

r2, _ := j.Parse(`<!DOCTYPE doc [<!ATTLIST doc lang CDATA #FIXED "en">]><doc/>`)
// r2.attributes == { "lang": "en" }
```

The DOCTYPE declaration itself is dropped from the output; only its
effects remain.

## Read xml:space and xml:lang

`xml:space` and `xml:lang` are inherited down the tree. The effective
value is recorded on each element as `space` (only when not the default)
and `lang`:

```go
result, _ := j.Parse(`<a xml:lang="fr"><b>bonjour</b></a>`)
a := result.(map[string]any)
// a["lang"] == "fr"
b := a["children"].([]any)[0].(map[string]any)
// b["lang"] == "fr" (inherited)
```

## Embed XML inside a Jsonic document

With `embed: true` the plugin keeps Jsonic's relaxed-JSON grammar and
adds XML literals as values: an `<tag>…</tag>` (or `<tag/>`) may appear
anywhere Jsonic expects a value. Plain Jsonic input is unaffected:

```go
j := jsonic.Make()
j.UseDefaults(xml.Xml, xml.Defaults, map[string]any{"embed": true})

r1, _ := j.Parse(`{a:1, b:"two"}`)
// r1 == map[string]any{"a": float64(1), "b": "two"}

r2, _ := j.Parse(`<a>hello</a>`)
// r2 == an element map with children ["hello"]
```

An XML literal can sit inside a map or list value, and its character data
keeps JSON-syntax characters (commas, colons) intact:

```go
r, _ := j.Parse(`<a>Hello, World!</a>`)
children := r.(map[string]any)["children"].([]any)
// children[0] == "Hello, World!"
```

## Decode a file of unknown encoding

XML files may carry a UTF-8/16/32 byte-order mark. `xml.DecodeBOM`
detects it and returns a decoded UTF-8 string ready to parse:

```go
import "os"

body, _ := os.ReadFile("doc.xml")
result, err := j.Parse(xml.DecodeBOM(string(body)))
```

With no recognised BOM the input is returned unchanged (UTF-8 assumed),
so BOM-less UTF-8 files round-trip correctly.

## Handle parse errors

A failed parse returns an `error`; `Parse` never panics. The message
names the specific error code:

```go
import "strings"

_, err := j.Parse(`<a></b>`)
if err != nil && strings.Contains(err.Error(), "xml_mismatched_tag") {
	// handle the mismatched-tag case
}
```

The error codes (`xml_mismatched_tag`, `unbound_prefix`,
`undeclared_entity`, `duplicate_attribute`, …) are listed in the
[reference](reference.md#errors).
