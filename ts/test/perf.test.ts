/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

import { test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml } from '../dist/xml'

// Build a ready-to-use XML parser the way every caller must (this plugin
// exposes no convenience parse — users instantiate the engine and `use`
// the plugin themselves). Building the instance runs the plugin, which
// parses + installs the XML grammar; that grammar build dominates a
// parse.
function makeXmlParser(): Tabnas {
  return new Tabnas().use(jsonic).use(Xml)
}

// Guards against a performance regression where callers rebuild the
// (expensive) XML parser+grammar on every parse instead of building one
// instance and reusing it. This plugin has no cacheable convenience
// `parse()` — it is instantiated per the `new Tabnas().use(jsonic).use(Xml)`
// pattern — so the guard instead pins the recommended usage: reusing ONE
// instance for N parses must stay far cheaper than building a fresh
// instance per parse.
//
// Building the grammar dominates a parse, so the rebuild-per-call path is
// many times slower than instance reuse. If anyone introduced a
// convenience that built a fresh parser each call (or refactored the
// usage that way), this test would catch it.
//
// The check is machine-INDEPENDENT: it compares instance reuse against
// rebuild-per-call on the SAME machine in the SAME run, so a slow CI box
// cannot make it flaky (both sides scale together). There is deliberately
// NO wall-clock budget.
test('reusing one parser is far faster than rebuilding it per parse', () => {
  const src = '<a x="1"><b>hello</b><c/></a>'
  // Each rebuild-per-call parse rebuilds the whole grammar (tens of ms),
  // so keep n modest: the ratio is already enormous and a larger n would
  // only make the suite slower without strengthening the signal.
  const n = 300

  // Warm both paths so the comparison is steady-state.
  const reused = makeXmlParser()
  for (let i = 0; i < 50; i++) {
    reused.parse(src)
    makeXmlParser().parse(src)
  }

  // Recommended usage: build one instance, reuse it for every parse.
  const t0 = process.hrtime.bigint()
  for (let i = 0; i < n; i++) {
    reused.parse(src)
  }
  const reuse = Number(process.hrtime.bigint() - t0)

  // Regression usage: build a fresh instance (rebuilding the grammar)
  // for every parse.
  const t1 = process.hrtime.bigint()
  for (let i = 0; i < n; i++) {
    makeXmlParser().parse(src)
  }
  const rebuild = Number(process.hrtime.bigint() - t1)

  const speedup = rebuild / reuse
  console.log(
    `  reuse=${(reuse / 1e6).toFixed(1)}ms  ` +
      `rebuild-per-call=${(rebuild / 1e6).toFixed(1)}ms  ` +
      `speedup=${speedup.toFixed(2)}x`,
  )

  // Reusing one instance is ~= a single grammar build amortised over N
  // parses; rebuilding per call pays the grammar build every time and
  // here runs many times slower. Require reuse to be at least 4x faster
  // than rebuild-per-call: this catches a regression to per-call
  // rebuilding without depending on absolute wall-clock speed.
  assert.ok(
    reuse * 4 < rebuild,
    `instance reuse is not meaningfully faster than rebuilding the parser ` +
      `per parse: ${n} reuse parses took ${(reuse / 1e6).toFixed(1)}ms vs ` +
      `${(rebuild / 1e6).toFixed(1)}ms rebuilding per call ` +
      `(ratio ${speedup.toFixed(1)}x, need >=4x). Build one parser ` +
      `(new Tabnas().use(jsonic).use(Xml)) and reuse it; do not rebuild the ` +
      `XML grammar on every parse.`,
  )
})
