/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

// The input column of the shared fixtures, decoded: `xml-spec.test.ts`
// runs the rows, and `window.test.ts` parses the same inputs from a reader.

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
export function unescapeInput(s: string): string {
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
