/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml } from '../dist/xml'

// ---------------------------------------------------------------------------
// Shared TSV spec runner
//
// Test cases are defined in tab-separated value files under test/spec/*.tsv.
// Each non-comment row is:
//   name<TAB>input<TAB>expected<TAB>opts
// - `input` uses the escape set \n \r \t \\
// - `expected` is raw JSON (standard JSON escapes apply) or the literal
//   token ERROR / ERROR:code for expected parse failures.
// - `opts` is optional JSON for plugin options.
// The same files drive the Go test suite in go/xml_test.go.
// ---------------------------------------------------------------------------

// At runtime this test file is loaded from `dist-test/`, so hop up one
// level to reach the shared spec directory in the project root.
const specDir = join(__dirname, '..', '..', 'test', 'spec')

type SpecRow = {
  file: string
  line: number
  name: string
  input: string
  expected: string
  opts: string
}

function loadSpec(file: string): SpecRow[] {
  const path = join(specDir, file)
  const body = readFileSync(path, 'utf8')
  const rows: SpecRow[] = []
  const lines = body.split('\n')
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i]
    if (raw === '' || raw.startsWith('#')) continue
    const cols = raw.split('\t')
    if (cols.length < 3) {
      throw new Error(`${file}:${i + 1}: expected >=3 tab-separated columns`)
    }
    rows.push({
      file,
      line: i + 1,
      name: cols[0],
      input: unescapeInput(cols[1]),
      expected: cols[2],
      opts: cols[3] ?? '',
    })
  }
  return rows
}

// Decode the escape sequences used in the spec `input` column. Keeps
// the behaviour identical to the Go loader so the two language test
// suites exercise the exact same XML text.
function unescapeInput(s: string): string {
  if (!s.includes('\\')) return s
  let out = ''
  for (let i = 0; i < s.length; i++) {
    const c = s[i]
    if (c === '\\' && i + 1 < s.length) {
      const n = s[i + 1]
      if (n === 'n') { out += '\n'; i++; continue }
      if (n === 'r') { out += '\r'; i++; continue }
      if (n === 't') { out += '\t'; i++; continue }
      if (n === '\\') { out += '\\'; i++; continue }
    }
    out += c
  }
  return out
}

function runSpec(file: string) {
  const rows = loadSpec(file)
  describe(file, () => {
    for (const row of rows) {
      test(row.name, () => {
        const opts = row.opts.trim() === '' ? undefined : JSON.parse(row.opts)
        const jx = opts
          ? new Tabnas().use(jsonic).use(Xml, opts)
          : new Tabnas().use(jsonic).use(Xml)

        if (row.expected.startsWith('ERROR')) {
          const code = row.expected.slice(5).replace(/^:/, '')
          assert.throws(
            () => jx.parse(row.input),
            (err: Error) =>
              code === '' || err.message.includes(code) ||
              // Jsonic wraps codes as `jsonic/<code>`; accept that form too.
              err.message.includes('/' + code),
            `${row.file}:${row.line}: expected error ${row.expected}`,
          )
          return
        }

        const got = jx.parse(row.input)
        const want = JSON.parse(row.expected)
        // Round-trip `got` through JSON so ordering of keys does not affect
        // structural comparison (deepEqual is already order-insensitive for
        // objects, but this also strips undefined fields cleanly).
        assert.deepEqual(
          JSON.parse(JSON.stringify(got)),
          want,
          `${row.file}:${row.line}: ${row.name}`,
        )
      })
    }
  })
}

// Auto-discover every .tsv under test/spec and run it. Keeping this
// driven by directory contents means adding a new spec file never
// requires editing the TypeScript test code.
for (const file of readdirSync(specDir)) {
  if (file.endsWith('.tsv')) runSpec(file)
}


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


// ---------------------------------------------------------------------------
// The W3C XML Conformance Test Suite used to be exercised from here, behind
// `describe(..., { skip: !xmlconfAvailable })` and against pass-rate FLOORS
// (>= 118/120 valid, >= 30/186 not-wf). That block was green while the
// parser rejected only 64 of 186 not-well-formed documents, never ran in CI
// at all (CI does not fetch the corpus), and for valid documents asserted
// only "did not throw" without ever comparing the parsed VALUE.
//
// It has been replaced by test/xmlconf.test.ts, which reads the full
// catalog, asserts every document individually, compares valid documents
// against the suite's canonical output, and FAILS LOUDLY when the corpus is
// absent instead of skipping.
// ---------------------------------------------------------------------------
