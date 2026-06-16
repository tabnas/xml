/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

// Composition test: the XML grammar plugin layered with the official
// @tabnas/debug plugin. @tabnas/debug is a devDependency, but this resolves
// it dynamically and SKIPS when it is absent so the suite stays runnable
// outside the package. The `compose-debug` CI job can point
// TABNAS_DEBUG_PATH at a sibling checkout's built plugin.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { createRequire } from 'node:module'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml } from '../dist/xml'

// Resolve from this test file's location so a `file:`-linked sibling
// checkout (or a TABNAS_DEBUG_PATH override) is found at runtime.
const req = createRequire(__filename)

function loadDebug(): any {
  const candidates = [process.env.TABNAS_DEBUG_PATH, '@tabnas/debug'].filter(
    Boolean,
  ) as string[]
  for (const c of candidates) {
    try {
      return req(c).Debug
    } catch {
      /* try next */
    }
  }
  return null
}

const Debug = loadDebug()
const skip = Debug
  ? false
  : '@tabnas/debug not available (set TABNAS_DEBUG_PATH)'

describe('compose: xml + @tabnas/debug', () => {
  test('parses normally with the debug plugin installed', { skip }, () => {
    const tn = new Tabnas().use(jsonic).use(Xml)
    tn.use(Debug, { print: false, trace: false })
    assert.deepEqual(JSON.parse(JSON.stringify(tn.parse('<a>hello</a>'))), {
      name: 'a',
      localName: 'a',
      attributes: {},
      children: ['hello'],
    })
  })

  test('debug.model() returns the structured xml grammar', { skip }, () => {
    const tn = new Tabnas().use(jsonic).use(Xml)
    tn.use(Debug, { print: false, trace: false })
    const m = tn.debug.model()

    // The structured rule set and entry rule. The XML grammar defines a
    // four-rule chain: xml -> element -> content -> child.
    assert.deepStrictEqual(m.rules.map((r: any) => r.name).sort(), [
      'child',
      'content',
      'element',
      'xml',
    ])

    // Entry (start) rule is the document rule `xml`.
    assert.equal(m.config.start, 'xml')

    // The plugin pipeline is recorded; the Xml plugin must be present.
    assert.ok(
      m.plugins.some((p: any) => p.name === 'Xml'),
      'plugins should list Xml',
    )

    // Structural facts specific to this grammar's push chain:
    //   xml opens by pushing `element`; content opens by pushing `child`.
    const xml = m.rules.find((r: any) => r.name === 'xml')
    assert.ok(
      xml.open.some((a: any) => a.push === 'element'),
      'xml should push element',
    )
    const content = m.rules.find((r: any) => r.name === 'content')
    assert.ok(
      content.open.some((a: any) => a.push === 'child'),
      'content should push child',
    )

    // The grammar portion is JSON-serialisable and round-trips.
    const grammar = {
      tokens: m.tokens,
      rules: m.rules,
      graph: m.graph,
      config: m.config,
      abnf: m.abnf,
    }
    assert.deepStrictEqual(JSON.parse(JSON.stringify(grammar)).rules, m.rules)
  })
})
