/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

// PROTOTYPE POLLUTION
//
// Every map keyed by a name the *document* controls must be allocated without
// a prototype, the way the core allocates nodes (@tabnas/parser builtins:
// "no prototype, like JSON"). On a plain {} literal the name __proto__ is not
// an ordinary key: reading it yields Object.prototype and writing it runs the
// Object.prototype setter. That turns a parsed document into a write onto
// Object.prototype, visible to every object in the process.
//
// These tests pin both halves: no global pollution, and __proto__ surviving
// as an ordinary key (which is what jsonic, json5 and zon already do).
//
// Expectations are compared as JSON TEXT on purpose. The same hazard applies
// to the test itself: in a JavaScript object literal `{ __proto__: x }` sets
// the prototype and defines no key at all, so an expected literal written
// that way silently asserts the wrong thing.

import { test, describe } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml } from '../dist/xml'

const p = (src: string) => new Tabnas().use(jsonic).use(Xml).parse(src)

// Fails loudly rather than leaking a poisoned prototype into sibling tests.
function noGlobalPollution() {
  const leaked = ['pwned', 'polluted', 'injected']
    .filter((k) => undefined !== ({} as any)[k])
  for (const k of leaked) delete (Object.prototype as any)[k]
  assert.deepEqual(leaked, [], 'Object.prototype was polluted: ' + leaked)
}

describe('prototype-pollution', () => {

  test('ATTLIST for an element named __proto__ does not write Object.prototype', () => {
    // The attribute-default table is keyed by element name. Before the fix
    // this single document set Object.prototype.pwned for the whole process.
    const out: any = p('<!DOCTYPE d [<!ATTLIST __proto__ pwned CDATA "OWNED">]><d/>')
    noGlobalPollution()
    assert.equal(out.name, 'd')
  })

  test('ATTLIST default still applies to an element named __proto__', () => {
    const out: any = p(
      '<!DOCTYPE d [<!ATTLIST __proto__ a CDATA "def">]><d><__proto__/></d>')
    noGlobalPollution()
    assert.deepEqual(out.children[0].attributes, { a: 'def' })
  })

  test('entity named __proto__ is declared and expands', () => {
    const out: any = p('<!DOCTYPE d [<!ENTITY __proto__ "PWNED">]><d>&__proto__;</d>')
    noGlobalPollution()
    assert.deepEqual(out.children, ['PWNED'])
  })

  test('an inherited name is not a declared entity', () => {
    assert.throws(() => p('<d>&toString;</d>'))
    noGlobalPollution()
  })

  test('attribute named __proto__ is kept', () => {
    const out: any = p('<d __proto__="1" b="2"/>')
    noGlobalPollution()
    assert.equal(JSON.stringify(out.attributes), '{"__proto__":"1","b":"2"}')
  })

  test('namespace prefix named __proto__ binds', () => {
    const out: any = p('<d xmlns:__proto__="urn:x"><__proto__:e/></d>')
    noGlobalPollution()
    assert.equal(out.children[0].namespace, 'urn:x')
  })

})
