// @ts-nocheck
/* Copyright (c) 2013-2026 Richard Rodger, MIT License */

/*  doc-examples.test.js
 *  Doc-example harness: extracts fenced ```js / ```javascript code blocks
 *  from this repo's README and docs, runs every block that contains a
 *  `// =>` assertion, and checks each `<expr> // => <expected>` line.
 *
 *  A block opts in to testing by including at least one `// =>` line.
 *  Blocks with no `// =>` are skipped (illustrative snippets). Mark a
 *  block ` ```js ignore ` (info string) to exclude it explicitly.
 *
 *  `require(...)` inside an example resolves from this package's
 *  node_modules; unresolved `@tabnas/<x>` specifiers fall back to the
 *  sibling repo `<tabnas-folder>/<x>/ts` (local, unpublished dev layout).
 *  Identical across all tabnas repos — discovers docs relative to the repo.
 */
'use strict'

const { describe, it } = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const TS_DIR = path.join(__dirname, '..') // <repo>/ts
const REPO = path.join(TS_DIR, '..') // <repo>
const TABNAS = path.join(REPO, '..') // the tabnas folder (siblings)

const OWN_NAME = (() => {
  try {
    return require(path.join(TS_DIR, 'package.json')).name
  } catch {
    return null
  }
})()

// Candidate doc locations, relative to the repo root. Missing ones skipped.
const DOC_GLOBS = [
  'README.md',
  'ts/README.md',
  'go/README.md',
  'ts/doc',
  'doc',
  'docs',
]

function collectMarkdown() {
  const out = []
  const add = (p) => {
    if (fs.existsSync(p) && fs.statSync(p).isFile() && p.endsWith('.md')) {
      out.push(p)
    }
  }
  const walk = (dir) => {
    if (!fs.existsSync(dir) || !fs.statSync(dir).isDirectory()) return
    for (const e of fs.readdirSync(dir)) {
      if (e === 'node_modules' || e.startsWith('dist') || e === '.git') continue
      const p = path.join(dir, e)
      const st = fs.statSync(p)
      if (st.isDirectory()) walk(p)
      else add(p)
    }
  }
  for (const g of DOC_GLOBS) {
    const p = path.join(REPO, g)
    if (g.endsWith('.md')) add(p)
    else walk(p)
  }
  return [...new Set(out)]
}

// Extract fenced js/javascript blocks with their starting line number.
function extractBlocks(src) {
  const lines = src.split('\n')
  const blocks = []
  let cur = null
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const open = line.match(/^```+\s*(\w+)?\s*(\w+)?\s*$/)
    if (cur) {
      if (/^```+\s*$/.test(line)) {
        blocks.push(cur)
        cur = null
      } else {
        cur.code.push(line)
      }
    } else if (open && /^(js|javascript)$/i.test(open[1] || '')) {
      const info = (open[2] || '').toLowerCase()
      cur = { lang: open[1], ignore: info === 'ignore', startLine: i + 2, code: [] }
    } else if (open && /^```/.test(line) && (open[1] || open[2])) {
      // a non-js fence; consume until close so its body isn't scanned
      let j = i + 1
      while (j < lines.length && !/^```+\s*$/.test(lines[j])) j++
      i = j
    }
  }
  return blocks
}

// import { A } from 'x' -> const { A } = require('x'); default + namespace too.
function importsToRequire(code) {
  return code
    .replace(
      /^\s*import\s+\*\s+as\s+(\w+)\s+from\s+(['"][^'"]+['"]).*$/gm,
      'const $1 = require($2)',
    )
    .replace(
      /^\s*import\s+(\{[^}]*\})\s+from\s+(['"][^'"]+['"]).*$/gm,
      'const $1 = require($2)',
    )
    .replace(
      /^\s*import\s+(\w+)\s+from\s+(['"][^'"]+['"]).*$/gm,
      'const $1 = require($2)',
    )
}

// Rewrite `<expr>  // => <expected>` into __eq(expr, expected) calls.
//
// Two forms are recognised:
//
//   trailing:  expr   // => expected
//
//   block:     expr
//              // => {
//              //   ...expected, continued over comment lines
//              // }
//
// The block form is how every multi-line expected value in these docs is
// written (the root README example among them). It used to be dropped
// entirely: the arrow line's code part is empty, so no assertion was built.
const ARROW = /\/\/\s*=>(.*)$/
// ARROW is applied line by line, where `$` means end of line. The block-level
// "does this example claim an expected value?" probe ran ARROW.test() against
// the whole joined block, where `$` means end of the WHOLE STRING — so a block
// whose `// =>` was not on its final line was silently dropped and never
// tested at all. Probe with an anchorless pattern instead.
const ARROW_ANY = /\/\/\s*=>/
const COMMENT_LINE = /^\s*\/\/(.*)$/

function rewriteAssertions(code) {
  let count = 0
  const lines = code.split('\n')
  const out = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]

    // -- block form: a pure-comment line opening with `=>` -------------------
    const cm = line.match(COMMENT_LINE)
    if (cm) {
      const am = cm[1].match(/^\s*=>(.*)$/)
      if (am) {
        // The expected value is this line plus any immediately following
        // pure-comment continuation lines (stopping at the next `=>`).
        let expected = am[1]
        let j = i + 1
        while (j < lines.length) {
          const nm = lines[j].match(COMMENT_LINE)
          if (!nm || /^\s*=>/.test(nm[1])) break
          expected += '\n' + nm[1]
          j++
        }
        // The expression is the last non-blank line already emitted, provided
        // it looks like a bare expression rather than a statement.
        let k = out.length - 1
        while (0 <= k && out[k].trim() === '') k--
        const expr = 0 <= k ? out[k] : ''
        const bare =
          expr.trim() !== '' &&
          !COMMENT_LINE.test(expr) &&
          !/[;{}(,]\s*$/.test(expr) &&
          !/^\s*(const|let|var|function|class|return|if|for|while|import|export)\b/.test(
            expr,
          )
        if (expected.trim() !== '' && bare) {
          const indent = expr.match(/^\s*/)[0]
          out[k] = `${indent}__eq((${expr.trim()}), (${expected.trim()}));`
          count++
          i = j - 1
          continue
        }
      }
      out.push(line)
      continue
    }

    // -- trailing form ------------------------------------------------------
    const m = line.match(ARROW)
    if (!m) {
      out.push(line)
      continue
    }
    const expected = m[1].trim()
    if (expected === '') {
      out.push(line) // `// =>` with no value: leave as comment
      continue
    }
    const codePart = line.slice(0, m.index).replace(/[;\s]+$/, '')
    if (codePart.trim() === '') {
      out.push(line)
      continue
    }
    const indent = line.match(/^\s*/)[0]
    count++
    out.push(`${indent}__eq((${codePart}), (${expected}));`)
  }
  return { code: out.join('\n'), count }
}

function deepEq(actual, expected) {
  const norm = (v) => JSON.parse(JSON.stringify(v ?? null))
  try {
    assert.deepStrictEqual(actual, expected)
    return
  } catch {}
  // Fall back to JSON-normalised compare (null-proto objects, etc.).
  assert.deepStrictEqual(norm(actual), norm(expected))
}

function makeRequire() {
  return function patchedRequire(spec) {
    try {
      return require(spec)
    } catch (e) {
      if (e && e.code === 'MODULE_NOT_FOUND') {
        // The repo's own package (self-reference fallback).
        if (OWN_NAME && (spec === OWN_NAME || spec.startsWith(OWN_NAME + '/'))) {
          return require(TS_DIR + spec.slice(OWN_NAME.length))
        }
        // A sibling @tabnas/<x> package -> <tabnas-folder>/<x>/ts (local dev).
        const m = spec.match(/^@tabnas\/([^/]+)(\/.*)?$/)
        if (m) return require(path.join(TABNAS, m[1], 'ts') + (m[2] || ''))
      }
      throw e
    }
  }
}

describe('doc-examples', () => {
  const files = collectMarkdown()
  let testable = 0
  // Blocks that advertise an expected value with `// =>` but from which no
  // assertion could be built. Those used to vanish silently, so a documented
  // example could be wrong forever without any test noticing. They are now
  // reported as a failure.
  const unasserted = []

  for (const file of files) {
    const rel = path.relative(REPO, file)
    const blocks = extractBlocks(fs.readFileSync(file, 'utf8'))
    blocks.forEach((b, bi) => {
      if (b.ignore) return
      const joined = b.code.join('\n')
      if (!ARROW_ANY.test(joined)) return // genuinely illustrative: no `// =>`
      const { code, count } = rewriteAssertions(importsToRequire(joined))
      const label = `${rel} block #${bi + 1} (line ${b.startLine})`
      if (count === 0) {
        unasserted.push(label)
        return
      }
      testable++
      it(label, () => {
        const isAsync = /\bawait\b/.test(code)
        const body = isAsync ? `return (async () => {\n${code}\n})()` : code
        const fn = new Function('require', '__eq', body)
        return fn(makeRequire(), deepEq)
      })
    })
  }

  it('at least one documented example is actually asserted', () => {
    // Was `assert.ok(testable >= 0, ...)` — a count is always >= 0, so that
    // assertion could not fail. If the extractor ever stopped finding any
    // example, the suite stayed green while testing nothing.
    assert.ok(
      0 < testable,
      `no documented example was asserted (found ${files.length} markdown file(s)). ` +
        'Either the docs lost their `// =>` examples or the extractor regressed.',
    )
  })

  it('every `// =>` example yields a real assertion', () => {
    assert.deepStrictEqual(
      unasserted,
      [],
      'these documented blocks contain `// =>` but produced no assertion, ' +
        'so their stated output is never checked:\n  ' +
        unasserted.join('\n  '),
    )
  })
})
