// The shared conformance fixtures, every one of them.
//
// `test/spec/*.tsv` is the parity contract: the TypeScript suite
// (`ts/test/xml-spec.test.ts`), the Go suite (`go/xml_test.go`) and this
// file run the same rows through the shared runner. Files are discovered
// by listing, exactly as the other two runners discover them, so adding a
// fixture runs it in all three runtimes without touching any runner. A
// row green in one runtime and red in another is a failure, not a
// discrepancy.
//
// A row is `# name<TAB>input<TAB>expected<TAB>opts<TAB>msg`. The header
// line begins with `#`, which is why the columns are read by NAME and why
// the first one is called `# name`. What is left here is only what is
// specific to xml: one extra escape, the `msg` column, and how to build
// the parser for a row's options.

mod common;

use std::fs;

use tabnas::Value;
use tabnas_support::{Failure, Runner};

use common::{spec_dir, spec_unescape, strip_ansi, to_failure, to_value};

fn runner() -> Runner {
    Runner::new_with_row(|_input, row| {
        // The runner's own decoding of the input column is bypassed (see
        // `spec_unescape`), so the raw cell is read and decoded here.
        let input = spec_unescape(row.named("input"));
        let opts = row.named("opts");

        // A FRESH parser per row, with the row's options as the plugin
        // options, as the Go runner builds one with `UseDefaults`.
        let mut parser = tabnas_jsonic::make();
        let options = if opts.trim().is_empty() {
            None
        } else {
            let opts: serde_json::Value = serde_json::from_str(opts)
                .map_err(|error| Failure::message(format!("opts column is not JSON: {error}")))?;
            Some(Value::from_json(&opts))
        };
        parser
            .use_plugin(tabnas_xml::plugin(), options)
            .map_err(|error| Failure::message(error.to_string()))?;
        parser
            .parse(&input)
            .map(|value| to_value(&value))
            .map_err(to_failure)
    })
    // Two things the default code comparison does not do. The engine
    // renders a code as `<tag>/<code>`, so the code and the rendered
    // message are both consulted; and the optional `msg` column pins the
    // rendered message, so a template that stops interpolating, leaving
    // a literal placeholder behind, fails here rather than shipping.
    .match_error(|failure, want, row| {
        let named = failure.code == want || failure.message.contains(&format!("/{want}"));
        let msg = row.named("msg");
        named && (msg.is_empty() || strip_ansi(&failure.message).contains(msg))
    })
    .input("input")
    .expected("expected")
}

#[test]
fn every_shared_fixture() {
    runner().dir(spec_dir());
}

/// The runner reads the row by column NAME, so every fixture must carry
/// the named columns it reads; a file that does not would be run against
/// the wrong cells rather than refused. This is the tripwire the other
/// two runners get from their loaders.
#[test]
fn every_fixture_has_the_named_columns() {
    let mut names: Vec<String> = fs::read_dir(spec_dir())
        .expect("the spec directory lists")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tsv"))
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "no fixtures under {}",
        spec_dir().display()
    );
    for name in &names {
        let path = spec_dir().join(name);
        let body = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
        let header: Vec<&str> = body
            .lines()
            .next()
            .unwrap_or_default()
            .split('\t')
            .collect();
        for column in ["# name", "input", "expected"] {
            assert!(
                header.contains(&column),
                "{name}: header {header:?} has no {column:?} column"
            );
        }
    }
}
