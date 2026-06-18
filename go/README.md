# tabnas/xml (Go)

A [Jsonic](https://jsonic.senecajs.org) grammar plugin that parses XML
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

The `jsonic` engine (`github.com/tabnas/jsonic/go`) is pulled in as a
dependency. While building from a source checkout before the modules are
published, clone `https://github.com/tabnas/jsonic` as a sibling of this
repo — the module's `go.mod` resolves `github.com/tabnas/jsonic/go` via a
`replace` directive to `../../jsonic/go`.

## Example

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

	result, _ := j.Parse(`<a>Tom &amp; Jerry</a>`)
	fmt.Println(result)
	// map[attributes:map[] children:[Tom & Jerry] localName:a name:a]
}
```

The result is a tree of plain Go values: each element is a
`map[string]any` with `name`, `localName`, `attributes`
(`map[string]any`), and `children` (`[]any`), plus — where they apply —
`prefix`, `namespace`, `space`, and `lang`.

## Documentation

Organised by the [Diátaxis](https://diataxis.fr) framework:

- [Tutorial](doc/tutorial.md) — a guided first parse.
- [How-to guide](doc/guide.md) — task recipes (options, errors, embed
  mode).
- [Reference](doc/reference.md) — the public API, every option, and the
  accepted XML syntax.
- [Concepts](doc/concepts.md) — how the parser works on the engine, plus
  a "Differences from the TS version" section.

## License

Copyright (c) Richard Rodger and other contributors, MIT License.
