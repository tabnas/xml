# Agents Guide — shared spec fixtures

`spec/*.tsv` holds the cross-runtime conformance fixtures. Both runtimes
auto-discover and run **every** file in this directory, so a change here
affects TypeScript and Go together — edit with that in mind.

## Format

Tab-separated, one case per line. Lines starting with `#` are comments; each
file opens with a legend naming the columns.

| Column | Meaning |
|---|---|
| `name` | Unique case identifier — it names the sub-test in both runtimes. |
| `input` | XML source. Escapes `\n` `\r` `\t` `\\` are decoded. |
| `expected` | The parse result as JSON, or `ERROR` / `ERROR:<code>` for input that must be rejected. |
| `opts` | Optional JSON object of plugin options (empty means defaults). |

`expected` and `opts` are **not** escape-decoded — they are raw JSON, so
JSON's own escape rules apply.

Results are compared after a JSON round-trip, so key order does not affect
the comparison.

## Who runs what

- TypeScript: `ts/test/xml.test.ts` — reads `../../test/spec` at runtime
  from `dist-test/`.
- Go: `go/xml_test.go` — `TestSpec` globs `../test/spec/*.tsv`.

Both discover files by directory listing: adding a `.tsv` here runs it in
both runtimes without touching either runner. (The Go side used to name each
file by hand, and that list had gone stale — `dtd-attlist`, `dtd-entities`
and `xmlspace-lang` were running in TypeScript only.)

`test/xmlconf/` is separate: the W3C XML Conformance Test Suite. It is
**never committed** (W3C-owned, not redistributed) — `scripts/fetch-xml-suite.sh`
downloads the pinned `xmlts20130923.tar.gz`, verifies its SHA-256, and
extracts it into the gitignored `test/xmlconf/`. The fetch runs automatically
before the tests (`pretest` in `ts/package.json`, `TestMain` in
`go/xmlconf_test.go`) and the tests **fail loudly** if the corpus is absent.
They never skip. Behavioural cases still belong here in `spec/`.

## The conformance baseline (measured 2026-08-07, parser unchanged)

Catalog: 2586 tests, of which 2312 are in scope (RECOMMENDATION XML1.0 —
all errata editions — or NS1.0; XML1.1 / NS1.1 are a different language
version this package does not claim). `error`-type tests (28) are not
asserted: the spec leaves reporting to the processor's discretion.

| measure | TypeScript | Go |
|---|---|---|
| `valid` accepted **and** canonical output matched | 621 / 729 (85.2%) | 623 / 729 (85.5%) |
| — of which merely parsed | 697 / 729 | 699 / 729 |
| — value-compared (catalog supplies OUTPUT) | 232 / 308 | 232 / 308 |
| `not-wf` rejected | **404 / 1326 (30.5%)** | **405 / 1326 (30.5%)** |
| `invalid` accepted (non-validating) | 216 / 229 (94.3%) | 216 / 229 (94.3%) |

The old harness reported this same parser as green, because it asserted
`>= 118/120` valid and `>= 30/186` not-wf over two directories of one
collection, and never compared a value. Do not restore floors, skips or
allow-lists. Leave a genuine gap RED.

## Rules

- Prefer adding a fixture here over a one-off in-language assertion when a
  case is expressible as input → output. That is what keeps the two runtimes
  honest against each other.
- TypeScript is canonical. If the two runtimes disagree, the TS behaviour is
  the expected value — unless Go has exposed a genuine TS defect, in which
  case fix TS first and pin the corrected behaviour here.
- A new fixture must pass in BOTH runtimes before it counts:
  `go test ./...` from `go/`, and **`npm run build && npm test`** from `ts/`.
  Plain `npm test` runs the previously compiled `dist-test/`, so it can pass
  without ever loading a newly added fixture.
