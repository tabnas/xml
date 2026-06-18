# How-to guide

Short, task-focused recipes. Each is self-contained and assumes you have
the plugin installed (see the [tutorial](tutorial.md) for the basics).
For the full option list and accepted syntax, see the
[reference](reference.md).

Every recipe starts from a parser built like this:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')
```

## Parse a document

Apply the plugin and call `parse`. The result is the root element as a
tree of objects:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

xml.parse('<doc><child1/><child2><nested>text</nested></child2></doc>')
// => {
//   name: 'doc', localName: 'doc', attributes: {},
//   children: [
//     { name: 'child1', localName: 'child1', attributes: {}, children: [] },
//     { name: 'child2', localName: 'child2', attributes: {}, children: [
//       { name: 'nested', localName: 'nested', attributes: {}, children: ['text'] },
//     ] },
//   ],
// }
```

A single configured instance is reusable; build it once and call `parse`
many times.

## Read attributes

Attributes arrive as a string-to-string map on each element. Quotes
(single or double) and entity references in values are handled for you:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

xml.parse('<doc attr1="value1" attr2="value2"/>').attributes
// => { attr1: 'value1', attr2: 'value2' }
```

## Add custom entities

Beyond the five predefined entities, declare extra named entities with
the `customEntities` option. Their replacement text is substituted
wherever the named reference appears:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml, {
  customEntities: { nbsp: ' ', copy: '©' },
})

xml.parse('<a>&copy; 2025&nbsp;all rights</a>').children
// => ['© 2025 all rights']
```

You can also declare entities inline in a DOCTYPE internal subset — see
[Use DOCTYPE entities and defaults](#use-doctype-entities-and-defaults).

## Allow unresolved entity references

By default a reference to an undeclared named entity is a hard error
(XML 1.0 §4.1). For templating-style input where unknown `&name;`
sequences should pass through untouched, set `strictEntities: false`:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml, { strictEntities: false })

xml.parse('<a>&unknown;</a>').children
// => ['&unknown;']
```

The five predefined entities and numeric references are still decoded;
only *unknown named* references are left verbatim.

## Turn entity decoding off entirely

To keep text and attribute values byte-for-byte as written (no entity
decoding at all), set `entities: false`:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml, { entities: false })

xml.parse('<a>&amp;</a>').children
// => ['&amp;']
```

## Turn namespace resolution off

Namespace resolution annotates elements with `prefix` / `namespace` and
rejects unbound prefixes. To skip it — leaving `xmlns` declarations as
plain attributes and never adding `namespace` — set `namespaces: false`:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml, { namespaces: false })

xml.parse('<a xmlns="http://example.com"/>')
// => {
//   name: 'a', localName: 'a',
//   attributes: { xmlns: 'http://example.com' },
//   children: [],
// }
```

## Use DOCTYPE entities and defaults

The plugin reads the internal subset of a `<!DOCTYPE ...>` declaration.
`<!ENTITY name "value">` declarations become usable entity references for
that parse, and `<!ATTLIST>` default values are filled in on elements
that omit the attribute:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

xml.parse('<!DOCTYPE doc [<!ENTITY x "world">]><doc>hello &x;!</doc>').children
// => ['hello world!']

xml.parse('<!DOCTYPE doc [<!ATTLIST doc lang CDATA #FIXED "en">]><doc/>').attributes
// => { lang: 'en' }
```

The DOCTYPE declaration itself is dropped from the output; only its
effects (declared entities, attribute defaults) remain.

## Read xml:space and xml:lang

`xml:space` and `xml:lang` are inherited down the tree. The effective
value is recorded on each element as `space` (only when not the default)
and `lang`:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

const r = xml.parse('<a xml:lang="fr"><b>bonjour</b></a>')
r.lang             // => 'fr'
r.children[0].lang // => 'fr'
```

## Embed XML inside a Jsonic document

With `embed: true` the plugin keeps Jsonic's relaxed-JSON grammar and
adds XML literals as values: an `<tag>…</tag>` (or `<tag/>`) may appear
anywhere Jsonic expects a value. Plain Jsonic input is unaffected:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const j = new Tabnas().use(jsonic).use(Xml, { embed: true })

j.parse('{a:1, b:"two"}')   // => { a: 1, b: 'two' }
j.parse('<a>hello</a>')     // => { name: 'a', localName: 'a', attributes: {}, children: ['hello'] }
```

An XML literal can sit inside a map or list value, and its character data
keeps JSON-syntax characters (commas, colons) intact:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const j = new Tabnas().use(jsonic).use(Xml, { embed: true })

j.parse('<a>Hello, World!</a>').children   // => ['Hello, World!']
```

## Decode a file of unknown encoding

XML files may carry a UTF-8/16/32 byte-order mark. `decodeBOM` detects it
and returns a decoded JS string ready to parse; pass it a Node `Buffer`
(or a Latin-1 "binary" string):

```js
const { readFileSync } = require('node:fs')
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml, decodeBOM } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)
const body = decodeBOM(readFileSync('doc.xml')) // Buffer in, string out
const doc = xml.parse(body)
```

With no BOM, UTF-8 is assumed, so BOM-less UTF-8 files (including
non-ASCII tag names) round-trip unchanged.

## Handle parse errors

A failed parse throws the engine's error. Catch it and read the message,
which names the specific error code:

```js
const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('@tabnas/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

let code = ''
try {
  xml.parse('<a></b>')
} catch (err) {
  code = /xml_mismatched_tag/.test(err.message) ? 'xml_mismatched_tag' : 'other'
}
code   // => 'xml_mismatched_tag'
```

The error codes (`xml_mismatched_tag`, `unbound_prefix`,
`undeclared_entity`, `duplicate_attribute`, …) are listed in the
[reference](reference.md#errors). Each has a one-line message and a
longer hint embedded in `err.message`.
