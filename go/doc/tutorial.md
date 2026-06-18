# Tutorial — your first XML parse (Go)

This walks you from nothing to a working parse of an XML document into a
tree of Go values. Follow it in order; each step builds on the last. When
you finish you will have parsed an element, read its attributes and
children, and seen how namespaces are resolved.

For a recipe-style index of individual tasks, see the
[how-to guide](guide.md). For exhaustive signatures and the full option
list, see the [reference](reference.md).

## 1. Install

```bash
go get github.com/tabnas/xml/go@latest
```

The plugin runs on the `jsonic` engine
(`github.com/tabnas/jsonic/go`), which it pulls in as a dependency.
(While building from a source checkout before the modules are published,
see the sibling-checkout note in the [README](../README.md).)

## 2. Parse an element

`xml.Xml` is a plugin. Register it on a `jsonic` instance with
`UseDefaults`, then call `Parse`:

```go
package main

import (
	"fmt"

	jsonic "github.com/tabnas/jsonic/go"
	xml "github.com/tabnas/xml/go"
)

func main() {
	j := jsonic.Make()
	if err := j.UseDefaults(xml.Xml, xml.Defaults); err != nil {
		panic(err)
	}

	result, err := j.Parse(`<a>hello</a>`)
	if err != nil {
		panic(err)
	}
	fmt.Println(result)
	// map[attributes:map[] children:[hello] localName:a name:a]
}
```

Run it with `go run .`. Every element comes back as a
`map[string]any` with four core keys: `name` (the tag as written),
`localName` (the part after any `prefix:`), `attributes` (a
`map[string]any` of string values), and `children` (a `[]any` mixing
text strings and nested element maps).

## 3. Inspect the result

`Parse` returns `any`. For an XML document the root is always a
`map[string]any`, so type-assert and read fields directly:

```go
result, _ := j.Parse(`<a>hello</a>`)
el := result.(map[string]any)

fmt.Println(el["name"])      // a
children := el["children"].([]any)
fmt.Println(children[0])     // hello
```

The concrete types in the tree are predictable:

| Tree value      | Go type          |
| --------------- | ---------------- |
| an element      | `map[string]any` |
| the `children`  | `[]any`          |
| a text child    | `string`         |
| an attribute    | `string`         |

## 4. Read attributes and mixed content

A real document has attributes, text, and nested elements at once. The
tree mirrors that structure exactly:

```go
result, _ := j.Parse(`<greeting lang="en">Hello, <b>world</b>!</greeting>`)
el := result.(map[string]any)

attrs := el["attributes"].(map[string]any)
fmt.Println(attrs["lang"])           // en

children := el["children"].([]any)
fmt.Println(children[0])             // Hello,
b := children[1].(map[string]any)
fmt.Println(b["name"])               // b
fmt.Println(children[2])             // !
```

The `children` slice is ordered and mixed: text runs and child elements
appear in source order. The five predefined entities (`&amp;`, `&lt;`,
…) and numeric references are decoded for you.

## 5. See namespaces resolve

When a document declares a namespace with `xmlns`, every element in scope
gains a resolved `namespace` value (and, for prefixed names, a
`prefix`):

```go
result, _ := j.Parse(
	`<entry xmlns="http://www.w3.org/2005/Atom"><title>Example</title></entry>`)
el := result.(map[string]any)

fmt.Println(el["namespace"]) // http://www.w3.org/2005/Atom
child := el["children"].([]any)[0].(map[string]any)
fmt.Println(child["namespace"]) // http://www.w3.org/2005/Atom
```

The `xmlns` declaration stays in `attributes`; the resolved `namespace`
is added alongside, and the child `<title>` inherits it.

## 6. Catch an error

When the input is not well-formed XML, `Parse` returns an `error` — it
never panics. Mismatched tags are a common case:

```go
_, err := j.Parse(`<a></b>`)
if err != nil {
	fmt.Println(err)
	// the message names the cause: xml_mismatched_tag
}
```

`err.Error()` renders a formatted message with the source location and
the specific error code (here `xml_mismatched_tag`), suitable for
showing a user. See [Handle parse errors](guide.md#handle-parse-errors)
for the full list of codes.

## Where to go next

- [How-to guide](guide.md) — focused recipes for individual tasks.
- [Reference](reference.md) — the public API, every option, and the
  accepted XML syntax.
- [Concepts](concepts.md) — how the parser works on the engine, and how
  the Go port differs from the TypeScript version.
