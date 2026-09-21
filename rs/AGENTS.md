# Agents Guide: rs/

The Rust port of the canonical TypeScript in [`../ts`](../ts). Read
[`../AGENTS.md`](../AGENTS.md) first: it holds the cross-runtime rules,
the option names and defaults, and the shape of the parsed element. This
file covers only what is specific to this crate.

## Layout

| Path | |
|---|---|
| `src/lib.rs` | `XmlOptions`, the embedded grammar, the typed reference registrations, `xml`, `plugin`, `make`, `make_with`, `parse` |
| `src/lex.rs` | the imperative `xmltag` matcher: the one place raw XML syntax is recognised |
| `src/entity.rs` | name and character classes, entity declaration and reference handling, DOCTYPE mining, line ending and attribute whitespace normalisation |
| `src/namespace.rs` | prefix resolution over the finished tree |
| `src/bom.rs` | `decode_bom` and `strip_bom` |
| `tests/parity_test.rs` | every `../test/spec/*.tsv` fixture through `tabnas_support::Runner`, plus the named-column census |
| `tests/xml_test.rs` | in-language cases mirrored from `go/xml_test.go`, `go/advance_col_test.go` and `go/perf_test.go` |
| `tests/xmlconf_test.rs` | the W3C conformance corpus, mirrored from `go/xmlconf_test.go` |
| `tests/version_test.rs` | `Cargo.toml`, `VERSION` and `ts/package.json` must agree |
| `tests/common/mod.rs` | the value normaliser and the fixture unescape both suites share |
| `README.md` | the crate front page; follows `../docs/STYLE-GUIDE.md` even though it is not yet in the gated list |

## Three crates by path

`Cargo.toml` takes `tabnas` (`../../parser/rs`), `tabnas-jsonic`
(`../../jsonic/rs`, which brings `tabnas-json`) and, as a
dev-dependency, `tabnas-support` (`../../support/rs`, feature
`serde_json`). None is published. Clone them as siblings before running
cargo, and expect `Cargo.lock` to move when one bumps its version:
`../ci/rust/run.sh` exempts exactly those entries when it diffs the
lock, and asserts everything else.

## The grammar is embedded, and a test holds it to the file

`GRAMMAR_TEXT` in `src/lib.rs`, between the `BEGIN`/`END EMBEDDED`
markers, is `../xml-grammar.jsonic` as JSON. Unlike chess and css,
`ts/embed-grammar.js` here has ONE target: it writes the raw jsonic TEXT
into `ts/src/xml.ts`, which parses it at load time. There is no Go
target and no JSON conversion in that script, so extending it to write
Rust would not be the mechanical change the porting playbook permits.
The embedded JSON is maintained by hand, and
`the_embedded_grammar_matches_xml_grammar_jsonic` parses the file with
`tabnas_jsonic` and compares, so an edit to the grammar that is not
carried across fails the suite. Edit the `.jsonic` file first, then the
constant.

## Every `@ref` is a typed registration

The grammar names eight references: `@no-root-yet`,
`@element-selfclose`, `@element-open`, `@element-is-selfclosed`,
`@element-close`, `@doc-text-open`, `@doc-text-close` and `@child-text`.
`register_refs` installs all of them, and it runs BEFORE the document is
installed, because the engine resolves names at install time. Adding a
reference to the grammar without adding it here fails at install with an
unresolved-name error, which is the behaviour you want.

## The shared node cell

`*rule.node.borrow_mut() = value` overwrites the PARENT's node too, as a
pushed rule shares the cell. `set_node` installs a fresh
`Rc<RefCell<Value>>` instead, and that is what to use for the canonical
`r.node = v`. The one deliberate exception is the `xml` close action,
which writes through the borrow because the start rule's cell IS the
document node. See `../../directive/rs/AGENTS.md` for the general form
of this hazard.

## Lexer state lives under `Context::u`

The matcher is imperative and carries no state of its own between
tokens. Depth, the DOCTYPE entity and attribute-default maps, and the
standalone flag all live under `Context::u`, spelled with the same keys
the canonical plugin uses (`xmlDepth`, `dtdEntities`, `dtdAttrDefaults`
and the rest) so a caller inspecting the context sees what it sees in
TypeScript. `MatcherState` holds only what is fixed when the plugin
installs: the entity decoder, the two entity flags and the minted token
identities.

## Three UTF-16 habits of the canonical plugin, reproduced here

TypeScript indexes UTF-16 code units and Rust indexes scalars, and three
places in `ts/src/xml.ts` depend on the difference. Each is reproduced
rather than corrected, because TypeScript is canonical, and each is
pinned by a test in `tests/xml_test.rs`.

1. **An entity declaration whose name starts outside the BMP declares
   nothing.** `parseDoctypeEntities` tests `charCodeAt`, so the leading
   surrogate of `<!ENTITY \u{1F600} "x">` is neither a NameStartChar nor
   a NameChar and the declaration is skipped; a later `&\u{1F600};` is
   then `undeclared_entity`. Every other name scanner in that file, the
   matcher's own and `readNameInBody`, tests `codePointAt` and admits the
   character, so element, attribute and `<!ATTLIST>` names take it. The
   inconsistency looks like an oversight rather than a decision, and XML
   1.0 [4] admits `#x10000-#xEFFFF`, so the declaration is well-formed
   and ought to be recorded. Repair it in TypeScript first, then here.
   `entity::read_declaration_name` is the narrow scanner;
   `entity::read_name` stays the full production for every other site.
   `an_astral_entity_declaration_declares_nothing` pins it. The Go port
   reads runes and records the declaration, so this cannot be a shared
   fixture row until Go is aligned too.

2. **`\s` and `\b` are the JavaScript classes, spelled out.** The
   `regex` crate reads `\s` as `\p{White_Space}`, which has U+0085 and
   has not U+FEFF, while ECMA-262 is the reverse; and it reads `\b` as a
   Unicode word boundary, while a JavaScript pattern without the `u`
   flag uses ASCII word characters. The NDATA, ExternalID and
   `standalone` patterns all run over DOCTYPE or XML-declaration text
   that the document supplies, so both differences are reachable and
   each changes a verdict. `entity::JS_SPACE` carries the class body and
   the `standalone` pattern asks for `(?-u:\b)`.
   `ported_patterns_use_the_javascript_character_classes` pins the rows.

3. **A byte-order mark costs no display column.** See the section below.

## A byte-order mark costs no display column

The mark is an encoding signature, not document content, so the other
two ports advance the source index past it and leave the column alone
(`pnt.sI = bomLen`, `pnt.SI = 3`). Those ports own the cursor. This one
hands the cursor to the engine, and `Lexer::advance_chars` charges a
column for every character it passes, so `lex::discount_bom` takes that
column back off the row the mark sat on, and `lib::check_doc_text`
applies it to the one token the plugin reports against but does not mint.

What it cannot reach is the engine's own `unexpected`. That is raised
against a token the engine minted, and at end of source (`#ZZ`) the
engine returns before any matcher runs, so `\u{FEFF}<a>` still reports
column 5 where TypeScript and Go report 4. Repairing that needs a way to
advance the engine's cursor without charging a column, which is a change
to the parser crate, not to this one.
`a_byte_order_mark_costs_no_column` pins both halves, so the day the
engine gains that the test says so.

## An unpaired surrogate becomes U+FFFF, not U+FFFD

`bom::not_a_scalar` carries the full reasoning. The short version: a
Rust `String` cannot hold a surrogate, and U+FFFD is a valid
`NameStartChar`, so folding to it turns eight ill-formed conformance
documents into well-formed ones. U+FFFF is excluded from `Char` and from
`NameChar` exactly as a surrogate is, so every XML check reaches the
same verdict. This is NOT the engine-wide "lone surrogates to U+FFFD"
rule in the parser's `DIVERGENCE.md`: that one governs a character VALUE
parsed out of a document, such as `&#xD800;`, where only the stored
value differs. This one governs SOURCE TEXT, where the fold would change
the verdict. `entity.rs` still folds `&#xD800;` to U+FFFD, and
`an_unpaired_surrogate_stays_invalid` pins the distinction.

## The conformance corpus must never skip

`tests/xmlconf_test.rs` fetches the pinned W3C snapshot through
`../scripts/fetch-xml-suite.sh` on first use. A corpus that is missing
and cannot be fetched PANICS, exactly as the Go and TypeScript graders
fail. Never turn that into a skip: a silent pass on an absent corpus is
the failure mode the floors exist to prevent.

The floors (`VALID_ACCEPT_FLOOR` 728, `VALID_CANONICAL_FLOOR` 232,
`NOT_WF_REJECT_FLOOR` 438, and the narrow `NOT_WF_SA_REJECT_FLOOR` 74)
are copied from `go/xmlconf_test.go` and the three runtimes currently
report an identical dial. Raise a floor when conformance genuinely
improves. Never lower one to make a red run green: a drop means the port
regressed, and the eight surrogate documents above are what that looks
like.

## The `opts` column builds a fresh parser per row

Fixture rows carry `# name`, `input`, `expected`, `opts` and an optional
`msg`. `parity_test.rs` reads columns by NAME, builds a fresh
`tabnas_jsonic::make()` per row and installs the plugin with the row's
`opts` as the plugin options, which is what Go's `UseDefaults` does.
`msg` pins a substring of the RENDERED message after ANSI codes are
stripped, so a template that stops interpolating fails here rather than
shipping. The input column has one extra escape beyond the shared
runner's, which is why `spec_unescape` decodes the raw cell and the
runner's own decoding is bypassed.

## Running it

```bash
cd rs
CARGO_TERM_COLOR=never cargo test --all-targets && cargo test --doc
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Or `make test-rs` from the repository root, and `ci/rust/run.sh` for
what CI would say. The corpus tests dominate the runtime; use
`cargo test --test parity_test` while iterating on the grammar.

## The README is doctested

`src/lib.rs` includes `../README.md` under `#[cfg(doctest)]`, so every
`rust` fence in it runs as a doctest and a stale example fails the gate.
Each fence must therefore be a COMPLETE program: wrap it in
`fn main() -> Result<(), Box<dyn std::error::Error>> { ... Ok(()) }`
rather than using `?` at the top level, and never use hidden `# ` lines,
which render as garbage on GitHub. `cargo test --doc` must list one
`readme_examples (line N)` entry per fence.
