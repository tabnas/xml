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

`test/xmlconf/` is separate: the W3C conformance suite, fetched on demand by
`scripts/fetch-xml-suite.sh` and checked against pass-rate floors rather than
exact output. Behavioural cases belong here in `spec/`.

## Rules

- Prefer adding a fixture here over a one-off in-language assertion when a
  case is expressible as input → output. That is what keeps the two runtimes
  honest against each other.
- TypeScript is canonical. If the two runtimes disagree, the TS behaviour is
  the expected value — unless Go has exposed a genuine TS defect, in which
  case fix TS first and pin the corrected behaviour here.
- A new fixture must pass in BOTH runtimes: run `go test ./...` (from `go/`)
  and `npm test` (from `ts/`) before considering it done.
