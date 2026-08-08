/* Copyright (c) 2026 Richard Rodger, MIT License */

// The exported VERSION must equal package.json "version".
//
// This is the CI check for version drift. It exists because the constant HAS
// drifted: @tabnas/json exported Version = '1.0.0' for several releases while
// the package shipped 0.4.x, because nothing rewrote it and AGENTS.md wrongly
// claimed `make publish-go` kept it in sync. A release that bumps
// package.json and forgets the constant now fails here.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

// At runtime this test file is loaded from `dist-test/`, so hop up one level
// to reach the package root. Read package.json directly rather than importing
// it: an unreadable or malformed file must FAIL the suite, never skip it — a
// version check that silently does not run is the exact failure mode this
// test is designed out to prevent.
const pkg = (() => {
  const pkgPath = join(__dirname, '..', 'package.json')
  let raw: string
  try {
    raw = readFileSync(pkgPath, 'utf8')
  } catch (err: any) {
    throw new Error(
      `cannot read ${pkgPath}, so VERSION cannot be checked: ${err.message}`,
    )
  }
  return JSON.parse(raw) as { name: string; version: string }
})()

// Load through the package root, exactly as a consumer would, so the test
// also proves VERSION is reachable from the published entry point.
const api = require('..')

describe('version', () => {
  test('VERSION matches package.json', () => {
    assert.ok(pkg.version, 'package.json has no version field')
    assert.equal(
      api.VERSION,
      pkg.version,
      `VERSION drift: ${pkg.name} exports ${api.VERSION} but package.json is ` +
        `${pkg.version}. Both are rewritten by admin/publish.sh at release; ` +
        `if you bumped one by hand, bump the other.`,
    )
  })

  test('VERSION is exported and looks like a semver', () => {
    assert.equal(
      typeof api.VERSION,
      'string',
      'VERSION must be exported as a string',
    )
    assert.match(api.VERSION, /^\d+\.\d+\.\d+/, 'VERSION must be a semver')
  })
})
