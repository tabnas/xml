/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The embedded grammar. xml-grammar.jsonic is authored in jsonic and
// shipped as JSON: ts/embed-grammar.js (`npm run embed`) reads it with
// @tabnas/jsonic at build time and writes the JSON into src/xml.ts, and
// the plugin reads that with JSON.parse when it installs. These tests hold
// the arrangement:
//
// - the embedded block is what embed-grammar.js writes from the grammar
//   file today, byte for byte, so an edit to xml-grammar.jsonic that was
//   not re-embedded fails here rather than shipping a stale grammar;
// - the grammar the plugin installs is, compared deeply (values, key
//   order, kinds), the one a jsonic parse of the file gives, which is what
//   the plugin installed when it read the text with jsonic itself;
// - installing the plugin loads no @tabnas/jsonic. It is a devDependency,
//   needed only by the embed script and these tests.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { execFileSync } from 'node:child_process'
import Fs from 'node:fs'
import Path from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml } from '../dist/xml'

const TS = Path.join(__dirname, '..')

// The embed script, a plain file beside package.json. Required for its
// functions: requiring it writes nothing.
const gen = require(Path.join(TS, 'embed-grammar.js')) as {
  GRAMMAR_FILE: string
  TS_FILE: string
  readGrammar: () => any
  grammarJson: () => string
  embeddedBlock: (json: string) => string
  committedBlock: (src: string) => string
}

// A value as its kinds, keys in order and leaves: what two grammar
// documents must share to install the same grammar. Prototypes are left
// out on purpose: jsonic builds some maps without one, JSON.parse never
// does, and the engine reads neither.
function shape(v: any): any {
  if (Array.isArray(v)) return ['array', v.map(shape)]
  if (null != v && 'object' === typeof v) {
    return ['object', Object.keys(v).map((k) => [k, shape(v[k])])]
  }
  return [typeof v, v]
}

describe('embedded grammar', () => {
  test('src/xml.ts embeds what embed-grammar.js writes from the grammar file', () => {
    const committed = gen.committedBlock(Fs.readFileSync(gen.TS_FILE, 'utf8'))
    assert.ok(
      committed === gen.embeddedBlock(gen.grammarJson()),
      'the grammar embedded in ts/src/xml.ts is stale against ' +
        'xml-grammar.jsonic: run `npm run embed` (from ts/) and commit the ' +
        'result; never edit between the markers by hand',
    )
    // LF only, so the committed bytes are the same on every OS
    // (.gitattributes pins ts/src/xml.ts to eol=lf).
    assert.ok(!committed.includes('\r'), 'the embedded block has a CR')
  })

  test('the plugin installs the grammar a jsonic parse of the file gives', () => {
    // What the plugin installed before it shipped JSON: the grammar file's
    // text, read by a jsonic-grammar engine.
    const text = Fs.readFileSync(gen.GRAMMAR_FILE, 'utf8')
    const fromJsonic = new Tabnas().use(jsonic).parse(text)

    // What it installs now, caught on the way in. The plugin adds its
    // function references as `ref`, which the file does not carry.
    const seen: any[] = []
    const tn = new Tabnas()
    const grammar = tn.grammar.bind(tn)
    ;(tn as any).grammar = (def: any, ...rest: any[]) => {
      const { ref, ...data } = def
      seen.push(JSON.parse(JSON.stringify(data)))
      return grammar(def, ...rest)
    }
    tn.use(Xml)

    assert.equal(seen.length, 1, 'the plugin installs one grammar document')
    assert.deepStrictEqual(shape(seen[0]), shape(fromJsonic))
    assert.deepStrictEqual(shape(gen.readGrammar()), shape(fromJsonic))
    assert.deepStrictEqual(Object.keys(seen[0].rule), [
      'xml',
      'element',
      'head',
      'content',
      'children',
      'child',
    ])
  })

  test('installing the plugin loads no @tabnas/jsonic', () => {
    // A fresh process, so that nothing this suite loaded (jsonic, above)
    // can hide in the module cache. It records every request for jsonic,
    // installs the plugin on the bare engine, parses, and reports which
    // loaded files belong to jsonic's directory.
    const script = `
      const Module = require('node:module')
      const Fs = require('node:fs')
      const Path = require('node:path')
      const asked = []
      const load = Module._load
      Module._load = function (request, ...rest) {
        if (/^@tabnas\\/jsonic(\\/|$)/.test(request)) asked.push(request)
        return load.call(this, request, ...rest)
      }
      const { Tabnas } = require('@tabnas/parser')
      const { Xml } = require(${JSON.stringify(Path.join(TS, 'dist', 'xml.js'))})
      const value = new Tabnas().use(Xml).parse('<a x="1">t<b/></a>')
      Module._load = load
      let home = null
      try {
        const main = Fs.realpathSync(require.resolve('@tabnas/jsonic', { paths: [process.cwd()] }))
        let dir = Path.dirname(main)
        while (!Fs.existsSync(Path.join(dir, 'package.json'))) dir = Path.dirname(dir)
        home = dir + Path.sep
      } catch (e) {}
      const loaded = Object.keys(require.cache).filter((f) => null != home && f.startsWith(home))
      process.stdout.write(JSON.stringify({ value, asked, loaded, home }))
    `
    const out = JSON.parse(
      execFileSync(process.execPath, ['-e', script], { cwd: TS, encoding: 'utf8' }),
    )
    assert.deepStrictEqual(out.value, {
      name: 'a',
      localName: 'a',
      attributes: { x: '1' },
      children: ['t', { name: 'b', localName: 'b', attributes: {}, children: [] }],
    })
    assert.deepStrictEqual(out.asked, [])
    assert.deepStrictEqual(out.loaded, [])
    // Only worth something if it could have failed: jsonic is installed
    // here (a devDependency), so a stray require would have found it.
    assert.ok(out.home, '@tabnas/jsonic should be resolvable from ts/')
  })
})
