/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole. `make()` declares `lex.window`: the plugin's matcher
// reads the text only through the lexer's window, and reads the window on
// where a construct runs past its end, so a reader is read a window at a
// time.
//
// Every row of the shared fixtures and every document of the W3C XML
// Conformance Test Suite is parsed whole and from a reader that hands its
// bytes over in pieces of 1, 3 and 64 bytes, under the engine's window and
// under windows whose margin and block are a few units, so that every
// construct meets the window's edge; a long document, whose long tokens
// of every kind run past the engine's window, in pieces of 4 KiB too. The
// value, the tokens the lexer gives, the rule passes, and an error's code,
// position, row, column, token and message must be the same. An error's
// excerpt is not compared: a parse from a reader shows a long line only as
// far back as the window reaches (tabnas/parser's DIVERGENCE.md).
//
// go/window_test.go and rs/tests/window_test.rs hold the Go and Rust ports
// to the same.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { findSpecDir, loadSpecDir } from '@tabnas/support'

import { Xml, decodeBOM, make } from '../dist/xml'
import { unescapeInput } from './spec-input'

// At runtime this file is loaded from `dist-test/`, so hop up two levels.
const CONF_ROOT = join(__dirname, '..', '..', 'test', 'xmlconf')

// A reader that hands its bytes over at most `size` at a time.
function pieces(bytes: Uint8Array, size: number) {
  let at = 0
  return {
    read(buf: Uint8Array): number {
      const n = Math.min(size, buf.length, bytes.length - at)
      buf.set(bytes.subarray(at, at + n), 0)
      at += n
      return n
    },
  }
}

// A read of a source: the pieces a reader hands its bytes over in, and the
// window it is read through, the engine's own (64 KiB) where `margin` and
// `block` are absent, or one whose margin and block are a few units, which
// the hidden `parseReaderWithSizes` sets, its reads as small as the pieces.
type Reading = { size: number; margin?: number; block?: number }

const PIECES = [1, 3, 64]
const WINDOWS = [
  {},
  { margin: 1, block: 1 },
  { margin: 3, block: 2 },
  { margin: 64, block: 64 },
]

// A fixture row is read in every pieces size through every window.
const ROW_READS: Reading[] =
  PIECES.flatMap((size) => WINDOWS.map((window) => ({ size, ...window })))

// A conformance document in pieces of 1, 3 and 64 bytes through the
// engine's window, and through windows of 1 and 1, 3 and 2, and 64 and 64
// units in pieces of 1, 3 and 64.
const DOCUMENT_READS: Reading[] = [
  { size: 1 },
  { size: 3 },
  { size: 64 },
  { size: 1, margin: 1, block: 1 },
  { size: 3, margin: 3, block: 2 },
  { size: 64, margin: 64, block: 64 },
]

// A long document in pieces of 1, 3, 64 and 4096 bytes through the engine's
// window, whose edges its long tokens straddle, and through a window of 64
// and 64 units.
const LONG_READS: Reading[] = [
  { size: 1 },
  { size: 3 },
  { size: 64 },
  { size: 4096 },
  { size: 64, margin: 64, block: 64 },
]

type Log = { tokens: string[]; rules: string[] }

// `tn` with a log of the tokens the lexer gives and the rule passes.
function logged(tn: Tabnas, log: Log): Tabnas {
  tn.sub({
    lex: (tkn: any) => {
      log.tokens.push(
        tkn.name + ' ' + JSON.stringify(tkn.src) + ' ' +
        JSON.stringify(tkn.val) + ' ' + tkn.sI + ' ' + tkn.rI + ':' + tkn.cI,
      )
    },
    ruleDone: (rule: any, _ctx: any, done: any) => {
      log.rules.push(rule.name + ' ' + done.state + ' ' + rule.d)
    },
  })
  return tn
}

// What a parse showed: its value, or its error without the excerpt
// (`src`, the line it is on); and the tokens and rule passes `log`
// gathered, which is empty again after.
function seen(log: Log, parse: () => any) {
  let outcome: string
  try {
    const value = parse()
    outcome = 'ok ' + (undefined === value ? 'undefined' : JSON.stringify(value))
  } catch (e: any) {
    if (null == e || 'function' !== typeof e.toJSON) {
      outcome = 'throw ' + String(e && e.message)
    } else {
      const { src: _excerpt, ...rest } = e.toJSON()
      outcome = 'err ' + JSON.stringify(rest)
    }
  }
  const tokens = log.tokens
  const rules = log.rules
  log.tokens = []
  log.rules = []
  return { outcome, tokens, rules }
}

// `input` parsed whole and from a reader in each of `reads`; the count of
// reader parses compared. The text is the one its UTF-8 bytes decode to,
// so that a lone surrogate, which no reader gives, cannot set the two
// apart; a byte order mark stays the source's U+FEFF, as the engine
// decodes it.
function differential(
  tn: Tabnas, log: Log, where: string, input: string, reads: Reading[],
): number {
  const bytes = new Uint8Array(Buffer.from(input, 'utf8'))
  const text = new TextDecoder('utf-8', { ignoreBOM: true }).decode(bytes)
  const whole = seen(log, () => tn.parse(text))
  for (const { size, margin, block } of reads) {
    const read = seen(log, () => undefined === margin || undefined === block
      ? tn.parseReader(pieces(bytes, size))
      : tn.parseReaderWithSizes(pieces(bytes, size), { margin, block, chunk: size }))
    const at = where + ' in pieces of ' + size +
      (undefined === margin ? '' : ', margin ' + margin + ', block ' + block)
    assert.equal(read.outcome, whole.outcome, at)
    assert.deepEqual(read.tokens, whole.tokens, at)
    assert.deepEqual(read.rules, whole.rules, at)
  }
  return reads.length
}

// The parser a fixture row asks for, built by `make`, with the window.
function rowParser(opts: string, log: Log): Tabnas {
  return logged('' === opts.trim() ? make() : make(JSON.parse(opts)), log)
}

// Every `.xml` file under `dir`, in a stable order.
function xmlFiles(dir: string, into: string[] = []): string[] {
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) xmlFiles(path, into)
    else if (name.endsWith('.xml')) into.push(path)
  }
  return into
}

// A document of every kind of token, the long ones longer than the
// engine's 64 KiB window, and characters past ASCII among them, so that
// tokens of every kind straddle the window's edges as a reader hands the
// text over. The same document as `long_document` in
// rs/tests/window_test.rs and `longDocument` in go/window_test.go.
function longDocument(): string {
  const parts: string[] = []
  parts.push('\uFEFF<?xml version="1.0" encoding="UTF-8" standalone="no"?>\n')
  parts.push('<!DOCTYPE doc [\n')
  parts.push('  <!ENTITY who "w\u00f6rld">\n  <!ATTLIST x:item kind CDATA "plain">\n')
  for (let i = 0; i < 1300; i++) {
    parts.push('  <!ENTITY e' + i + ' "entity \u20ac ' + i + '"> <!-- ' + i + ' > ] \' -->\n')
  }
  parts.push(']>\n')
  parts.push('<!-- ' + 'a comment past the window, \u00e9 \ud834\udd1e - '.repeat(2000) + '-->\n')
  parts.push('<doc xmlns="urn:doc" xmlns:x="urn:x" note="' +
    'an attribute value past the window &amp; \u00fc '.repeat(1600) + '">\n')
  for (let i = 0; i < 1000; i++) {
    parts.push('  <x:item n="' + i + '" x:at=\'' + (i % 7) + '\'>&who; &#' + (60 + (i % 40)) +
      '; &#x' + (0x4e00 + i).toString(16) + '; t\u00ebxt ' + i + '</x:item>\r\n')
    if (500 === i) {
      parts.push('  <![CDATA[' + 'cdata <b>not markup</b> \u00e7 ]] > '.repeat(2200) + ']]>\n')
      parts.push('  <?pi ' + 'an instruction past the window \u00f1 ? > '.repeat(1800) + '?>\n')
      parts.push('  <text>' +
        'character data past the window, \u00e5 &amp; &lt;x&gt; \r\n'.repeat(1300) + '</text>\n')
      parts.push('  <e' + 'n'.repeat(70000) + '/>\n')
      parts.push('  <empty   a="1"\n     b = "2"  />\n')
    }
  }
  parts.push('</doc>\n<!-- after -->\n')
  return parts.join('')
}

describe('window', () => {
  test('make reads a reader a window at a time', () => {
    assert.equal((make().options as any).lex.window, true)
    assert.equal((make({ strictNamespaces: true }).options as any).lex.window, true)
    // The plugin itself declares nothing: a grammar built on it (feed)
    // declares the window on its own parser, once its own actions are
    // verified.
    assert.notEqual((new Tabnas().use(Xml).options as any).lex.window, true)
  })

  test('make refuses embed mode, which needs a jsonic engine', () => {
    assert.throws(() => make({ embed: true }), /embed mode needs a jsonic host/)
  })

  test('every fixture row parses the same from a reader', () => {
    const log: Log = { tokens: [], rules: [] }
    const files = loadSpecDir(findSpecDir(__dirname))
    let rows = 0
    let compared = 0
    files.forEach((file, index) => {
      for (const row of file.rows) {
        const input = unescapeInput(row.named('input'))
        compared += differential(rowParser(row.named('opts'), log), log, row.where(), input, ROW_READS)
        rows++
      }
      console.log(
        'window fixtures: ' + (index + 1) + ' of ' + files.length + ' files (' +
        Math.floor(((index + 1) * 100) / files.length) + '%), ' + compared + ' reader parses',
      )
    })
    assert.ok(200 < rows, String(rows))
    assert.equal(compared, rows * ROW_READS.length)
  })

  test('embed mode on a jsonic engine reads a reader a window at a time too', () => {
    // Embed mode is built on jsonic's engine, which declares no window, so
    // the window is declared here: the plugin's matcher is the same in both
    // modes, and is held to the same.
    const log: Log = { tokens: [], rules: [] }
    const tn = logged(new Tabnas().use(jsonic).use(Xml, { embed: true }), log)
    tn.options({ lex: { window: true } })
    let compared = 0
    for (const input of [
      '{a: <x>1, 2</x>, b: [<y/>, <z q="r">s</z>]}',
      'a: <p>T&amp;J <b>bold</b> <![CDATA[<raw>]]> <!-- c --></p>',
      '[<a/>, <b></b>, 3]',
      '{a: <x>unclosed}',
      '{a: <x></y>}',
    ]) {
      compared += differential(tn, log, JSON.stringify(input), input, ROW_READS)
    }
    assert.equal(compared, 5 * ROW_READS.length)
  })

  test('a byte order mark is dropped at the source\'s start, not the window\'s', () => {
    // A parser that keeps no consumed token for a rewind lets the window
    // begin at the cursor, so the window's start is no sign of the
    // source's: a U+FEFF there is character data, as in the whole text.
    const log: Log = { tokens: [], rules: [] }
    const tn = logged(make(), log)
    tn.options({ rewind: { history: 0 } })
    let compared = 0
    for (const input of [
      '<a>\uFEFFx</a>',
      '<a><b/>\uFEFFx</a>',
      '\uFEFF<a><b/>\uFEFFx</a>',
      '<a/>\uFEFF',
    ]) {
      compared += differential(tn, log, JSON.stringify(input), input, ROW_READS)
    }
    assert.equal(compared, 4 * ROW_READS.length)
  })

  test('every document of the W3C conformance suite parses the same from a reader', () => {
    const log: Log = { tokens: [], rules: [] }
    const tn = logged(make(), log)
    const files = xmlFiles(CONF_ROOT)
    assert.ok(3000 < files.length, String(files.length))
    let compared = 0
    let next = 0
    files.forEach((file, index) => {
      // The corpus mixes UTF-8, UTF-16 and UTF-32; decodeBOM transcodes, as
      // the conformance suite reads each document.
      const input = decodeBOM(readFileSync(file))
      compared += differential(tn, log, file.substring(CONF_ROOT.length + 1), input, DOCUMENT_READS)
      if (index + 1 === files.length || next <= index) {
        next = index + 250
        console.log(
          'window conformance: ' + (index + 1) + ' of ' + files.length + ' documents (' +
          Math.floor(((index + 1) * 100) / files.length) + '%), ' + compared + ' reader parses',
        )
      }
    })
  })

  test('a long document parses the same from a reader', () => {
    const log: Log = { tokens: [], rules: [] }
    const tn = logged(make(), log)
    const text = longDocument()
    assert.ok(512 * 1024 < text.length, String(text.length))
    const value = tn.parse(text)
    log.tokens = []
    log.rules = []
    assert.equal(value.name, 'doc')
    assert.equal(differential(tn, log, 'the long document', text, LONG_READS), LONG_READS.length)
  })

  test('a long document fails the same from a reader', () => {
    // A construct that runs past the engine's window and is then refused,
    // or never ends: the error is the whole text's, code, place and token.
    const log: Log = { tokens: [], rules: [] }
    const tn = logged(make(), log)
    const pad = '<p>' + 'x\u00e9 '.repeat(30000) + '</p>'
    const bad = [
      '<a>' + pad + '<!-- ' + 'c '.repeat(40000),
      '<a>' + pad + '<![CDATA[' + 'd '.repeat(40000),
      '<a>' + pad + '<?pi ' + 'p '.repeat(40000),
      '<!DOCTYPE a [' + '<!ENTITY e "v">'.repeat(6000) + '<a>' + pad + '</a>',
      '<a>' + pad + 't '.repeat(40000) + ']]>' + 't '.repeat(10) + '</a>',
      '<a>' + pad + '<b c="' + 'v '.repeat(40000) + '<"/></a>',
      '<a>' + pad + '<b c="' + 'v '.repeat(40000),
      '<a>' + pad + '<!-- ' + 'c '.repeat(40000) + '-- -->',
      '<a>' + pad + '</b>',
      '<a>' + pad + '</a>' + 'trailing '.repeat(10000),
    ]
    let compared = 0
    bad.forEach((text, index) => {
      const whole = seen(log, () => tn.parse(text))
      assert.ok(whole.outcome.startsWith('err '), whole.outcome.slice(0, 200))
      compared += differential(tn, log, 'long error document ' + index, text, LONG_READS)
    })
    assert.equal(compared, bad.length * LONG_READS.length)
  })
})
