# @jsonic/xml

A [Jsonic](https://jsonic.senecajs.org) syntax plugin that parses
XML text into a tree of elements, with support for attributes, mixed
content, namespaces, entities, CDATA sections, comments, processing
instructions, and DOCTYPE declarations.

The same parser is available in two languages — a TypeScript/JavaScript
package on npm and a Go module:

| Language   | Package                                                     | Source                                                       |
| ---------- | ----------------------------------------------------------- | ------------------------------------------------------------ |
| TypeScript | [`@jsonic/xml`](https://npmjs.com/package/@jsonic/xml)      | [`src/xml.ts`](src/xml.ts)                                   |
| Go         | [`github.com/jsonicjs/xml/go`](https://github.com/jsonicjs/xml/tree/main/go) | [`go/xml.go`](go/xml.go)            |

[![npm version](https://img.shields.io/npm/v/@jsonic/xml.svg)](https://npmjs.com/package/@jsonic/xml)
[![build](https://github.com/jsonicjs/xml/actions/workflows/build.yml/badge.svg)](https://github.com/jsonicjs/xml/actions/workflows/build.yml)
[![Coverage Status](https://coveralls.io/repos/github/jsonicjs/xml/badge.svg?branch=main)](https://coveralls.io/github/jsonicjs/xml?branch=main)
[![Known Vulnerabilities](https://snyk.io/test/github/jsonicjs/xml/badge.svg)](https://snyk.io/test/github/jsonicjs/xml)
[![DeepScan grade](https://deepscan.io/api/teams/5016/projects/22466/branches/663906/badge/grade.svg)](https://deepscan.io/dashboard#view=project&tid=5016&pid=22466&bid=663906)
[![Maintainability](https://api.codeclimate.com/v1/badges/10e9bede600896c77ce8/maintainability)](https://codeclimate.com/github/jsonicjs/xml/maintainability)

| ![Voxgig](https://www.voxgig.com/res/img/vgt01r.png) | This open source module is sponsored and supported by [Voxgig](https://www.voxgig.com). |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------- |


## Install

**TypeScript / JavaScript**

```sh
npm install jsonic @jsonic/xml
```

**Go**

```sh
go get github.com/jsonicjs/xml/go
```


## Quick example

**TypeScript**

```typescript
import { Jsonic } from 'jsonic'
import { Xml } from '@jsonic/xml'

const parse = Jsonic.make().use(Xml)

parse('<greeting lang="en">Hello, <b>world</b>!</greeting>')
// {
//   name: 'greeting', localName: 'greeting',
//   attributes: { lang: 'en' },
//   children: [ 'Hello, ',
//               { name: 'b', localName: 'b', attributes: {}, children: ['world'] },
//               '!' ]
// }
```

**Go**

```go
import (
    jsonic "github.com/jsonicjs/jsonic/go"
    xml "github.com/jsonicjs/xml/go"
)

j := jsonic.Make()
j.UseDefaults(xml.Xml, xml.Defaults)
result, _ := j.Parse(`<greeting lang="en">Hello, <b>world</b>!</greeting>`)
```


## Documentation

Documentation is organised by the [Diataxis](https://diataxis.fr)
framework — each language guide contains a tutorial, how-to recipes,
a reference section, and a short explanation of design choices:

- [TypeScript guide](doc/xml-ts.md)
- [Go guide](doc/xml-go.md)


## License

Copyright (c) 2021-2025 Richard Rodger and other contributors,
[MIT License](LICENSE).
