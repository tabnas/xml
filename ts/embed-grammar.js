#!/usr/bin/env node

// Embed xml-grammar.jsonic into TypeScript source files.
// Run via: npm run embed  (or:  node embed-grammar.js)

const fs = require('fs')
const path = require('path')

const GRAMMAR_FILE = path.join(__dirname, 'xml-grammar.jsonic')
const TS_FILE = path.join(__dirname, 'src', 'xml.ts')

const BEGIN = '// --- BEGIN EMBEDDED xml-grammar.jsonic ---'
const END = '// --- END EMBEDDED xml-grammar.jsonic ---'

const grammar = fs.readFileSync(GRAMMAR_FILE, 'utf8')

// --- TypeScript embedding ---
function embedTS() {
  let src = fs.readFileSync(TS_FILE, 'utf8')
  const startIdx = src.indexOf(BEGIN)
  const endIdx = src.indexOf(END)
  if (startIdx === -1 || endIdx === -1) {
    console.error('TS markers not found in', TS_FILE)
    process.exit(1)
  }

  // Escape backticks and template expressions for a JS template literal.
  const escaped = grammar
    .replace(/\\/g, '\\\\')
    .replace(/`/g, '\\`')
    .replace(/\$\{/g, '\\${')

  const replacement =
    BEGIN +
    '\nconst grammarText = `\n' +
    escaped +
    '`\n' +
    END

  src = src.substring(0, startIdx) + replacement + src.substring(endIdx + END.length)
  fs.writeFileSync(TS_FILE, src)
  console.log('Embedded grammar into', TS_FILE)
}

embedTS()
