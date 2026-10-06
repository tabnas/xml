# @tabnas/xml

A [tabnas](https://github.com/tabnas/parser) grammar plugin that parses XML
text into a tree of elements, with support for attributes, mixed content,
namespaces, entities, CDATA sections, comments, processing instructions,
and DOCTYPE declarations.

This is the TypeScript / JavaScript package. A Go port lives in
[`../go`](../go) (see [its README](../go/README.md)).

[![npm version](https://img.shields.io/npm/v/@tabnas/xml.svg)](https://npmjs.com/package/@tabnas/xml)
[![CI](https://github.com/tabnas/xml/actions/workflows/ci.yml/badge.svg)](https://github.com/tabnas/xml/actions/workflows/ci.yml)

## Install

```sh
npm install @tabnas/parser @tabnas/xml
```

`@tabnas/parser` (the engine) is a peer dependency, and the only one.
Embed mode, which places XML inside jsonic source, also needs
`@tabnas/jsonic` installed (see the [guide](doc/guide.md)).

## Example

```js
const { Tabnas } = require('@tabnas/parser')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(Xml)

xml.parse('<a>Tom &amp; Jerry</a>').children   // => ['Tom & Jerry']
```

The result is an `XmlElement` tree: each element has `name`, `localName`,
`attributes`, `children`, and (where they apply) `prefix`,
`namespace`, `space`, and `lang`.

## Documentation

Organised by the [Diátaxis](https://diataxis.fr) framework:

- [Tutorial](doc/tutorial.md). A guided first parse.
- [How-to guide](doc/guide.md). Task recipes (options, errors, embed
  mode).
- [Reference](doc/reference.md). The public API, every option, and the
  accepted XML syntax.
- [Concepts](doc/concepts.md). How the parser works on the engine, and
  why.

## Grammar diagram

The installed grammar as a railroad/syntax diagram, generated from the
live grammar with
[`@tabnas/railroad`](https://github.com/tabnas/railroad):

![xml grammar railroad diagram](doc/grammar.svg)

A vertical ASCII version is in [`doc/grammar.txt`](doc/grammar.txt). The
grammar source lives in the repository's top-level `xml-grammar.jsonic`
and is embedded into [`src/xml.ts`](src/xml.ts) as JSON by
`embed-grammar.js` (run via `npm run embed`).

## License

Copyright (c) Richard Rodger and other contributors,
[MIT License](LICENSE).
