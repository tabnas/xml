# @tabnas/xml

This plugin allows the [Jsonic](https://jsonic.senecajs.org) JSON parser to support xml syntax.

This repository contains:

| Path | Description |
|---|---|
| [`ts/`](ts/) | TypeScript / JavaScript implementation. |
| [`go/`](go/) | Go port. |
| [`test/spec/`](test/spec/) | Shared conformance fixtures, exercised by both runtimes. |

## Grammar

The grammar is defined once in the top-level
[`xml-grammar.jsonic`](xml-grammar.jsonic). It is embedded into both
implementations by [`ts/embed-grammar.js`](ts/embed-grammar.js): the
grammar text is spliced verbatim into the TypeScript source
([`ts/src/xml.ts`](ts/src/xml.ts)), and the Go source
([`go/xml.go`](go/xml.go)) mirrors the same grammar. Run
`cd ts && npm run build` (or `npm run embed`) after editing the grammar
to re-embed it.

See [`ts/README.md`](ts/README.md) for usage.

## Grammar diagram

The grammar as a railroad/syntax diagram, generated from the live grammar
with [`@tabnas/railroad`](https://github.com/tabnas/railroad):

![xml grammar railroad diagram](ts/doc/grammar.svg)

ASCII version: [`ts/doc/grammar.txt`](ts/doc/grammar.txt).

## License

MIT. Copyright (c) Richard Rodger.
