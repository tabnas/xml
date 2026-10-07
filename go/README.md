# tabnas/xml (Go)

A [tabnas](https://github.com/tabnas/parser) grammar plugin that parses XML
text into a tree of elements, with support for attributes, mixed content,
namespaces, entities, CDATA sections, comments, processing instructions,
and DOCTYPE declarations.

This is the Go module. It is a faithful port of the canonical
TypeScript package [`@tabnas/xml`](../ts) (see [its
README](../ts/README.md)); both pass the same shared conformance
fixtures.

## Install

```sh
go get github.com/tabnas/xml/go
```

The engine (`github.com/tabnas/parser/go`) is pulled in as a dependency;
nothing else is needed. Embed mode, which places XML inside jsonic
source, also needs the jsonic grammar from `github.com/tabnas/jsonic/go`:
see the [guide](doc/guide.md).

## Example

```go
package main

import (
	"fmt"

	tabnas "github.com/tabnas/parser/go"
	tabnasxml "github.com/tabnas/xml/go"
)

func main() {
	j := tabnas.Make()
	if err := j.UseDefaults(tabnasxml.Xml, tabnasxml.Defaults); err != nil {
		panic(err)
	}

	result, _ := j.Parse(`<a title="T" href="/x">Tom &amp; Jerry</a>`)
	el := result.(map[string]any)
	fmt.Println(el["name"], el["children"])
	// a [Tom & Jerry]

	attrs := el["attributes"].(*tabnas.OrderedMap)
	fmt.Println(attrs.Keys, attrs.Vals["href"])
	// [title href] /x
}
```

The result is a tree of Go values: each element is a `map[string]any`
with `name`, `localName`, `attributes` (a `*tabnas.OrderedMap` of
string values, in the order the tag writes them), and `children`
(`[]any`), plus, where they apply, `prefix`, `namespace`, `space`, and
`lang`.

## Documentation

Organised by the [Diátaxis](https://diataxis.fr) framework:

- [Tutorial](doc/tutorial.md). A guided first parse.
- [How-to guide](doc/guide.md). Task recipes (options, errors, embed
  mode).
- [Reference](doc/reference.md). The public API, every option, and the
  accepted XML syntax.
- [Concepts](doc/concepts.md). How the parser works on the engine, plus
  a "Differences from the TS version" section.

## License

Copyright (c) Richard Rodger and other contributors, MIT License.
