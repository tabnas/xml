# @tabnas/xml

A [Jsonic](https://jsonic.senecajs.org) syntax plugin that parses
XML text into a tree of elements, with support for attributes, mixed
content, namespaces, entities, CDATA sections, comments, processing
instructions, and DOCTYPE declarations.

The same parser is available in two languages — a TypeScript/JavaScript
package on npm and a Go module:

| Language   | Package                                                     | Source                                                       |
| ---------- | ----------------------------------------------------------- | ------------------------------------------------------------ |
| TypeScript | [`@tabnas/xml`](https://npmjs.com/package/@tabnas/xml)      | [`src/xml.ts`](src/xml.ts)                                   |
| Go         | [`github.com/tabnas/xml/go`](https://github.com/tabnas/xml/tree/main/go) | [`go/xml.go`](go/xml.go)            |

[![npm version](https://img.shields.io/npm/v/@tabnas/xml.svg)](https://npmjs.com/package/@tabnas/xml)
[![build](https://github.com/tabnas/xml/actions/workflows/build.yml/badge.svg)](https://github.com/tabnas/xml/actions/workflows/build.yml)
[![Coverage Status](https://coveralls.io/repos/github/tabnas/xml/badge.svg?branch=main)](https://coveralls.io/github/tabnas/xml?branch=main)
[![Known Vulnerabilities](https://snyk.io/test/github/tabnas/xml/badge.svg)](https://snyk.io/test/github/tabnas/xml)
[![DeepScan grade](https://deepscan.io/api/teams/5016/projects/22466/branches/663906/badge/grade.svg)](https://deepscan.io/dashboard#view=project&tid=5016&pid=22466&bid=663906)
[![Maintainability](https://api.codeclimate.com/v1/badges/10e9bede600896c77ce8/maintainability)](https://codeclimate.com/github/tabnas/xml/maintainability)

| ![Voxgig](https://www.voxgig.com/res/img/vgt01r.png) | This open source module is sponsored and supported by [Voxgig](https://www.voxgig.com). |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------- |


## Install

**TypeScript / JavaScript**

```sh
npm install @tabnas/jsonic @tabnas/xml
```

**Go**

```sh
go get github.com/tabnas/xml/go
```


## Quick example

**TypeScript**

```typescript
import { Jsonic } from '@tabnas/jsonic'
import { Xml } from '@tabnas/xml'

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
    jsonic "github.com/tabnas/jsonic/go"
    xml "github.com/tabnas/xml/go"
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
