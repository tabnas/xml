# tabnas-xml (Rust)

XML 1.0 grammar plugin for the
[`tabnas`](https://github.com/tabnas/parser) parsing engine, crate
`tabnas_xml`.

The plugin parses XML text into a tree of plain values: elements with a
name, a local name, an attribute map, and a list of children. It covers
attributes, mixed content, namespaces, entities (the five predefined
ones, numeric character references, caller-supplied entities and
`<!ENTITY>` declarations in the DOCTYPE internal subset), CDATA
sections, comments, processing instructions, `<!ATTLIST>` attribute
defaults, and `xml:space` and `xml:lang` inheritance.

It is layered on the relaxed-JSON grammar of
[`tabnas-jsonic`](https://github.com/tabnas/jsonic), as the canonical
plugin is layered on `@tabnas/jsonic`. In the default pure-XML mode the
XML rules replace the JSON value rules, so a document is XML and nothing
else. In embed mode the two sit side by side, so an XML element can
appear wherever a jsonic value can.

This is the Rust port of the canonical TypeScript implementation in
[`../ts`](../ts); the TypeScript version is authoritative and this crate
tracks it. The rule chain is authored once, in
[`../xml-grammar.jsonic`](../xml-grammar.jsonic), and every runtime
carries the same grammar.

## Use

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let value = tabnas_xml::parse(r#"<greeting lang="en">Hello</greeting>"#)?;
    println!("{value}");
    Ok(())
}
```

The shared default parser is built once and is safe to use from several
threads. To build an instance of your own, and reuse it across parses:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = tabnas_xml::make();
    let value = parser.parse("<doc><item>one</item><item>two</item></doc>")?;
    println!("{value}");
    Ok(())
}
```

Options are a typed struct rather than a loose bag, so a misspelled name
is a compile error:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = tabnas_xml::XmlOptions {
        strict_namespaces: true,
        ..tabnas_xml::XmlOptions::default()
    };
    let parser = tabnas_xml::make_with(&options);
    assert!(parser.parse("<a:doc/>").is_err());
    Ok(())
}
```

To install the plugin on an instance you already have, so XML sits
beside another grammar, use the engine's plugin entry point:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = tabnas_jsonic::make();
    let options = tabnas_xml::XmlOptions { embed: true, ..Default::default() };
    parser.use_plugin(tabnas_xml::plugin(), Some(options.to_value()))?;
    let value = parser.parse("{ doc: <a>text</a> }")?;
    println!("{value}");
    Ok(())
}
```

Files of unknown encoding go through `decode_bom`, which transcodes from
whichever of UTF-8, UTF-16 or UTF-32 the byte-order mark indicates:

```rust,no_run
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let body = tabnas_xml::decode_bom(&std::fs::read("doc.xml")?);
    let value = tabnas_xml::parse(&body)?;
    println!("{value}");
    Ok(())
}
```

## Install

The `tabnas` and `tabnas-jsonic` crates are not published to a registry,
so they are consumed as **sibling checkouts**, the standard tabnas
development model. Clone `https://github.com/tabnas/parser` and
`https://github.com/tabnas/jsonic` next to this repository and point at
them:

```toml
[dependencies]
tabnas-xml = { path = "../xml/rs" }
tabnas-jsonic = { path = "../jsonic/rs" }
tabnas = { path = "../parser/rs" }
```

All three entries are needed. A crate's dependencies are not passed on
to its dependents, so `tabnas-xml` alone does not put `tabnas` or
`tabnas-jsonic` in your extern prelude, and the examples above that name
them would not resolve. Only `XmlError` is re-exported.

## Differences from the canonical TypeScript

The parsed value, the option names and defaults, and the error codes are
the same. Six things differ. Five are forced by the language; the last is
a choice this port made, and it is marked as one:

- **Options are a struct, not a map.** `XmlOptions` has a field per
  option, with `Default` giving the canonical defaults. The plugin entry
  point still takes the engine's value bag, and `XmlOptions::from_value`
  and `to_value` convert, so a serialized configuration works as it does
  in the other two runtimes.
- **Error columns count characters.** A character outside the basic
  multilingual plane is one column here and two in TypeScript, which
  counts UTF-16 units. This is the engine-wide position rule, recorded
  in the parser's divergence register, and Go counts characters too.
- **An unpaired surrogate in the source becomes U+FFFF.** A Rust string
  holds Unicode scalar values only, so a surrogate cannot survive
  decoding. U+FFFF stands in for it because the two are treated alike by
  every XML rule: neither is a valid `Char` and neither is a valid
  `NameChar`. Folding it to U+FFFD would not be equivalent, because
  U+FFFD is a valid name character. Note what that does and does not
  buy: this parser checks character data for the illegal C0 controls and
  not for the whole of `Char`, so the substitute is refused in an
  element or attribute name and accepted in text, attribute values,
  comments, CDATA, processing instructions, and DTD declaration names.
  `decode_bom` followed by `parse` is not full `Char` validation; the
  doc comment on `decode_bom` carries the measured table.
- **Patterns carry no lookaround.** The `regex` crate does not support
  it, so the few places the TypeScript grammar uses a negative lookahead
  are written as a positive match plus an inversion in a check hook.
  That is the shape the Go port already uses.
- **Byte-order-mark handling is two functions.** The canonical
  `decodeBOM` takes either a byte sequence or a string, and decides which
  by looking at the code units it finds. Rust has the types for that
  question already: `decode_bom` takes bytes and transcodes, `strip_bom`
  takes text and removes a leading U+FEFF. A caller holding a Latin-1
  byte string in Rust holds a `&[u8]`, so the case the canonical
  function detects at run time does not arise.
- **The grammar text is public. (A CHOICE, not a constraint.)**
  `GRAMMAR_TEXT` is exported so a caller can inspect the installed rule
  chain, and the suite compares it with `../xml-grammar.jsonic`. The
  canonical plugin keeps its copy private and parses it at load time.
  Nothing in Rust required this; it is a wider public surface taken
  deliberately, and it could be withdrawn without changing any parse.

## Build and test

The engine and the jsonic base grammar are path dependencies on sibling
checkouts, so there is nothing to fetch:

```bash
cargo test --all-targets && cargo test --doc
```

Or, from the repository root, `make test-rs`. For what CI would say,
including formatting, clippy and the lockfile check, run
`ci/rust/run.sh`.

The suite runs the shared `../test/spec/*.tsv` conformance fixtures, the
same files the TypeScript and Go suites run, through the same shared
runner.

It also grades the W3C XML Conformance Test Suite, fetching the pinned
snapshot on first use through `../scripts/fetch-xml-suite.sh`. All three
runtimes reach the same verdict on every one of the 2284 documents in
scope: 728 of 729 valid documents parse, 232 of the 332 with a
catalogued canonical form serialize to it, all 229 merely DTD-invalid
documents are accepted as a non-validating parser must accept them, and
438 of 1326 not-well-formed documents are rejected. A corpus that cannot
be fetched fails the suite rather than skipping it.

## License

MIT.
