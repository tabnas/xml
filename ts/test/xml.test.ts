/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml, decodeBOM } from '../dist/xml'



// ---------------------------------------------------------------------------
// XML embedded in Jsonic source
//
// With `embed: true` the plugin extends Jsonic's own grammar so a literal
// XML element can appear anywhere a Jsonic value is expected. The outer
// document is parsed by standard Jsonic; the XML subtree is built by the
// plugin's element grammar.
// ---------------------------------------------------------------------------

describe('xml-embedded-in-jsonic', () => {
  test('plain Jsonic is unaffected by embed mode', () => {
    const j = new Tabnas().use(jsonic).use(Xml, { embed: true })
    assert.deepEqual(j.parse('{a:1, b:"two"}'), { a: 1, b: 'two' })
    assert.deepEqual(j.parse('[1, 2, 3]'), [1, 2, 3])
  })

  test('XML literal as the top-level value', () => {
    const j = new Tabnas().use(jsonic).use(Xml, { embed: true })
    assert.deepEqual(j.parse('<a>hello</a>'), {
      name: 'a',
      localName: 'a',
      attributes: {},
      children: ['hello'],
    })
    assert.deepEqual(j.parse('<br/>'), {
      name: 'br',
      localName: 'br',
      attributes: {},
      children: [],
    })
  })

  test('XML literal as a value inside a Jsonic map', () => {
    const j = new Tabnas().use(jsonic).use(Xml, { embed: true })
    const src =
      '{\n' +
      '  title: "order-42",\n' +
      '  payload: <order id="42">\n' +
      '    <item qty="2">Widget</item>\n' +
      '    <item qty="1">Gadget</item>\n' +
      '  </order>,\n' +
      '}'
    const result = j.parse(src) as any
    assert.equal(result.title, 'order-42')
    const payload = result.payload
    assert.equal(payload.name, 'order')
    assert.equal(payload.attributes.id, '42')
    const items = payload.children.filter(
      (c: any) => typeof c === 'object' && c.name === 'item',
    )
    assert.equal(items.length, 2)
    assert.equal(items[0].attributes.qty, '2')
    assert.equal(items[0].children[0], 'Widget')
    assert.equal(items[1].attributes.qty, '1')
    assert.equal(items[1].children[0], 'Gadget')
  })

  test('XML literal preserves comma and colon in text', () => {
    // Without embed-mode text handling, Jsonic's lexer would split this
    // text on the comma and reject the fragment. The custom matcher
    // claims the run when depth > 0, so it arrives as a single child.
    const j = new Tabnas().use(jsonic).use(Xml, { embed: true })
    assert.deepEqual(j.parse('<a>Hello, World!</a>'), {
      name: 'a',
      localName: 'a',
      attributes: {},
      children: ['Hello, World!'],
    })
    assert.deepEqual(j.parse('<a>key: value</a>'), {
      name: 'a',
      localName: 'a',
      attributes: {},
      children: ['key: value'],
    })
  })

  test('multiple XML literals inside a Jsonic list', () => {
    const j = new Tabnas().use(jsonic).use(Xml, { embed: true })
    const result = j.parse('[<a/>, <b>x</b>, <c x="1"/>]') as any[]
    assert.equal(result.length, 3)
    assert.equal(result[0].name, 'a')
    assert.equal(result[1].name, 'b')
    assert.deepEqual(result[1].children, ['x'])
    assert.equal(result[2].attributes.x, '1')
  })

  test('XML literal with namespaces resolves correctly', () => {
    const j = new Tabnas().use(jsonic).use(Xml, { embed: true })
    const result = j.parse(
      '{doc: <root xmlns="http://e.example"><child/></root>}',
    ) as any
    assert.equal(result.doc.namespace, 'http://e.example')
    assert.equal(result.doc.children[0].namespace, 'http://e.example')
  })
})


describe('decodeBOM', () => {
  // Regression: decodeBOM built the UTF-16 string with
  // `String.fromCharCode(...units)`, which overflows the call stack
  // (RangeError) once the document has more than a few tens of
  // thousands of code units — a crash rather than a parse. The W3C
  // suite's japanese/pr-xml-utf-16.xml is such a document.
  for (const [label, big] of [['utf-16be', true], ['utf-16le', false]] as
    [string, boolean][]) {
    test(`large ${label} document decodes without overflowing`, () => {
      const text = '<doc>' + 'あ'.repeat(200000) + '</doc>'
      const bytes: number[] = big ? [0xfe, 0xff] : [0xff, 0xfe]
      for (let i = 0; i < text.length; i++) {
        const u = text.charCodeAt(i)
        if (big) bytes.push(u >> 8, u & 0xff)
        else bytes.push(u & 0xff, u >> 8)
      }
      const decoded = decodeBOM(Uint8Array.from(bytes))
      assert.equal(decoded, text)
      const el = new Tabnas().use(jsonic).use(Xml).parse(decoded) as any
      assert.equal(el.name, 'doc')
      assert.equal(el.children[0].length, 200000)
    })
  }
})


// ---------------------------------------------------------------------------
// W3C XML Conformance Test Suite (xmltest subset)
//
// The narrow guard: valid/sa documents that parse and not-wf/sa documents
// that are correctly rejected, each required to stay above a regression
// floor. Mirrors the Go test in go/xmlconf_test.go. Measured numbers are
// 120/120 valid and 74/186 not-wf rejected.
//
// The suite is fetched by `scripts/fetch-xml-suite.sh`, which the `pretest`
// npm script runs before every `npm test`. This block used to carry
// `{ skip: !xmlconfAvailable }`, so when the corpus was absent — which is
// what CI always did, since nothing fetched it — it reported green while
// measuring nothing. A missing corpus is now a hard failure.
//
// `xmlconf.test.ts` runs the whole catalogue (2586 documents, not the 306
// here) and additionally compares valid documents against the catalogue's
// canonical OUTPUT. These floors stay because they are a tighter guard on
// the sub-corpus they cover.
// ---------------------------------------------------------------------------

const xmlconfRoot = join(__dirname, '..', '..', 'test', 'xmlconf')

if (!existsSync(join(xmlconfRoot, 'xmltest'))) {
  throw new Error(
    'W3C XML Conformance Test Suite is MISSING.\n' +
      `  expected: ${join(xmlconfRoot, 'xmltest')}\n` +
      '  fix: run scripts/fetch-xml-suite.sh (`npm test` does this for you\n' +
      '       via the `pretest` script; it needs network access to w3.org)\n' +
      '  these tests do NOT skip.',
  )
}

// Regression guards, set to the measured counts. Raise them when
// conformance genuinely improves; never lower one to make a
// regression pass.
const VALID_SA_PASS_FLOOR = 120
const NOT_WF_SA_REJECT_FLOOR = 74

function xmlconfFiles(dir: string): string[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir)
    .filter((n) => n.endsWith('.xml'))
    .filter((n) => statSync(join(dir, n)).isFile())
    .map((n) => join(dir, n))
}

describe('w3c-xml-conformance', () => {
  test('valid/sa documents parse', () => {
    const files = xmlconfFiles(join(xmlconfRoot, 'xmltest', 'valid', 'sa'))
    assert.ok(files.length > 0, 'no valid/sa files')
    const parser = new Tabnas().use(jsonic).use(Xml)
    let pass = 0
    const failures: string[] = []
    for (const path of files) {
      // Read as a Buffer and let decodeBOM choose the encoding via
      // the BOM (default UTF-8). This lets the same runner handle the
      // suite's UTF-8 files (with or without BOM) and the few UTF-16
      // / UTF-32 documents.
      const body = decodeBOM(readFileSync(path))
      try {
        parser.parse(body)
        pass++
      } catch (err) {
        const msg = (err as Error).message.split('\n', 1)[0]
        failures.push(`${path.split('/').slice(-1)[0]}: ${msg}`)
      }
    }
    console.log(`  valid/sa: ${pass} / ${files.length} parsed successfully`)
    assert.ok(
      pass >= VALID_SA_PASS_FLOOR,
      `valid/sa pass count ${pass} dropped below floor ${VALID_SA_PASS_FLOOR}. Sample failures:\n  ${failures.slice(0, 5).join('\n  ')}`,
    )
  })

  test('not-wf/sa documents are rejected', () => {
    const files = xmlconfFiles(join(xmlconfRoot, 'xmltest', 'not-wf', 'sa'))
    assert.ok(files.length > 0, 'no not-wf/sa files')
    const parser = new Tabnas().use(jsonic).use(Xml)
    let rejected = 0
    const falseAccepts: string[] = []
    for (const path of files) {
      const body = decodeBOM(readFileSync(path))
      try {
        parser.parse(body)
        falseAccepts.push(path.split('/').slice(-1)[0])
      } catch {
        rejected++
      }
    }
    console.log(`  not-wf/sa: ${rejected} / ${files.length} rejected as expected`)
    assert.ok(
      rejected >= NOT_WF_SA_REJECT_FLOOR,
      `not-wf/sa reject count ${rejected} dropped below floor ${NOT_WF_SA_REJECT_FLOOR}. Sample false accepts:\n  ${falseAccepts.slice(0, 5).join('\n  ')}`,
    )
  })
})
