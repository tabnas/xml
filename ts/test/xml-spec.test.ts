/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

// Cross-runtime conformance, driven by the shared `test/spec/*.tsv` fixtures
// at the repo root (see ../../test/AGENTS.md).
//
// The fixture loader, the `ERROR:` contract and the row loop come from
// @tabnas/support, whose Go and Rust halves `go/xml_test.go` and
// `rs/tests/parity_test.rs` use to run the SAME files — so the
// implementations cannot drift without one of them going red, and neither
// can the loaders.
//
// What is left here is only what is specific to xml: one extra escape, the
// `msg` column, and how to build the parser for a row's options.
//
// A row is `# name<TAB>input<TAB>expected<TAB>opts<TAB>msg`. The header
// line begins with `#` — which is why the columns are read by NAME, and
// why the first one is called `# name`.

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { findSpecDir, makeRunner } from '@tabnas/support'

import { Xml } from '../dist/xml'
import { unescapeInput } from './spec-input'

// The engine writes SGR colour sequences into rendered error messages, so
// the `msg` spec column can stay plain text.
function stripANSI(s: string): string {
  // eslint-disable-next-line no-control-regex
  return s.replace(/\x1b\[[0-9;]*m/g, '')
}

makeRunner({
  // The runner's own decoding of the input column is bypassed — see
  // unescapeInput in spec-input.ts — so the raw cell is read and decoded
  // here.
  parse: (_input, row) => {
    const input = unescapeInput(row.named('input'))
    const opts = row.named('opts')

    return ('' === opts.trim()
      ? new Tabnas().use(jsonic).use(Xml)
      : new Tabnas().use(jsonic).use(Xml, JSON.parse(opts))
    ).parse(input)
  },

  // Two things the default code comparison does not do. The engine renders
  // a code as `jsonic/<code>`, so both spellings are accepted; and the
  // optional `msg` column pins the rendered message, so a template that
  // stops interpolating — leaving a literal placeholder behind — fails
  // here rather than silently shipping.
  matchError: (err: any, want, row) => {
    const message = String(err?.message)
    const named = err?.code === want ||
      message.includes(want) || message.includes('/' + want)
    const msg = row.named('msg')
    return named && ('' === msg || stripANSI(message).includes(msg))
  },

  input: 'input',
  expected: 'expected',
  caseName: (row) => `row ${row.line}: ${row.named('# name')}`,
})
  // `findSpecDir` walks up from this file — `dist-test/` at runtime — to the
  // repo root's `test/spec`, so moving the suite does not mean recounting
  // `..` hops. `dir` then auto-discovers every fixture in it, so adding a
  // .tsv runs it in every runtime without touching any runner.
  .dir(findSpecDir(__dirname))
