/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml, decodeBOM } from '../dist/xml'

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
// W3C XML Conformance Test Suite (xmltest subset)
//
// Exercised when the suite has been fetched to `test/xmlconf/` via
// `scripts/fetch-xml-suite.sh`. Skipped otherwise. Mirrors the Go test
// in go/xmlconf_test.go: counts valid/sa documents that parse and
// not-wf/sa documents that are correctly rejected, requiring each
// count to stay above a regression floor. Current parser numbers are
// ~116/120 valid and ~39/186 not-wf rejected.
// ---------------------------------------------------------------------------

const xmlconfRoot = join(__dirname, '..', '..', 'test', 'xmlconf')
const xmlconfAvailable = existsSync(join(xmlconfRoot, 'xmltest'))

// Regression guards; raise once parser coverage improves.
const VALID_SA_PASS_FLOOR = 118
const NOT_WF_SA_REJECT_FLOOR = 30

function xmlconfFiles(dir: string): string[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir)
    .filter((n) => n.endsWith('.xml'))
    .filter((n) => statSync(join(dir, n)).isFile())
    .map((n) => join(dir, n))
}

describe('w3c-xml-conformance', { skip: !xmlconfAvailable }, () => {
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
