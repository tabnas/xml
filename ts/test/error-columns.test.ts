/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml } from '../dist/xml'

// Deliberately its OWN file. `xml.test.ts` loads the W3C conformance
// corpus at module scope and throws when it is absent, so a test placed
// there cannot run without a network fetch it does not need. This one
// asserts four short strings.
// ---------------------------------------------------------------------------
// Error COLUMNS after a non-ASCII character.
//
// This port advances `pnt.cI` by `end - sI` over UTF-16 indices, which
// counts CHARACTERS. The Go port's `advance` added `to - from` over BYTE
// indices, so a 2-byte `é` charged two columns, a 3-byte `€` three and an
// astral character four — every diagnostic after a non-ASCII character
// reported a column past where the problem was.
//
// The two lines look like the same line, which is why it survived: a
// transliteration is not a port when the two languages index strings
// differently. Found by the fleet parity probe.
//
// go/advance_col_test.go asserts the same four inputs. The astral row is
// the only one where the answers differ, and that is the recorded engine
// divergence — this port counts UTF-16 units (an astral character is 2),
// Go counts runes (1). See parser/DIVERGENCE.md, "Column positions for
// astral characters".
// ---------------------------------------------------------------------------

describe('error-columns', () => {
  test('error columns count characters, not bytes', () => {
    const cases: [string, string, number, number][] = [
      // Control: pure ASCII, where bytes and units coincide. Without it,
      // "columns count characters" is also satisfied by never counting.
      ['ascii', '<a>xx</a><', 10, 10],

      // 2 and 3 bytes, 1 rune, 1 UTF-16 unit: both ports agree.
      ['latin1', '<a>\u00e9</a><', 9, 9],
      ['bmp', '<a>\u20ac</a><', 9, 9],

      // 4 bytes, 1 rune, TWO UTF-16 units: the recorded divergence, and
      // the only row where the two halves differ.
      ['astral', '<a>\u{1F600}</a><', 10, 9],
    ]

    for (const [label, src, col, go] of cases) {
      const j = new Tabnas().use(jsonic).use(Xml)
      let err: any = null
      try {
        j.parse(src)
      }
      catch (e) {
        err = e
      }
      assert.ok(null != err,
        `${label}: ${JSON.stringify(src)} parsed, expected a diagnostic`)

      // Read the SERIALISED diagnostic, not the thrown object: `col` is
      // part of the JSON contract and is not an own enumerable property,
      // so `err.col` is `undefined` and an assertion against it would
      // compare nothing.
      const diag = JSON.parse(JSON.stringify(err))
      assert.equal(diag.col, col,
        `${label}: ${JSON.stringify(src)} col — Go says ${go}`)
    }
  })
})
