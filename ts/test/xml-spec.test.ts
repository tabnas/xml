/* Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License */

// Cross-runtime conformance, driven by the shared `test/spec/*.tsv` fixtures
// at the repo root (see ../../test/AGENTS.md).
//
// The fixture loader, the `ERROR:` contract and the row loop come from
// @tabnas/support, whose Go half `go/xml_test.go` uses to run the SAME
// files — so the two implementations cannot drift without one of them
// going red, and neither can the two loaders.
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

const HEX = /^[0-9a-fA-F]{4}$/

// The one thing this repo does not take from @tabnas/support: its own
// escape codec, because xml's fixtures need a sixth escape.
//
// `\uXXXX` names a character that must not be written literally into a
// fixture — a leading U+FEFF byte-order mark above all, which is invisible
// in a diff and which git would carry into the wrong place. The shared
// codec passes `\u` through on purpose: an XML fixture, like a JSON one,
// has to be able to carry a literal `A` as source text.
//
// So it is decoded here, in one pass over the RAW cell. Two passes cannot
// work: the shared codec turns an escaped backslash followed by `uFEFF`
// into exactly the six characters a plain `\\uFEFF` already is, and a
// second pass could no longer tell which of the two the fixture wrote.
//
// Kept byte-identical to specUnescape in go/xml_test.go.
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
      if (n === 'u' && HEX.test(s.substring(i + 2, i + 6))) {
        out += String.fromCharCode(parseInt(s.substring(i + 2, i + 6), 16))
        i += 5
        continue
      }
    }
    out += c
  }
  return out
}

// The engine writes SGR colour sequences into rendered error messages, so
// the `msg` spec column can stay plain text.
function stripANSI(s: string): string {
  // eslint-disable-next-line no-control-regex
  return s.replace(/\x1b\[[0-9;]*m/g, '')
}

makeRunner({
  // The runner's own decoding of the input column is bypassed — see
  // unescapeInput above — so the raw cell is read and decoded here.
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
  // .tsv runs it in both runtimes without touching either runner.
  .dir(findSpecDir(__dirname))
