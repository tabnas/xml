/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

// Composition test: the XML grammar plugin layered with the official
// @tabnas/debug plugin. @tabnas/debug is a declared devDependency and CI
// builds it as a sibling, so it is always meant to be present; this resolves
// it dynamically only so a TABNAS_DEBUG_PATH override can point at a sibling
// checkout's built plugin. If it cannot be resolved the test FAILS — it used
// to skip, which turned a broken dependency graph into a green tick.

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

if (!Debug) {
  throw new Error(
    '@tabnas/debug could not be resolved, so the xml+debug composition test ' +
      'cannot run.\n' +
      '  it is a devDependency of ts/package.json and must be installed.\n' +
      '  fix: build the sibling checkout (../../debug/ts) and re-link, or set ' +
      'TABNAS_DEBUG_PATH to a built copy.\n' +
      '  this test does NOT skip: a test that quietly does not run is the ' +
      'same defect class as a swallowed failure.',
  )
}

describe('compose: xml + @tabnas/debug', () => {
  test('parses normally with the debug plugin installed', () => {
    const tn = new Tabnas().use(jsonic).use(Xml)
    tn.use(Debug, { print: false, trace: false })
    assert.deepEqual(JSON.parse(JSON.stringify(tn.parse('<a>hello</a>'))), {
      name: 'a',
      localName: 'a',
      attributes: {},
      children: ['hello'],
    })
  })

  test('debug.model() returns the structured xml grammar', () => {
    const tn = new Tabnas().use(jsonic).use(Xml)
    tn.use(Debug, { print: false, trace: false })
    const m = tn.debug.model()

    // The structured rule set and entry rule. The XML grammar defines a
    // six-rule chain: xml -> element -> head (replaced by content) ->
    // content -> children -> child.
    assert.deepStrictEqual(m.rules.map((r: any) => r.name).sort(), [
      'child',
      'children',
      'content',
      'element',
      'head',
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
    //   xml opens by pushing `element`; an element with content pushes
    //   `head`, which reads nothing and closes by replacing itself with
    //   `content`; content opens by pushing `children`, which opens by
    //   pushing `child`, and a child closes by replacing itself with the
    //   next child, so siblings are a replace loop, never a push chain.
    const rule = (name: string) => m.rules.find((r: any) => r.name === name)
    assert.ok(
      rule('xml').open.some((a: any) => a.push === 'element'),
      'xml should push element',
    )
    assert.ok(
      rule('element').open.some((a: any) => a.push === 'head'),
      'element should push head',
    )
    assert.deepStrictEqual(rule('head').open, [], 'head reads nothing')
    assert.ok(
      rule('head').close.some((a: any) => a.replace === 'content'),
      'head should be replaced by content',
    )
    assert.ok(
      rule('content').open.some((a: any) => a.push === 'children'),
      'content should push children',
    )
    assert.ok(
      rule('children').open.some((a: any) => a.push === 'child'),
      'children should push child',
    )
    assert.ok(
      rule('child').close.some((a: any) => a.replace === 'child'),
      'a child should be replaced by the next',
    )
    assert.ok(
      !rule('child').close.some((a: any) => a.push),
      'a child never pushes the next',
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
