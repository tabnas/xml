/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The order the rules build an element in (xml-grammar.jsonic sets it
// out), and the two things that order must not cost: the value, which is
// what it always was, member order included, and rule depth, which a list
// of siblings must not grow.
//
// The order is what lets a reader of the rule events (tabnas-transduce's
// incremental source) stream a document as the parse proceeds: each
// element in a node of its own, made when its start tag is read, with
// its members added in their order; the `children` member named in
// `u.key` before its list opens; the list in a node of its own, each
// child appended when it is done; and the root element, once it is done,
// on the start rule, set aside while the white space after it is read,
// so that the rule replacing the start rule to read it does not open
// holding the finished document, and put back when the last rule closes.

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Xml } from '../dist/xml'

// Each rule pass as [state, rule, depth, node, u.key, transition], with a
// container named by the order it was first seen (M for a map, L for a
// list) and shown with its keys or length at the end of the pass.
function passes(tn: Tabnas, src: string): string[][] {
  const ids = new Map<object, number>()
  const node = (v: any): string => {
    if (null == v || 'object' !== typeof v) return String(v)
    if (!ids.has(v)) ids.set(v, ids.size)
    return Array.isArray(v)
      ? `L${ids.get(v)}[${v.length}]`
      : `M${ids.get(v)}{${Object.keys(v).join(',')}}`
  }
  const out: string[][] = []
  tn.sub({
    ruleDone: (rule: any, _ctx: any, done: any) => {
      const step = done.alt
        ? (done.alt.p ? 'p:' + done.alt.p : '') + (done.alt.r ? 'r:' + done.alt.r : '')
        : ''
      out.push([done.state, rule.name, String(rule.d), node(rule.node), rule.u.key ?? '', step])
    },
  })
  tn.parse(src)
  return out
}

describe('build-order', () => {
  test('each element is built in a node of its own, member by member, in document order', () => {
    assert.deepStrictEqual(passes(new Tabnas().use(Xml), '<a x="1">t<b/></a>\n'), [
      ['o', 'xml', '0', 'undefined', '', 'p:element'],
      // The element's node, with the members its start tag gives.
      ['o', 'element', '1', 'M0{name,localName,attributes}', '', 'p:head'],
      // head reads nothing: its close comes after those members.
      ['o', 'head', '2', 'M0{name,localName,attributes}', '', ''],
      ['c', 'head', '2', 'M0{name,localName,attributes}', '', 'r:content'],
      // content names the member it builds before the list opens.
      ['o', 'content', '2', 'M0{name,localName,attributes}', 'children', 'p:children'],
      // The list, in a node of its own.
      ['o', 'children', '3', 'L1[0]', '', 'p:child'],
      // A text child is in the list at once.
      ['o', 'child', '4', 'L1[1]', '', ''],
      ['c', 'child', '4', 'L1[1]', '', 'r:child'],
      // The next child replaces the last, at the same depth.
      ['o', 'child', '4', 'L1[1]', '', 'p:element'],
      // A self-closing element is complete when its start tag is read.
      ['o', 'element', '5', 'M2{name,localName,attributes,children}', '', ''],
      ['c', 'element', '5', 'M2{name,localName,attributes,children}', '', ''],
      // A finished element child is appended when the child closes.
      ['c', 'child', '4', 'L1[2]', '', ''],
      // The last child hands the list back to the rule whose node it is.
      ['c', 'children', '3', 'L1[2]', '', ''],
      // The finished list becomes `children`.
      ['c', 'content', '2', 'M0{name,localName,attributes,children}', 'children', ''],
      ['c', 'element', '1', 'M0{name,localName,attributes,children}', '', ''],
      // The white space after the root replaces the start rule, so the
      // root is set aside and the rule replacing it holds nothing; the
      // root is the result at the last close.
      ['c', 'xml', '0', 'undefined', '', 'r:xml'],
      ['o', 'xml', '0', 'undefined', '', ''],
      ['c', 'xml', '0', 'M0{name,localName,attributes,children}', '', ''],
    ])
  })

  test('rule depth over 10,000 siblings is what one sibling needs', () => {
    const maxDepth = (src: string) => {
      const tn = new Tabnas().use(Xml)
      let max = 0
      tn.sub({ ruleDone: (rule: any) => { if (rule.d > max) max = rule.d } })
      const value = tn.parse(src) as any
      return { max, value }
    }
    const one = maxDepth('<r><i>v</i></r>')
    const many = maxDepth('<r>' + '<i>v</i>t'.repeat(10_000) + '</r>')
    assert.equal(many.value.children.length, 20_000)
    assert.equal(many.max, one.max)
    // xml 0, element 1, head and content 2, children 3, child 4, the
    // element <i> 5, its head and content 6, its children 7, its text
    // child 8.
    assert.equal(one.max, 8)
    // A level of nesting costs four rules: element, head (then content),
    // children and child.
    assert.equal(maxDepth('<r><i><i><i>v</i></i></i></r>').max, one.max + 8)
  })

  test('the value keeps its members in order: name, localName, attributes, children, then prefix, namespace, space, lang', () => {
    const json = (tn: Tabnas, src: string) => JSON.stringify(tn.parse(src))
    const pure = new Tabnas().use(Xml)
    assert.equal(
      json(pure, '<p:a xmlns:p="urn:p" xml:lang="en">hi<p:b/></p:a>'),
      '{"name":"p:a","localName":"a","attributes":{"xmlns:p":"urn:p","xml:lang":"en"},' +
        '"children":["hi",{"name":"p:b","localName":"b","attributes":{},"children":[],' +
        '"prefix":"p","namespace":"urn:p","lang":"en"}],"prefix":"p","namespace":"urn:p","lang":"en"}',
    )
    assert.equal(
      json(pure, '<a xmlns:p="u"><b xmlns:p="v" xml:space="preserve"><p:c/></b><p:d/></a>'),
      '{"name":"a","localName":"a","attributes":{"xmlns:p":"u"},"children":[' +
        '{"name":"b","localName":"b","attributes":{"xmlns:p":"v","xml:space":"preserve"},' +
        '"children":[{"name":"p:c","localName":"c","attributes":{},"children":[],' +
        '"prefix":"p","namespace":"v","space":"preserve"}],"space":"preserve"},' +
        '{"name":"p:d","localName":"d","attributes":{},"children":[],"prefix":"p","namespace":"u"}]}',
    )
    assert.equal(
      json(pure, '<?xml version="1.0"?>\n<!-- c -->\n<a xmlns="A"/>\n<!-- d -->\n'),
      '{"name":"a","localName":"a","attributes":{"xmlns":"A"},"children":[],"namespace":"A"}',
    )
    assert.equal(
      json(new Tabnas().use(Xml, { namespaces: false }), '<p:a xmlns:p="urn:p"><p:b/></p:a>'),
      '{"name":"p:a","localName":"p:a","attributes":{"xmlns:p":"urn:p"},' +
        '"children":[{"name":"p:b","localName":"p:b","attributes":{},"children":[]}]}',
    )
  })

  test('a namespace violation fails the document where it always did', () => {
    // The first violation in document order, raised when the root element
    // is done: a later one, or a later prefix, does not change the code.
    const strict = new Tabnas().use(Xml, { strictNamespaces: true })
    assert.throws(() => strict.parse('<a><p:b/><c xmlns:xml="x"/></a>'), { code: 'unbound_prefix' })
    assert.throws(() => strict.parse('<a><c xmlns:xml="x"/><p:b/></a>'), { code: 'reserved_namespace' })
    // A malformed document fails as malformed first.
    assert.throws(() => strict.parse('<a><p:b/></c>'), { code: 'xml_mismatched_tag' })
  })

  test('embed mode resolves each XML literal on its own, and stops at its first violation', () => {
    const embed = new Tabnas().use(jsonic).use(Xml, { embed: true, strictNamespaces: true })
    assert.equal(
      JSON.stringify(embed.parse('{k: <a xmlns:q="u"><p:b/><q:c><q:d/></q:c></a>, j: <q:e xmlns:q="w"/>}')),
      // p:b is the first violation: it keeps its prefix and local name,
      // and the elements after it in document order are left unresolved;
      // the second literal starts afresh.
      '{"k":{"name":"a","localName":"a","attributes":{"xmlns:q":"u"},"children":[' +
        '{"name":"p:b","localName":"b","attributes":{},"children":[],"prefix":"p"},' +
        '{"name":"q:c","localName":"q:c","attributes":{},"children":[' +
        '{"name":"q:d","localName":"q:d","attributes":{},"children":[]}]}]},' +
        '"j":{"name":"q:e","localName":"e","attributes":{"xmlns:q":"w"},"children":[],' +
        '"prefix":"q","namespace":"w"}}',
    )
  })
})
