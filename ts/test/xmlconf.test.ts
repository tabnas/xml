/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

// ---------------------------------------------------------------------------
// W3C XML Conformance Test Suite (xmlts) — TypeScript runner
//
// Corpus: https://www.w3.org/XML/Test/xmlts20130923.tar.gz
//         sha256 9b61db9f5dbffa545f4b8d78422167083a8568c59bd1129f94138f936cf6fc1f
// Fetched by scripts/fetch-xml-suite.sh into test/xmlconf/ (gitignored —
// the corpus is W3C-owned and is never committed to this repository).
//
// This file is the honest dial. It is deliberately allowed to be RED.
//
// HOW IT DIFFERS FROM WHAT IT REPLACED
//   The previous conformance block lived in xml.test.ts and was green while
//   the parser rejected only 64 of 186 not-well-formed documents, because:
//     * it ran `describe(..., { skip: !xmlconfAvailable })`, and CI never
//       fetched the corpus, so in CI it silently did not run at all;
//     * it asserted pass-rate FLOORS (>= 118 of 120 valid, >= 30 of 186
//       not-wf). A floor of 30/186 is a test that cannot fail.
//     * for valid documents it asserted only "did not throw" and never
//       compared the parsed VALUE against the suite's expected output.
//   All three are fixed here: the corpus is fetched by the `pretest` npm
//   script, a missing corpus is a hard failure, every catalogued test is an
//   individual assertion, and valid documents are compared against the
//   suite's canonical-XML output where the catalog supplies one.
//
// SCOPE
//   @tabnas/xml claims XML 1.0 (plus Namespaces 1.0 — the README advertises
//   namespace support). Catalog entries whose RECOMMENDATION is XML1.1 or
//   NS1.1 describe a different language version that this package does not
//   claim, and are not run. That exclusion is by declared scope, not to
//   improve a number; the excluded set is 274 of the catalog's 2586 tests
//   and is reported by the census test below so it can never go unnoticed.
//
//   Catalog TYPE meanings, and what a non-validating XML processor must do:
//     valid    - well-formed AND valid  -> must be ACCEPTED, and where the
//                catalog supplies OUTPUT the parse result must equal it.
//     invalid  - well-formed, not valid -> must still be ACCEPTED (this
//                parser is non-validating; rejecting one is a real defect).
//     not-wf   - not well-formed        -> must be REJECTED.
//     error    - the spec leaves reporting at the processor's discretion,
//                so there is no correct answer to assert. These are NOT
//                turned into tests at all, rather than into a test that
//                asserts nothing. Their count is reported by the census.
// ---------------------------------------------------------------------------

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml, decodeBOM } from '../dist/xml'

// At runtime this file is loaded from `dist-test/`, so hop up two levels.
const CONF_ROOT = join(__dirname, '..', '..', 'test', 'xmlconf')
const CATALOG = join(CONF_ROOT, 'xmlconf.xml')

// -- Never skip -------------------------------------------------------------
// A conformance suite that quietly does not run is worse than no suite at
// all, because the green tick is a lie. Fail at load time, loudly.
if (!existsSync(CATALOG)) {
  throw new Error(
    'W3C XML Conformance Test Suite is MISSING.\n' +
      `  expected catalog: ${CATALOG}\n` +
      '  fix: run scripts/fetch-xml-suite.sh (npm test does this for you via `pretest`)\n' +
      '  this test does NOT skip: a conformance suite that silently does not\n' +
      '  run produces a green tick that means nothing.',
  )
}

// ---------------------------------------------------------------------------
// Catalog reader
//
// xmlconf.xml is itself XML, and parsing it with the parser under test would
// be circular (and it does not currently survive the file). The <TEST>
// elements are flat and attribute-only, so a scanner is enough. Sub-catalogs
// are pulled in through internal-subset SYSTEM entities; xml:base on the
// enclosing <TESTCASES> establishes the directory each URI is relative to.
// ---------------------------------------------------------------------------

export type ConfTest = {
  id: string
  type: string // valid | invalid | not-wf | error
  recommendation: string // XML1.0 | XML1.0-errata2e | NS1.0 | XML1.1 | ...
  entities: string // none | general | parameter | both
  sections: string
  uri: string // absolute path to the document
  output: string | null // absolute path to expected canonical output
  catalog: string // which sub-catalog declared it
  description: string
}

function attrsOf(tag: string): Record<string, string> {
  const out: Record<string, string> = {}
  const re = /([A-Za-z:._-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g
  let m: RegExpExecArray | null
  while ((m = re.exec(tag))) out[m[1]] = m[2] !== undefined ? m[2] : m[3]
  return out
}

// The 2013 catalog's last <TESTCASES> declares xml:base="eduni/namespaces/misc/"
// but the files ship in eduni/misc/. Upstream bug; resolve both, and if
// neither exists the test fails (a missing corpus file is an error, never a
// silent skip).
function resolveInCorpus(base: string, rel: string): string {
  const primary = resolve(CONF_ROOT, base, rel)
  if (existsSync(primary)) return primary
  const alt = primary.replace(
    `${join('eduni', 'namespaces', 'misc')}`,
    `${join('eduni', 'misc')}`,
  )
  return existsSync(alt) ? alt : primary
}

function readCatalog(file: string, base: string, into: ConfTest[]): void {
  const body = readFileSync(file, 'utf8')
  const dir = dirname(file)

  const ents: Record<string, string> = {}
  const ere = /<!ENTITY\s+([A-Za-z0-9._-]+)\s+SYSTEM\s+"([^"]+)"\s*>/g
  let em: RegExpExecArray | null
  while ((em = ere.exec(body))) ents[em[1]] = em[2]

  const re =
    /<TESTCASES\b([^>]*)>|<\/TESTCASES\s*>|<TEST\b([^>]*?)\/?>([\s\S]*?)<\/TEST\s*>|&([A-Za-z0-9._-]+);/g
  const stack: string[] = [base]
  let m: RegExpExecArray | null
  while ((m = re.exec(body))) {
    const tok = m[0]
    const top = stack[stack.length - 1]
    if (tok.startsWith('<TESTCASES')) {
      const a = attrsOf(m[1] || '')
      stack.push(a['xml:base'] ? join(top, a['xml:base']) : top)
    } else if (tok.startsWith('</TESTCASES')) {
      if (1 < stack.length) stack.pop()
    } else if (tok.startsWith('<TEST')) {
      const a = attrsOf(m[2] || '')
      into.push({
        id: a.ID,
        type: a.TYPE,
        recommendation: a.RECOMMENDATION || 'XML1.0',
        entities: a.ENTITIES || 'none',
        sections: a.SECTIONS || '',
        uri: resolveInCorpus(top, a.URI),
        output: a.OUTPUT ? resolveInCorpus(top, a.OUTPUT) : null,
        catalog: file,
        description: (m[3] || '').replace(/\s+/g, ' ').trim().slice(0, 160),
      })
    } else if (tok.startsWith('&')) {
      const sys = ents[m[4]]
      if (sys) readCatalog(resolve(dir, sys), top, into)
    }
  }
}

const ALL: ConfTest[] = []
readCatalog(CATALOG, '.', ALL)

// A tripwire on the corpus itself: if the fetch script ever installs a
// different snapshot, or the reader regresses, the whole suite silently
// shrinking is exactly the failure mode this project keeps getting bitten
// by. Pin the census.
const CATALOG_TOTAL = 2586

// Recommendations this package claims. XML1.1 / NS1.1 are a different
// language version and out of scope (see SCOPE above).
const CLAIMED = (rec: string) =>
  rec.startsWith('XML1.0') || rec.startsWith('NS1.0')

const IN_SCOPE = ALL.filter((t) => CLAIMED(t.recommendation))

// ---------------------------------------------------------------------------
// Canonical XML (James Clark's "first canonical form"), which is what the
// suite's OUTPUT files contain. Serialising the parse result into it is how
// the VALUE is checked — "it didn't throw" is not an assertion.
//
//   - document element only; no XML declaration, no DTD, no comments
//   - processing instructions ARE retained
//   - empty elements written as a start tag plus an end tag
//   - attributes sorted by name, always double-quoted, single space separated
//   - & < > " and #x9 #xA #xD replaced by character references, in both
//     character data and attribute values
// ---------------------------------------------------------------------------

function canonEscape(s: string): string {
  let out = ''
  for (const ch of s) {
    if ('&' === ch) out += '&amp;'
    else if ('<' === ch) out += '&lt;'
    else if ('>' === ch) out += '&gt;'
    else if ('"' === ch) out += '&quot;'
    else if ('\t' === ch) out += '&#9;'
    else if ('\n' === ch) out += '&#10;'
    else if ('\r' === ch) out += '&#13;'
    else out += ch
  }
  return out
}

function canonical(node: any): string {
  if ('string' === typeof node) return canonEscape(node)
  if (null === node || 'object' !== typeof node) return canonEscape(String(node))
  let s = '<' + node.name
  const attrs = node.attributes || {}
  for (const n of Object.keys(attrs).sort()) {
    s += ' ' + n + '="' + canonEscape(String(attrs[n])) + '"'
  }
  s += '>'
  for (const c of node.children || []) s += canonical(c)
  return s + '</' + node.name + '>'
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

// The documented setup: `new Tabnas().use(jsonic).use(Xml)` (README).
function makeParser() {
  return new Tabnas().use(jsonic).use(Xml)
}

function readDoc(t: ConfTest): string {
  // The corpus mixes UTF-8 / UTF-16 / UTF-32; decodeBOM transcodes.
  return decodeBOM(readFileSync(t.uri))
}

function parseDoc(t: ConfTest): { value?: any; error?: Error } {
  try {
    return { value: makeParser().parse(readDoc(t)) }
  } catch (err) {
    return { error: err as Error }
  }
}

function firstLine(err: Error): string {
  return String(err.message).replace(/\[[0-9;]*m/g, '').split('\n')[0]
}

function label(t: ConfTest): string {
  return `${t.id} [${t.recommendation}/${t.entities}] ${t.sections}`
}

describe('w3c-xml-conformance/census', () => {
  test('the whole catalog was read', () => {
    assert.equal(
      ALL.length,
      CATALOG_TOTAL,
      `catalog census changed: read ${ALL.length} <TEST> entries, expected ${CATALOG_TOTAL}. ` +
        'Either the corpus snapshot changed (check scripts/fetch-xml-suite.sh) ' +
        'or the catalog reader regressed. A shrinking corpus must never pass quietly.',
    )
  })

  test('every catalogued document exists on disk', () => {
    const missing = ALL.filter((t) => !existsSync(t.uri)).map((t) => `${t.id} -> ${t.uri}`)
    assert.deepEqual(missing, [], `catalogued documents missing from the corpus:\n  ${missing.join('\n  ')}`)
  })

  test('scope split is what this package claims', () => {
    const out = ALL.filter((t) => !CLAIMED(t.recommendation))
    // Reported so the excluded set can never grow unnoticed.
    console.log(
      `  catalog ${ALL.length}: in-scope (XML1.0/NS1.0) ${IN_SCOPE.length}, ` +
        `out-of-scope (XML1.1/NS1.1) ${out.length}`,
    )
    const byType = (list: ConfTest[]) => {
      const c: Record<string, number> = {}
      for (const t of list) c[t.type] = (c[t.type] || 0) + 1
      return c
    }
    console.log(`  in-scope by TYPE: ${JSON.stringify(byType(IN_SCOPE))}`)
    console.log(`  'error' TYPE tests are reported, not asserted: ${byType(IN_SCOPE)['error'] || 0}`)
    assert.equal(IN_SCOPE.length + out.length, ALL.length)
  })
})

// -- valid: must be ACCEPTED, and must produce the right VALUE --------------

describe('w3c-xml-conformance/valid', () => {
  for (const t of IN_SCOPE.filter((x) => 'valid' === x.type)) {
    test(label(t), () => {
      const r = parseDoc(t)
      assert.ok(
        !r.error,
        `valid document was rejected: ${t.uri}\n  ${t.description}\n  ${r.error ? firstLine(r.error) : ''}`,
      )
      if (null === t.output) return
      // The catalog supplies the expected canonical form: compare the VALUE.
      const want = decodeBOM(readFileSync(t.output))
      const got = canonical(r.value)
      assert.equal(
        got,
        want,
        `canonical output mismatch for ${t.id}\n  doc: ${t.uri}\n  out: ${t.output}\n  ${t.description}`,
      )
    })
  }
})

// -- invalid: well-formed but not valid; a non-validating parser ACCEPTS ----

describe('w3c-xml-conformance/invalid-but-well-formed', () => {
  for (const t of IN_SCOPE.filter((x) => 'invalid' === x.type)) {
    test(label(t), () => {
      const r = parseDoc(t)
      assert.ok(
        !r.error,
        `well-formed (though DTD-invalid) document was rejected by a ` +
          `non-validating parser: ${t.uri}\n  ${t.description}\n  ${r.error ? firstLine(r.error) : ''}`,
      )
    })
  }
})

// -- not-wf: must be REJECTED ----------------------------------------------

describe('w3c-xml-conformance/not-well-formed', () => {
  for (const t of IN_SCOPE.filter((x) => 'not-wf' === x.type)) {
    test(label(t), () => {
      const r = parseDoc(t)
      assert.ok(
        r.error,
        `not-well-formed document was ACCEPTED: ${t.uri}\n  ${t.description}\n` +
          `  parsed as: ${JSON.stringify(r.value)?.slice(0, 200)}`,
      )
    })
  }
})

// -- The dial ---------------------------------------------------------------
// One place that prints the true numbers, so a human reading CI output does
// not have to count 2000 subtest lines. It asserts nothing extra; the
// per-document tests above are the assertions.

describe('w3c-xml-conformance/summary', () => {
  test('true conformance numbers (TypeScript)', () => {
    let validTotal = 0, validAccepted = 0, validChecked = 0, validCorrect = 0
    let notwfTotal = 0, notwfRejected = 0
    let invalidTotal = 0, invalidAccepted = 0

    for (const t of IN_SCOPE) {
      if ('error' === t.type) continue
      const r = parseDoc(t)
      if ('valid' === t.type) {
        validTotal++
        if (!r.error) {
          validAccepted++
          if (null !== t.output) {
            validChecked++
            if (canonical(r.value) === decodeBOM(readFileSync(t.output))) validCorrect++
          }
        }
      } else if ('not-wf' === t.type) {
        notwfTotal++
        if (r.error) notwfRejected++
      } else if ('invalid' === t.type) {
        invalidTotal++
        if (!r.error) invalidAccepted++
      }
    }

    // A "valid" test passes only if it parsed AND (when the catalog gives an
    // expected canonical output) matched it.
    const validPass = validAccepted - (validChecked - validCorrect)
    const pct = (a: number, b: number) => (0 === b ? '-' : ((100 * a) / b).toFixed(1) + '%')

    console.log('')
    console.log('  === W3C XML conformance, TypeScript (xmlts 20130923, XML1.0/NS1.0 scope) ===')
    console.log(`  valid   accepted+correct : ${validPass} / ${validTotal}  (${pct(validPass, validTotal)})`)
    console.log(`            of which parsed: ${validAccepted} / ${validTotal}`)
    console.log(`            value-compared : ${validCorrect} / ${validChecked} documents with catalog OUTPUT`)
    console.log(`  not-wf  rejected         : ${notwfRejected} / ${notwfTotal}  (${pct(notwfRejected, notwfTotal)})`)
    console.log(`  invalid accepted (non-validating): ${invalidAccepted} / ${invalidTotal}  (${pct(invalidAccepted, invalidTotal)})`)
    console.log('')

    assert.equal(validTotal + notwfTotal + invalidTotal, IN_SCOPE.filter((t) => 'error' !== t.type).length)
  })
})
