#!/usr/bin/env node

// Embed xml-grammar.jsonic into the TypeScript source as JSON.
// Run via: npm run embed  (or:  node embed-grammar.js)
//
// The grammar is AUTHORED in jsonic, so it can carry comments, and SHIPPED
// as JSON. This script reads the jsonic text with @tabnas/jsonic, here, at
// build time, and writes the result into src/xml.ts between the
// `BEGIN/END EMBEDDED` markers as a JSON template literal, which the plugin
// reads with JSON.parse when it installs. So no runtime loads jsonic:
// @tabnas/jsonic is a devDependency, needed only by this script and the
// tests.
//
// The output is deterministic: JSON.stringify with a two-space indent, keys
// in the order the grammar file writes them, LF line endings, the same bytes
// on every machine. ts/test/grammar-json.test.ts reads the grammar file
// again and fails when the embedded copy is stale, so run this after every
// edit to xml-grammar.jsonic, and never hand-edit between the markers.
//
// Not part of `npm run build`: a build that rewrote the embedded JSON would
// make that test compare this script with itself. `npm run embed` runs it.
//
// The Rust crate carries the same JSON by hand (GRAMMAR_TEXT in
// rs/src/lib.rs, held to the grammar file by rs/tests/xml_test.rs); this
// script does not write it.

const fs = require('fs')
const path = require('path')

const GRAMMAR_FILE = path.join(__dirname, '..', 'xml-grammar.jsonic')
const TS_FILE = path.join(__dirname, 'src', 'xml.ts')

const BEGIN = '// --- BEGIN EMBEDDED xml-grammar.jsonic ---'
const END = '// --- END EMBEDDED xml-grammar.jsonic ---'

// The grammar as the engine's grammar document: the jsonic text, parsed.
function readGrammar() {
  // Resolved here rather than at the top, so that requiring this file for
  // its constants loads neither the engine nor jsonic.
  const { Tabnas } = require('@tabnas/parser')
  const { jsonic } = require('@tabnas/jsonic')
  const spec = new Tabnas().use(jsonic).parse(fs.readFileSync(GRAMMAR_FILE, 'utf8'))
  if (null == spec || null == spec.rule) {
    throw new Error('Grammar has no `rule` table: ' + GRAMMAR_FILE)
  }
  return spec
}

// The grammar as the JSON text the TypeScript source embeds.
function grammarJson() {
  return JSON.stringify(readGrammar(), null, 2)
}

// The whole marked block, markers included, for a given JSON text.
function embeddedBlock(json) {
  // Escape for a JS template literal. JSON never holds a backtick or `${`
  // outside a string, but a grammar string could, and JSON's own escapes
  // are backslashes.
  const escaped = json
    .replace(/\\/g, '\\\\')
    .replace(/`/g, '\\`')
    .replace(/\$\{/g, '\\${')
  return BEGIN + '\nconst grammarJson = `\n' + escaped + '\n`\n' + END
}

// The marked block as src/xml.ts carries it now, markers included.
function committedBlock(src) {
  const startIdx = src.indexOf(BEGIN)
  const endIdx = src.indexOf(END)
  if (-1 === startIdx || -1 === endIdx) {
    throw new Error('TS markers not found in ' + TS_FILE)
  }
  return src.substring(startIdx, endIdx + END.length)
}

function embedTS() {
  const src = fs.readFileSync(TS_FILE, 'utf8')
  const out = src.replace(committedBlock(src), () => embeddedBlock(grammarJson()))
  fs.writeFileSync(TS_FILE, out)
  console.log('Embedded grammar into', TS_FILE)
}

module.exports = {
  BEGIN,
  END,
  GRAMMAR_FILE,
  TS_FILE,
  readGrammar,
  grammarJson,
  embeddedBlock,
  committedBlock,
}

if (require.main === module) embedTS()
