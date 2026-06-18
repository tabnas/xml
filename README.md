# @tabnas/xml

A grammar plugin for the [Jsonic](https://github.com/tabnas/jsonic) parser
engine that parses XML text into a tree of elements — attributes, mixed
content, namespaces, entities, CDATA, comments, PIs, and DOCTYPE. The
same parser ships in two languages: a TypeScript/JavaScript package on
npm and a Go module.

| Language   | Package                                  | Source                     |
| ---------- | ---------------------------------------- | -------------------------- |
| TypeScript | [`@tabnas/xml`](ts/)                     | [`ts/src/xml.ts`](ts/src/xml.ts) |
| Go         | [`github.com/tabnas/xml/go`](go/)        | [`go/xml.go`](go/xml.go)   |

## Install

```sh
# TypeScript / JavaScript
npm install @tabnas/parser @tabnas/jsonic @tabnas/xml

# Go
go get github.com/tabnas/xml/go
```

## Example

**TypeScript**

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

xml.parse('<greeting lang="en">Hi <b>world</b></greeting>')
// => {
//   name: 'greeting', localName: 'greeting',
//   attributes: { lang: 'en' },
//   children: ['Hi ', { name: 'b', localName: 'b', attributes: {}, children: ['world'] }],
// }
```

**Go**

```go
import (
	jsonic "github.com/tabnas/jsonic/go"
	xml "github.com/tabnas/xml/go"
)

j := jsonic.Make()
j.UseDefaults(xml.Xml, xml.Defaults)
result, _ := j.Parse(`<greeting lang="en">Hi <b>world</b></greeting>`)
```

## Documentation

Each language guide follows the [Diátaxis](https://diataxis.fr)
framework — a tutorial, how-to recipes, a complete reference, and an
explanation of how it works.

| | TypeScript | Go |
| --- | --- | --- |
| Learn | [tutorial](ts/doc/tutorial.md) | [tutorial](go/doc/tutorial.md) |
| Do | [guide](ts/doc/guide.md) | [guide](go/doc/guide.md) |
| Look up | [reference](ts/doc/reference.md) | [reference](go/doc/reference.md) |
| Understand | [concepts](ts/doc/concepts.md) | [concepts](go/doc/concepts.md) |

The repository layout:

| Path | Description |
|---|---|
| [`ts/`](ts/) | TypeScript / JavaScript implementation. |
| [`go/`](go/) | Go port. |
| [`test/spec/`](test/spec/) | Shared conformance fixtures, run by both runtimes. |

## Grammar diagram

The installed grammar as a railroad/syntax diagram, generated from the
live grammar with
[`@tabnas/railroad`](https://github.com/tabnas/railroad):

![xml grammar railroad diagram](ts/doc/grammar.svg)

An ASCII version is in [`ts/doc/grammar.txt`](ts/doc/grammar.txt).

The grammar is defined once in the top-level
[`xml-grammar.jsonic`](xml-grammar.jsonic) and embedded into both
implementations by [`ts/embed-grammar.js`](ts/embed-grammar.js): the text
is spliced verbatim into [`ts/src/xml.ts`](ts/src/xml.ts), and
[`go/xml.go`](go/xml.go) mirrors the same rules. Run
`cd ts && npm run build` (or `npm run embed`) after editing the grammar
to re-embed it.

## License

MIT. Copyright (c) Richard Rodger and other contributors.
