/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

// ---------------------------------------------------------------------------
// W3C XML Conformance Test Suite (xmlts) — TypeScript runner
//
// Corpus: https://www.w3.org/XML/Test/xmlts20130923.tar.gz
//         sha256 9b61db9f5dbffa545f4b8d78422167083a8568c59bd1129f94138f936cf6fc1f
// Fetched by scripts/fetch-xml-suite.sh into test/xmlconf/ (gitignored — the
// corpus is W3C-owned and is never committed to this repository). The fetch
// runs automatically from the `pretest` npm script, so `npm test` always has
// it, in CI as well as locally.
//
// WHY THIS FILE EXISTS
//   `xml.test.ts` carries the original conformance guard: two directories of
//   one collection (xmltest/valid/sa, xmltest/not-wf/sa — 306 of the
//   catalogue's 2586 documents), asserted as pass-count floors, and for valid
//   documents asserting only "did not throw". Those floors are still there
//   and still enforced; they are a fine narrow regression guard.
//
//   This file widens the instrument to the whole catalogue and adds the
//   assertion the narrow one cannot make: for every `valid` document where
//   the catalogue supplies an OUTPUT file, the parse result is serialised to
//   James Clark canonical XML and compared against it. "It did not throw" is
//   not a statement about the parsed VALUE.
//
// SCOPE
//   @tabnas/xml claims XML 1.0 plus Namespaces 1.0. Catalogue entries whose
//   RECOMMENDATION is XML1.1 or NS1.1 describe a different language version
//   that this package does not claim, and are not asserted. That exclusion is
//   by declared scope, not to improve a number, and the census test below
//   reports the excluded count so it can never grow unnoticed.
//
//   Catalogue TYPE meanings, and what a non-validating XML processor must do:
//     valid    - well-formed AND valid  -> must be ACCEPTED, and where the
//                catalogue supplies OUTPUT the result must equal it.
//     invalid  - well-formed, not valid -> must still be ACCEPTED (this
//                parser is non-validating; rejecting one is a real defect).
//     not-wf   - not well-formed        -> must be REJECTED.
//     error    - the spec leaves reporting at the processor's discretion, so
//                there is no correct answer to assert. These are NOT turned
//                into tests at all, rather than into a test that asserts
//                nothing. Their count is reported by the census.
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
// A conformance suite that quietly does not run is worse than no suite,
// because the green tick then means nothing. The corpus is fetched by the
// `pretest` npm script; if it is still missing, fail at load time, loudly.
if (!existsSync(CATALOG)) {
  throw new Error(
    'W3C XML Conformance Test Suite is MISSING.\n' +
      `  expected catalogue: ${CATALOG}\n` +
      '  fix: run scripts/fetch-xml-suite.sh (`npm test` does this for you\n' +
      '       via the `pretest` script; it needs network access to w3.org)\n' +
      '  this suite does NOT skip.',
  )
}

// ---------------------------------------------------------------------------
// Catalogue reader
//
// xmlconf.xml is itself XML, and parsing it with the parser under test would
// be circular. The <TEST> elements are flat and attribute-only, so a scanner
// is enough. Sub-catalogues are pulled in through internal-subset SYSTEM
// entities; xml:base on the enclosing <TESTCASES> establishes the directory
// each URI is relative to.
// ---------------------------------------------------------------------------

export type ConfTest = {
  id: string
  type: string // valid | invalid | not-wf | error
  recommendation: string // XML1.0 | XML1.0-errata2e | NS1.0 | XML1.1 | ...
  entities: string // none | general | parameter | both
  sections: string
  uri: string // absolute path to the document
  output: string | null // absolute path to the expected canonical output
  description: string
}

function attrsOf(tag: string): Record<string, string> {
  const out: Record<string, string> = {}
  const re = /([A-Za-z:._-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g
  let m: RegExpExecArray | null
  while ((m = re.exec(tag))) out[m[1]] = m[2] !== undefined ? m[2] : m[3]
  return out
}

// The 2013 catalogue's last <TESTCASES> declares xml:base="eduni/namespaces/misc/"
// but the files ship in eduni/misc/. Upstream inconsistency; resolve both. If
// neither exists the census test below fails — a missing corpus file is an
// error, never a silent skip.
function resolveInCorpus(base: string, rel: string): string {
  const primary = resolve(CONF_ROOT, base, rel)
  if (existsSync(primary)) return primary
  const alt = primary.replace(
    join('eduni', 'namespaces', 'misc'),
    join('eduni', 'misc'),
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

// A tripwire on the corpus itself. If the fetch script ever installs a
// different snapshot, or the reader regresses, the suite silently shrinking
// is the failure mode that matters most here — every count below would drop
// with it and read as "no worse than before".
const CATALOG_TOTAL = 2586

// Recommendations this package claims. XML1.1 / NS1.1 are a different
// language version and out of scope (see SCOPE above).
const CLAIMED = (rec: string) =>
  rec.startsWith('XML1.0') || rec.startsWith('NS1.0')

const IN_SCOPE = ALL.filter((t) => CLAIMED(t.recommendation))

// ---------------------------------------------------------------------------
// Canonical XML (James Clark's "first canonical form"), which is what the
// suite's OUTPUT files contain. Serialising the parse result into it is how
// the VALUE is checked.
//
//   - document element only; no XML declaration, no DTD, no comments
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
// The sweep
//
// One pass over the catalogue, classifying every in-scope document. The
// assertions below are counts, set to what this parser actually achieves
// (measured 2026-08-09 on main, and re-derivable at any time by reading the
// summary this file prints). They are regression guards in the same spirit as
// the narrow floors in xml.test.ts: a floor pinned to the measured value
// fails the moment conformance drops by a single document. Raise a floor when
// conformance genuinely improves; never lower one to make a regression pass.
// ---------------------------------------------------------------------------

// The documented setup: `new Tabnas().use(jsonic).use(Xml)` (README).
function parseDoc(t: ConfTest): { value?: any; error?: Error } {
  try {
    // The corpus mixes UTF-8 / UTF-16 / UTF-32; decodeBOM transcodes.
    const src = decodeBOM(readFileSync(t.uri))
    return { value: new Tabnas().use(jsonic).use(Xml).parse(src) }
  } catch (err) {
    return { error: err as Error }
  }
}

type Sweep = {
  validTotal: number
  validAccepted: number
  validRejected: string[]
  validChecked: number
  validCorrect: number
  validMismatched: string[]
  notwfTotal: number
  notwfRejected: number
  notwfAccepted: string[]
  invalidTotal: number
  invalidAccepted: number
  invalidRejected: string[]
}

function sweep(): Sweep {
  const s: Sweep = {
    validTotal: 0,
    validAccepted: 0,
    validRejected: [],
    validChecked: 0,
    validCorrect: 0,
    validMismatched: [],
    notwfTotal: 0,
    notwfRejected: 0,
    notwfAccepted: [],
    invalidTotal: 0,
    invalidAccepted: 0,
    invalidRejected: [],
  }
  for (const t of IN_SCOPE) {
    if ('error' === t.type) continue
    const r = parseDoc(t)
    if ('valid' === t.type) {
      s.validTotal++
      if (r.error) {
        s.validRejected.push(`${t.id} (${t.sections}): ${t.uri}`)
        continue
      }
      s.validAccepted++
      if (null !== t.output) {
        s.validChecked++
        if (canonical(r.value) === decodeBOM(readFileSync(t.output))) {
          s.validCorrect++
        } else {
          s.validMismatched.push(`${t.id} (${t.sections}): ${t.uri}`)
        }
      }
    } else if ('not-wf' === t.type) {
      s.notwfTotal++
      if (r.error) s.notwfRejected++
      else s.notwfAccepted.push(`${t.id} (${t.sections}): ${t.uri}`)
    } else if ('invalid' === t.type) {
      s.invalidTotal++
      if (r.error) s.invalidRejected.push(`${t.id} (${t.sections}): ${t.uri}`)
      else s.invalidAccepted++
    }
  }
  return s
}

const S = sweep()

// Regression guards, set to the counts measured on this parser. Raise them
// when conformance genuinely improves; never lower one to make a regression
// pass. `rmt-e2e-50` (eduni/errata-2e/E50.xml) is the single `valid` document
// currently rejected, which is why the valid floor is 728 and not 729.
const VALID_ACCEPT_FLOOR = 728
const VALID_CANONICAL_FLOOR = 232
const NOT_WF_REJECT_FLOOR = 438

function sample(list: string[], n = 5): string {
  return list.slice(0, n).join('\n  ') + (n < list.length ? `\n  ... and ${list.length - n} more` : '')
}

describe('w3c-xml-conformance/census', () => {
  test('the whole catalogue was read', () => {
    assert.equal(
      ALL.length,
      CATALOG_TOTAL,
      `catalogue census changed: read ${ALL.length} <TEST> entries, expected ${CATALOG_TOTAL}. ` +
        'Either the corpus snapshot changed (check scripts/fetch-xml-suite.sh) ' +
        'or the catalogue reader regressed. A shrinking corpus must never pass quietly.',
    )
  })

  test('every catalogued document exists on disk', () => {
    const missing = ALL.filter((t) => !existsSync(t.uri)).map(
      (t) => `${t.id} -> ${t.uri}`,
    )
    assert.deepEqual(
      missing,
      [],
      `catalogued documents missing from the corpus:\n  ${missing.join('\n  ')}`,
    )
  })

  test('scope split is what this package claims', () => {
    const out = ALL.filter((t) => !CLAIMED(t.recommendation))
    const byType = (list: ConfTest[]) => {
      const c: Record<string, number> = {}
      for (const t of list) c[t.type] = (c[t.type] || 0) + 1
      return c
    }
    // Reported so the excluded set can never grow unnoticed.
    console.log(
      `  catalogue ${ALL.length}: in-scope (XML1.0/NS1.0) ${IN_SCOPE.length}, ` +
        `out-of-scope (XML1.1/NS1.1) ${out.length}`,
    )
    console.log(`  in-scope by TYPE: ${JSON.stringify(byType(IN_SCOPE))}`)
    console.log(
      `  'error' TYPE tests are reported, not asserted: ${byType(IN_SCOPE)['error'] || 0}`,
    )
    assert.equal(IN_SCOPE.length + out.length, ALL.length)
  })
})

describe('w3c-xml-conformance/valid', () => {
  test(`at least ${VALID_ACCEPT_FLOOR} valid documents are accepted`, () => {
    assert.ok(
      VALID_ACCEPT_FLOOR <= S.validAccepted,
      `valid accepted ${S.validAccepted} / ${S.validTotal} dropped below the ` +
        `measured floor ${VALID_ACCEPT_FLOOR}. Rejected valid documents:\n  ${sample(S.validRejected)}`,
    )
  })

  test(`at least ${VALID_CANONICAL_FLOOR} valid documents serialise to the catalogue's canonical output`, () => {
    // This is the assertion the narrow suite in xml.test.ts cannot make: the
    // parsed VALUE, not merely "it did not throw".
    assert.ok(
      VALID_CANONICAL_FLOOR <= S.validCorrect,
      `canonical-output matches ${S.validCorrect} / ${S.validChecked} dropped below the ` +
        `measured floor ${VALID_CANONICAL_FLOOR}. Mismatches:\n  ${sample(S.validMismatched)}`,
    )
  })
})

describe('w3c-xml-conformance/invalid-but-well-formed', () => {
  test('every well-formed but DTD-invalid document is accepted', () => {
    // This parser is non-validating, so rejecting one of these is a real
    // defect. All of them currently pass, so this is an exact assertion, not
    // a floor.
    assert.deepEqual(
      S.invalidRejected,
      [],
      `a non-validating parser must accept well-formed documents that are ` +
        `merely DTD-invalid, but ${S.invalidRejected.length} were rejected:\n  ${sample(S.invalidRejected)}`,
    )
  })
})

describe('w3c-xml-conformance/not-well-formed', () => {
  test(`at least ${NOT_WF_REJECT_FLOOR} not-well-formed documents are rejected`, () => {
    assert.ok(
      NOT_WF_REJECT_FLOOR <= S.notwfRejected,
      `not-wf rejected ${S.notwfRejected} / ${S.notwfTotal} dropped below the ` +
        `measured floor ${NOT_WF_REJECT_FLOOR}. Sample false accepts:\n  ${sample(S.notwfAccepted)}`,
    )
  })
})

// -- The dial ---------------------------------------------------------------
// One place that prints the true numbers, so the current state of conformance
// can be read off a CI log without counting anything by hand. It asserts only
// internal consistency; the tests above carry the guards.

describe('w3c-xml-conformance/summary', () => {
  test('conformance numbers (TypeScript)', () => {
    const validPass = S.validAccepted - (S.validChecked - S.validCorrect)
    const pct = (a: number, b: number) =>
      0 === b ? '-' : ((100 * a) / b).toFixed(1) + '%'

    console.log('')
    console.log(
      '  === W3C XML conformance, TypeScript (xmlts 20130923, XML1.0/NS1.0 scope) ===',
    )
    console.log(
      `  valid   accepted+correct : ${validPass} / ${S.validTotal}  (${pct(validPass, S.validTotal)})`,
    )
    console.log(`            of which parsed: ${S.validAccepted} / ${S.validTotal}`)
    console.log(
      `            value-compared : ${S.validCorrect} / ${S.validChecked} documents with catalogue OUTPUT`,
    )
    console.log(
      `  not-wf  rejected         : ${S.notwfRejected} / ${S.notwfTotal}  (${pct(S.notwfRejected, S.notwfTotal)})`,
    )
    console.log(
      `  invalid accepted (non-validating): ${S.invalidAccepted} / ${S.invalidTotal}  (${pct(S.invalidAccepted, S.invalidTotal)})`,
    )
    console.log('')

    assert.equal(
      S.validTotal + S.notwfTotal + S.invalidTotal,
      IN_SCOPE.filter((t) => 'error' !== t.type).length,
    )
  })
})
