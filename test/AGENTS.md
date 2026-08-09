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
| `input` | XML source. Escapes `\n` `\r` `\t` `\\` `\uXXXX` are decoded. |
| `expected` | The parse result as JSON, or `ERROR` / `ERROR:<code>` for input that must be rejected. |
| `opts` | Optional JSON object of plugin options (empty means defaults). |
| `msg` | Optional substring the rendered error message must contain. Only meaningful alongside an `ERROR` expectation; it exists so a message template that stops interpolating (and emits a literal `{openname}` / `$openname`) fails a test instead of shipping. |

`\uXXXX` decodes a BMP code point. Use it for characters that must not be
written literally into a fixture — a leading U+FEFF byte-order mark above
all, which is invisible in an editor and easy to destroy. Note the Go
runner sees U+FEFF as its UTF-8 encoding (`EF BB BF`) and the TS runner as
a single UTF-16 code unit; the plugin handles both, so the same row covers
both runtimes.

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
They never skip. See the conformance section of the root `AGENTS.md` for the
two layers (narrow floors, catalogue-wide sweep) and the measured numbers.

Behavioural cases still belong here in `spec/`.

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
