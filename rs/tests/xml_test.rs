// In-language behaviour the shared fixtures cannot express: the API
// surface, embed mode inside a jsonic document, byte-order-mark
// transcoding at scale, error columns after non-ASCII characters, the
// embedded grammar against its source file, instance reuse, threads, and
// the names a document controls kept as ordinary keys.
//
// Ports of ts/test/xml.test.ts (embedded XML, decodeBOM),
// ts/test/error-columns.test.ts and go/advance_col_test.go,
// ts/test/perf.test.ts and go/perf_test.go,
// ts/test/prototype-pollution.test.ts, and the rule-set assertions of
// ts/test/debug-model.test.ts.

mod common;

use std::fs;
use std::time::Instant;

use tabnas::{Tabnas, Value};
use tabnas_support::{equal_value, format_value};
use tabnas_xml::{decode_bom, make, make_with, parse, plugin, xml, XmlOptions, GRAMMAR_TEXT};

use common::{json, repo_root, to_value};

fn embed() -> Tabnas {
    make_with(&XmlOptions {
        embed: true,
        ..Default::default()
    })
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    match value {
        Value::Object(map) => map.get(key).unwrap_or(&Value::Undefined),
        _ => &Value::Undefined,
    }
}

fn item(value: &Value, index: usize) -> &Value {
    match value {
        Value::Array(items) => items.get(index).unwrap_or(&Value::Undefined),
        _ => &Value::Undefined,
    }
}

// ---------------------------------------------------------------------------
// The API
// ---------------------------------------------------------------------------

#[test]
fn the_default_parser_and_make_agree() {
    let src = r#"<greeting lang="en">Hi <b>world</b></greeting>"#;
    let want = r#"{"name":"greeting","localName":"greeting","attributes":{"lang":"en"},"children":["Hi ",{"name":"b","localName":"b","attributes":{},"children":["world"]}]}"#;
    assert_eq!(json(&parse(src).expect("parses")), want);
    assert_eq!(json(&make().parse(src).expect("parses")), want);
}

#[test]
fn the_plugin_layers_on_a_jsonic_instance_and_is_idempotent() {
    let mut parser = tabnas_jsonic::make();
    xml(&mut parser, &XmlOptions::default()).expect("installs");
    xml(&mut parser, &XmlOptions::default()).expect("installs again");
    assert_eq!(
        json(&parser.parse("<a/>").expect("parses")),
        r#"{"name":"a","localName":"a","attributes":{},"children":[]}"#
    );
    let mut used = tabnas_jsonic::make();
    used.use_plugin(plugin(), None).expect("installs");
    used.use_plugin(plugin(), None).expect("installs again");
    assert_eq!(
        json(&used.parse("<a>x</a>").expect("parses")),
        r#"{"name":"a","localName":"a","attributes":{},"children":["x"]}"#
    );
    // Rerunning the plugin must not double the alternates: a second root
    // is still refused, and the rule set is still the four XML rules.
    assert!(used.parse("<a/><b/>").is_err());
}

#[test]
fn pure_mode_carries_only_the_xml_rules_and_starts_at_xml() {
    // The rule set, the start rule and the plugin list that
    // ts/test/debug-model.test.ts reads off the debug model.
    let parser = make();
    let mut names = parser.rule_names();
    names.sort();
    assert_eq!(names, ["child", "content", "element", "xml"]);
    assert_eq!(parser.config().rule.start, "xml");
    assert!(parser
        .installed_plugins()
        .iter()
        .any(|plugin| plugin.name == "xml"));
    assert_eq!(
        parser
            .plugin_options("xml")
            .map(|options| options.to_json()["embed"].clone()),
        Some(serde_json::Value::Bool(false))
    );
}

#[test]
fn a_derived_instance_rebuilds_the_grammar() {
    // The plugin is installed through `use_plugin`, so an instance derived
    // from one carrying it re-runs the plugin against the new options
    // rather than inheriting a grammar-less engine.
    let parser = make();
    let derived = parser.derive(|_options| {}).expect("derives");
    assert_eq!(
        json(&derived.parse("<a>&lt;</a>").expect("parses")),
        r#"{"name":"a","localName":"a","attributes":{},"children":["<"]}"#
    );
}

#[test]
fn the_option_bag_is_read_field_by_field() {
    let bag = Value::from_json(&serde_json::json!({
        "namespaces": false,
        "entities": "no",
        "customEntities": { "nbsp": "\u{a0}", "n": 1 },
        "strictEntities": false,
        "strictNamespaces": true,
        "embed": "yes",
    }));
    let options = XmlOptions::from_value(&bag);
    assert!(!options.namespaces);
    assert!(options.entities, "a non-boolean leaves the default");
    assert_eq!(options.custom_entities["nbsp"], "\u{a0}");
    assert_eq!(options.custom_entities["n"], "1");
    assert!(!options.strict_entities);
    assert!(options.strict_namespaces);
    assert!(!options.embed, "embed is on only when exactly true");

    let round = XmlOptions::from_value(&XmlOptions::default().to_value());
    assert_eq!(round, XmlOptions::default());
}

#[test]
fn errors_carry_the_code_the_position_and_the_rendered_message() {
    let error = parse("<a>\n  <b></c>\n</a>").unwrap_err();
    assert_eq!(error.code, "xml_mismatched_tag");
    assert_eq!((error.row, error.col), (2, 6));
    let report = error.to_string();
    assert!(
        report.contains("closing tag </c> does not match opening tag <b>"),
        "{report}"
    );
    assert!(report.contains("Expected </b> but found </c>."), "{report}");
    assert!(report.contains("[jsonic/xml_mismatched_tag]"), "{report}");
}

// ---------------------------------------------------------------------------
// XML embedded in jsonic source
//
// With `embed: true` the plugin extends jsonic's own grammar so a literal
// XML element can appear anywhere a jsonic value is expected. The outer
// document is parsed by jsonic; the XML subtree is built by the plugin's
// element grammar.
// ---------------------------------------------------------------------------

#[test]
fn plain_jsonic_is_unaffected_by_embed_mode() {
    let parser = embed();
    assert_eq!(
        json(&parser.parse("{a:1, b:\"two\"}").expect("parses")),
        r#"{"a":1.0,"b":"two"}"#
    );
    assert_eq!(
        json(&parser.parse("[1, 2, 3]").expect("parses")),
        "[1.0,2.0,3.0]"
    );
}

#[test]
fn xml_literal_as_the_top_level_value() {
    let parser = embed();
    assert_eq!(
        json(&parser.parse("<a>hello</a>").expect("parses")),
        r#"{"name":"a","localName":"a","attributes":{},"children":["hello"]}"#
    );
    assert_eq!(
        json(&parser.parse("<br/>").expect("parses")),
        r#"{"name":"br","localName":"br","attributes":{},"children":[]}"#
    );
}

#[test]
fn xml_literal_as_a_value_inside_a_jsonic_map() {
    let parser = embed();
    let src = "{\n  title: \"order-42\",\n  payload: <order id=\"42\">\n    <item qty=\"2\">Widget</item>\n    <item qty=\"1\">Gadget</item>\n  </order>,\n}";
    let result = parser.parse(src).expect("parses");
    assert_eq!(field(&result, "title"), &Value::String("order-42".into()));
    let payload = field(&result, "payload");
    assert_eq!(field(payload, "name"), &Value::String("order".into()));
    assert_eq!(
        field(field(payload, "attributes"), "id"),
        &Value::String("42".into())
    );
    let Value::Array(children) = field(payload, "children") else {
        panic!("children is an array")
    };
    let items: Vec<&Value> = children
        .iter()
        .filter(|child| field(child, "name") == &Value::String("item".into()))
        .collect();
    assert_eq!(items.len(), 2);
    assert_eq!(
        field(field(items[0], "attributes"), "qty"),
        &Value::String("2".into())
    );
    assert_eq!(
        item(field(items[0], "children"), 0),
        &Value::String("Widget".into())
    );
    assert_eq!(
        field(field(items[1], "attributes"), "qty"),
        &Value::String("1".into())
    );
    assert_eq!(
        item(field(items[1], "children"), 0),
        &Value::String("Gadget".into())
    );
}

#[test]
fn xml_literal_preserves_comma_and_colon_in_text() {
    // Without the matcher claiming the text run while an element is open,
    // jsonic's lexer would split this on the comma and reject it.
    let parser = embed();
    assert_eq!(
        json(&parser.parse("<a>Hello, World!</a>").expect("parses")),
        r#"{"name":"a","localName":"a","attributes":{},"children":["Hello, World!"]}"#
    );
    assert_eq!(
        json(&parser.parse("<a>key: value</a>").expect("parses")),
        r#"{"name":"a","localName":"a","attributes":{},"children":["key: value"]}"#
    );
}

#[test]
fn multiple_xml_literals_inside_a_jsonic_list() {
    let result = embed()
        .parse("[<a/>, <b>x</b>, <c x=\"1\"/>]")
        .expect("parses");
    let Value::Array(items) = &result else {
        panic!("a list, got {result:?}")
    };
    assert_eq!(items.len(), 3);
    assert_eq!(field(&items[0], "name"), &Value::String("a".into()));
    assert_eq!(field(&items[1], "name"), &Value::String("b".into()));
    assert_eq!(json(field(&items[1], "children")), r#"["x"]"#);
    assert_eq!(
        field(field(&items[2], "attributes"), "x"),
        &Value::String("1".into())
    );
}

#[test]
fn xml_literal_with_namespaces_resolves_in_embed_mode() {
    let result = embed()
        .parse("{doc: <root xmlns=\"http://e.example\"><child/></root>}")
        .expect("parses");
    let doc = field(&result, "doc");
    assert_eq!(
        field(doc, "namespace"),
        &Value::String("http://e.example".into())
    );
    assert_eq!(
        field(item(field(doc, "children"), 0), "namespace"),
        &Value::String("http://e.example".into())
    );
}

#[test]
fn embed_mode_accepts_no_document_element_as_a_fragment_would() {
    // Standalone mode makes "no document element" an error; a fragment
    // inside a jsonic document has no document element of its own.
    assert!(make().parse("<!-- only a comment -->").is_err());
    assert!(make().parse("").is_err());
    assert!(embed().parse("<!-- only a comment -->").is_ok());
}

// ---------------------------------------------------------------------------
// decode_bom at scale
// ---------------------------------------------------------------------------

#[test]
fn large_utf16_documents_decode_and_parse() {
    // The W3C suite's japanese/pr-xml-utf-16.xml is such a document, and
    // the canonical decoder once overflowed on it.
    let text = format!("<doc>{}</doc>", "あ".repeat(200_000));
    for big in [true, false] {
        let mut bytes: Vec<u8> = if big {
            vec![0xfe, 0xff]
        } else {
            vec![0xff, 0xfe]
        };
        for unit in text.encode_utf16() {
            let [high, low] = unit.to_be_bytes();
            if big {
                bytes.extend([high, low]);
            } else {
                bytes.extend([low, high]);
            }
        }
        let decoded = decode_bom(&bytes);
        assert_eq!(decoded, text);
        let element = make().parse(&decoded).expect("parses");
        assert_eq!(field(&element, "name"), &Value::String("doc".into()));
        let Value::String(body) = item(field(&element, "children"), 0) else {
            panic!("a text child")
        };
        assert_eq!(body.chars().count(), 200_000);
    }
}

// ---------------------------------------------------------------------------
// Error columns after a non-ASCII character
//
// This plugin brings its own matcher, and a plugin that does owns the
// column arithmetic. The engine's lexer counts characters as it advances,
// so a 2-byte `é`, a 3-byte `€` and a 4-byte astral character each cost
// one column, as in Go. TypeScript counts UTF-16 units, so the astral row
// is the one where the answers differ: the recorded engine divergence
// (parser/DIVERGENCE.md, "Column positions for astral characters").
// ---------------------------------------------------------------------------

#[test]
fn error_columns_count_characters_not_bytes() {
    for (label, src, col, ts) in [
        // Control: pure ASCII, where bytes and characters coincide.
        ("ascii", "<a>xx</a><", 10, 10),
        // 2 and 3 bytes, 1 character, 1 UTF-16 unit: every port agrees.
        ("latin1", "<a>\u{e9}</a><", 9, 9),
        ("bmp", "<a>\u{20ac}</a><", 9, 9),
        // 4 bytes, 1 character, TWO UTF-16 units: the recorded divergence.
        ("astral", "<a>\u{1F600}</a><", 9, 10),
    ] {
        let error = make()
            .parse(src)
            .expect_err(&format!("{label}: {src:?} parsed, expected a diagnostic"));
        assert_eq!(
            error.col, col,
            "{label}: {src:?} col = {}, want {col} (TypeScript says {ts})",
            error.col
        );
    }
}

// ---------------------------------------------------------------------------
// The embedded grammar is the grammar file
// ---------------------------------------------------------------------------

#[test]
fn the_embedded_grammar_matches_xml_grammar_jsonic() {
    // xml-grammar.jsonic is authored in jsonic and parsed at load time by
    // the TypeScript plugin; this crate embeds the parsed form. The two
    // are held to the same value here, through the jsonic port, so an
    // edit to the file that is not carried into `GRAMMAR_TEXT` fails.
    let path = repo_root().join("xml-grammar.jsonic");
    let source =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let from_file = to_value(&tabnas_jsonic::parse(&source).expect("the grammar file parses"));
    let embedded: serde_json::Value =
        serde_json::from_str(GRAMMAR_TEXT).expect("the embedded grammar is JSON");
    let embedded = tabnas_support::Value::from(embedded);
    assert!(
        equal_value(&from_file, &embedded),
        "rs/src/lib.rs GRAMMAR_TEXT differs from xml-grammar.jsonic\n  file:     {}\n  embedded: {}",
        format_value(&from_file),
        format_value(&embedded)
    );
}

// ---------------------------------------------------------------------------
// Names the document controls stay ordinary keys
//
// The canonical plugin allocates every map keyed by a name the document
// controls without a prototype, so `__proto__` is a key like any other.
// A Rust map has no prototype to pollute; these pin the visible half of
// that contract, the same documents parsing the same way.
// ---------------------------------------------------------------------------

#[test]
fn document_controlled_names_are_ordinary_keys() {
    let out = parse("<!DOCTYPE d [<!ATTLIST __proto__ a CDATA \"def\">]><d><__proto__/></d>")
        .expect("parses");
    assert_eq!(
        json(field(item(field(&out, "children"), 0), "attributes")),
        r#"{"a":"def"}"#
    );
    let out =
        parse("<!DOCTYPE d [<!ENTITY __proto__ \"PWNED\">]><d>&__proto__;</d>").expect("parses");
    assert_eq!(json(field(&out, "children")), r#"["PWNED"]"#);
    assert!(parse("<d>&toString;</d>").is_err());
    let out = parse("<d __proto__=\"1\" b=\"2\"/>").expect("parses");
    assert_eq!(
        json(field(&out, "attributes")),
        r#"{"__proto__":"1","b":"2"}"#
    );
    let out = parse("<d xmlns:__proto__=\"urn:x\"><__proto__:e/></d>").expect("parses");
    assert_eq!(
        field(item(field(&out, "children"), 0), "namespace"),
        &Value::String("urn:x".into())
    );
}

// ---------------------------------------------------------------------------
// Instance reuse and threads
// ---------------------------------------------------------------------------

#[test]
fn reusing_one_parser_is_far_faster_than_rebuilding_it_per_parse() {
    // Guards against a regression where callers rebuild the (expensive)
    // XML parser and grammar on every parse instead of building one
    // instance and reusing it. Building the grammar dominates a parse, so
    // the rebuild-per-call path is many times slower. The check is
    // machine-independent: both sides run on the same machine in the same
    // process, with no wall-clock budget. Mirrors go/perf_test.go and
    // ts/test/perf.test.ts.
    const SRC: &str = r#"<a x="1"><b>hello</b><c/></a>"#;
    const N: usize = 300;

    let reused = make();
    for _ in 0..20 {
        reused.parse(SRC).expect("warm reuse");
        make().parse(SRC).expect("warm rebuild");
    }

    let started = Instant::now();
    for _ in 0..N {
        reused.parse(SRC).expect("reuse parse");
    }
    let reuse = started.elapsed();

    let started = Instant::now();
    for _ in 0..N {
        make().parse(SRC).expect("rebuild parse");
    }
    let rebuild = started.elapsed();

    assert!(
        reuse * 4 < rebuild,
        "instance reuse is not meaningfully faster than rebuilding the parser per parse: \
         {N} reuse parses took {reuse:?} vs {rebuild:?} rebuilding per call \
         (ratio {:.1}x, need >=4x). Build one parser (tabnas_xml::make()) and reuse it.",
        rebuild.as_secs_f64() / reuse.as_secs_f64()
    );
}

#[test]
fn the_shared_default_parser_is_safe_across_threads() {
    // `parse` reuses one instance behind a OnceLock, as the Go port's
    // sync.Once does. Failing parses are interleaved with succeeding ones
    // on purpose: state leaking across calls would surface as a wrong
    // value or a spurious error here.
    let threads: Vec<_> = (0..8)
        .map(|n| {
            std::thread::spawn(move || {
                let src = format!("<r n=\"{n}\"><i>{n}</i>, <j/></r>");
                for _ in 0..50 {
                    let value = parse(&src).expect("parses");
                    assert_eq!(
                        field(field(&value, "attributes"), "n"),
                        &Value::String(n.to_string())
                    );
                    assert_eq!(
                        item(field(item(field(&value, "children"), 0), "children"), 0),
                        &Value::String(n.to_string())
                    );
                    assert_eq!(parse("<a></b>").unwrap_err().code, "xml_mismatched_tag");
                    assert_eq!(parse("<a/>x").unwrap_err().code, "text_at_top_level");
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().expect("no thread panicked");
    }
}
