# Tutorial — your first XML parse

This walks you from nothing to a working parse of an XML document into a
tree of plain JavaScript objects. Follow it in order; each step builds on
the last. When you finish you will have parsed an element, read its
attributes and children, and seen how namespaces are resolved.

For a recipe-style index of individual tasks, see the
[how-to guide](guide.md). For exhaustive signatures and the full option
list, see the [reference](reference.md).

## 1. Install

The plugin runs on the `tabnas` parser engine with the `jsonic`
relaxed-JSON grammar as its base, so install all three:

```bash
npm install @tabnas/parser @tabnas/jsonic @tabnas/xml
```

## 2. Parse an element

`Xml` is a plugin. Apply it to a `Tabnas` engine that already has
`jsonic` installed, then call `parse`:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

xml.parse('<a>hello</a>')
// => { name: 'a', localName: 'a', attributes: {}, children: ['hello'] }

xml.parse('<a>hello</a>').children   // => ['hello']
```

Every element comes back as an object with four core fields: `name` (the
tag as written), `localName` (the part after any `prefix:`), `attributes`
(a string-to-string map), and `children` (a mixed array of text strings
and nested element objects).

In TypeScript the import is the same, and the result is typed as
`XmlElement`:

```ts
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml, XmlElement } from '@tabnas/xml'

const xml = new Tabnas().use(jsonic).use(Xml)
const doc = xml.parse('<a/>') as XmlElement
```

The parser instance is reusable — call `parse` as many times as you
like.

## 3. Read attributes and mixed content

A real document has attributes, text, and nested elements all at once.
The tree mirrors that structure exactly:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

xml.parse('<greeting lang="en">Hello, <b>world</b>!</greeting>')
// => {
//   name: 'greeting',
//   localName: 'greeting',
//   attributes: { lang: 'en' },
//   children: [
//     'Hello, ',
//     { name: 'b', localName: 'b', attributes: {}, children: ['world'] },
//     '!',
//   ],
// }
```

The `children` array is *ordered* and *mixed*: text runs and child
elements appear in the order they were written. To pick out child
elements, filter for objects; to read text, take the strings.

## 4. Decode entities

The five predefined entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`,
`&apos;`) and numeric character references are decoded for you, in both
text and attribute values:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

xml.parse('<a>Tom &amp; Jerry</a>').children   // => ['Tom & Jerry']
```

## 5. See namespaces resolve

When a document declares a namespace with `xmlns`, every element in
scope is annotated with the resolved `namespace` URI (and, for prefixed
names, a `prefix`):

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

const entry = xml.parse('<entry xmlns="http://www.w3.org/2005/Atom"><title>Example</title></entry>')
entry.namespace                  // => 'http://www.w3.org/2005/Atom'
entry.children[0].namespace      // => 'http://www.w3.org/2005/Atom'

entry
// => {
//   name: 'entry',
//   localName: 'entry',
//   namespace: 'http://www.w3.org/2005/Atom',
//   attributes: { xmlns: 'http://www.w3.org/2005/Atom' },
//   children: [
//     {
//       name: 'title',
//       localName: 'title',
//       namespace: 'http://www.w3.org/2005/Atom',
//       attributes: {},
//       children: ['Example'],
//     },
//   ],
// }
```

The `xmlns` declaration stays in `attributes`; the resolved `namespace`
is added alongside. The child `<title>` inherits the default namespace
without re-declaring it.

## 6. Catch an error

When the input is not well-formed XML the parse throws. Mismatched tags
are a common case:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

let code = ''
try {
  xml.parse('<a></b>')   // close tag does not match open tag
} catch (err) {
  // err.message is a formatted report naming the error code:
  code = /xml_mismatched_tag/.test(err.message) ? 'xml_mismatched_tag' : 'other'
}
code   // => 'xml_mismatched_tag'
```

The thrown error is the engine's error type, with a formatted multi-line
message (source extract plus a hint) suitable for showing a user. See
[Handle parse errors](guide.md#handle-parse-errors) for the full list of
error codes.

## Where to go next

- [How-to guide](guide.md) — focused recipes for individual tasks.
- [Reference](reference.md) — the public API, every option, and the
  accepted XML syntax.
- [Concepts](concepts.md) — how the parser works on the engine, and why.
