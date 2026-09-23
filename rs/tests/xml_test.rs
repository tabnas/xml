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

use common::{json, repo_root, strip_ansi, to_value};

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

    // The push edges the same TypeScript test reads off the debug model:
    // `xml` pushes `element` and `content` pushes `child`. The engine
    // carries them on the rule specs, so no debug plugin is needed to see
    // them. Every fixture exercises the chain, but nothing else names it,
    // and a grammar edit that reroutes a push while still parsing the
    // corpus would go unremarked.
    for (rule, pushed) in [("xml", "element"), ("content", "child")] {
        let spec = parser
            .rule_specs()
            .into_iter()
            .find(|spec| spec.name == rule)
            .unwrap_or_else(|| panic!("the grammar carries a `{rule}` rule"));
        let pushes =
            |alts: &[tabnas::AltSpec]| alts.iter().any(|alt| alt.p.as_deref() == Some(pushed));
        // `open` ALONE, as the TypeScript test this mirrors checks. A
        // push moved to `close` still parses the corpus and still
        // satisfied an `open || close` assertion, while the rule chain
        // no longer matched the canonical phase -- which is the whole
        // property this case exists to pin.
        assert!(
            pushes(&spec.open),
            "`{rule}` does not push `{pushed}` from an OPEN alternative"
        );
    }
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

/// Namespace resolution runs at DOCUMENT CLOSE, after the last token has
/// been consumed, so there is no current token to mark. The canonical
/// marks its no-token sentinel there and reports at the start of the
/// source; a port that raises a bare action error instead leaves the
/// engine's template machinery unused and hands the caller a literal
/// stand-in message. Each of the three namespace codes is pinned by its
/// FULL rendered text, its hint and its position: pinning the code alone
/// would pass against the stand-in.
///
/// Measured against `ts/src/xml.ts` through `ts/dist/xml.js`: every case
/// below reports at row 1, column 1, including the multi-line one whose
/// offending element sits on row 3.
#[test]
fn a_namespace_failure_renders_its_template_at_the_start_of_the_source() {
    let strict = make_with(&XmlOptions {
        strict_namespaces: true,
        ..Default::default()
    });
    // The default bag is lenient about an unbound prefix, but a reserved
    // prefix and a namespace name with white space are refused either way.
    let lenient = make();

    // (parser, source, code, message, hint)
    let cases: [(&Tabnas, &str, &str, &str, &str); 4] = [
        (
            &strict,
            "<a><foo:b/></a>",
            "unbound_prefix",
            "element or attribute uses an undeclared namespace prefix",
            "Declare the prefix with xmlns:prefix=\"...\" on this element or one of its ancestors.",
        ),
        // The offending element is on row 3; the report still lands on
        // row 1, column 1, as the canonical's does.
        (
            &strict,
            "<a>\n  <b>\n    <foo:c/>\n  </b>\n</a>",
            "unbound_prefix",
            "element or attribute uses an undeclared namespace prefix",
            "Declare the prefix with xmlns:prefix=\"...\" on this element or one of its ancestors.",
        ),
        (
            &lenient,
            "<a>\n  <b xmlns:xml=\"http://wrong\"/>\n</a>",
            "reserved_namespace",
            "invalid use of a reserved namespace prefix or URI",
            "The \"xml\" prefix is fixed to http://www.w3.org/XML/1998/namespace;",
        ),
        (
            &lenient,
            "<a>\n  <b xmlns=\"http://x y\"/>\n</a>",
            "invalid_namespace_uri",
            "namespace name cannot contain white space",
            "A namespace name is a URI reference, and a URI reference cannot contain white space;",
        ),
    ];

    for (parser, source, code, message, hint) in cases {
        let error = parser.parse(source).unwrap_err();
        assert_eq!(error.code, code, "{source:?}");
        assert_eq!((error.row, error.col), (1, 1), "{source:?}");

        let report = strip_ansi(&error.to_string());
        let first = report.lines().next().unwrap_or_default();
        assert_eq!(first, format!("[jsonic/{code}]: {message}"), "{report}");
        assert!(report.contains(hint), "{report}");
        // The caret line repeats the message, so the stand-in must be
        // absent from the whole report, not merely from its first line.
        assert!(!report.contains("namespace resolution failed"), "{report}");
    }
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
// A byte-order mark costs no column
//
// The mark is an encoding signature, not document content, so it occupies
// no display column: `ts/src/xml.ts` advances `pnt.sI` alone and
// `go/xml.go` advances `pnt.SI` alone. This port hands the cursor to the
// engine, which charges a column for every character it passes, so the
// matcher gives that one back (`discount_bom` in `src/lex.rs`).
//
// Every column below was measured against the canonical implementation
// (@tabnas/xml 0.7.7, whose published `src/xml.ts` is byte for byte the
// file in `ts/src`) and against the Go port; the two agree on all of them.
// ---------------------------------------------------------------------------

#[test]
fn a_byte_order_mark_costs_no_column() {
    // Each pair is the same document with and without the mark, and the
    // same column is wanted for both.
    for (label, src, code, col) in [
        ("mismatched", "<a></b>", "xml_mismatched_tag", 4),
        ("mismatched-bom", "\u{FEFF}<a></b>", "xml_mismatched_tag", 4),
        ("top-level-text", "<a/>junk", "text_at_top_level", 5),
        (
            "top-level-text-bom",
            "\u{FEFF}<a/>junk",
            "text_at_top_level",
            5,
        ),
        ("bad-entity", "<a>&#X26;</a>", "bad_entity_ref", 4),
        (
            "bad-entity-bom",
            "\u{FEFF}<a>&#X26;</a>",
            "bad_entity_ref",
            4,
        ),
        ("unterminated", "<a><!-- x</a>", "unterminated_comment", 4),
        (
            "unterminated-bom",
            "\u{FEFF}<a><!-- x</a>",
            "unterminated_comment",
            4,
        ),
        ("attr-lt", "<a b=\"<\"/>", "lt_in_attr_value", 1),
        ("attr-lt-bom", "\u{FEFF}<a b=\"<\"/>", "lt_in_attr_value", 1),
    ] {
        let error = make()
            .parse(src)
            .expect_err(&format!("{label}: {src:?} parsed, expected a diagnostic"));
        assert_eq!(error.code, code, "{label}: {src:?}");
        assert_eq!(
            (error.row, error.col),
            (1, col),
            "{label}: {src:?} reported {}:{}, want 1:{col}. A column one to the \
             right of the want means the byte-order mark was charged a display \
             column; TypeScript and Go charge it none.",
            error.row,
            error.col
        );
    }

    // The mark also leaves the row alone, and a column on a later row was
    // never affected: the engine resets it at the line ending.
    for src in ["<a>\n  <b></c>\n</a>", "\u{FEFF}<a>\n  <b></c>\n</a>"] {
        let error = make().parse(src).expect_err("a diagnostic");
        assert_eq!((error.row, error.col), (2, 6), "{src:?}");
    }

    // The one diagnostic still a column to the right, and why: `unexpected`
    // is raised by the ENGINE against a token the engine minted, and at end
    // of source (`#ZZ`) no matcher runs at all, so `discount_bom` never
    // sees it. TypeScript and Go both say 4 here, because there the plugin
    // owns the cursor and never charged the column in the first place.
    // Repairing it means giving the engine a way to advance without
    // charging a column; until then this is the measured difference.
    let error = make().parse("\u{FEFF}<a>").expect_err("a diagnostic");
    assert_eq!(error.code, "unexpected");
    assert_eq!((error.row, error.col), (1, 5));
    let error = make().parse("<a>").expect_err("a diagnostic");
    assert_eq!((error.row, error.col), (1, 4));
}

// ---------------------------------------------------------------------------
// A `customEntities` replacement is coerced as JavaScript coerces it
//
// The canonical decoder returns the replacement straight out of a
// `String.prototype.replace` callback, so JavaScript's `ToString` runs on
// whatever the option bag held. Every want below was measured through
// @tabnas/xml 0.7.7.
// ---------------------------------------------------------------------------

/// A parser whose `customEntities` maps `x` to `replacement`, installed
/// through the loose option bag `plugin` takes, which is the only way a
/// non-string replacement can arrive: [`XmlOptions::custom_entities`] is
/// typed as strings.
fn parser_with_custom_x(replacement: Value) -> Tabnas {
    let mut options = Value::from_json(&serde_json::json!({"customEntities": {"x": null}}));
    options
        .as_object_mut()
        .and_then(|bag| bag.get_mut("customEntities"))
        .and_then(Value::as_object_mut)
        .expect("the bag carries a customEntities object")
        .insert("x".to_string(), replacement);
    let mut parser = tabnas_jsonic::make();
    parser
        .use_plugin(plugin(), Some(options))
        .expect("the plugin installs");
    parser
}

#[test]
fn custom_entity_replacements_coerce_as_javascript_does() {
    let number = |n: f64| Value::Number(n);
    for (label, replacement, want) in [
        // An object has no `toString` of its own, so it is the bare tag.
        (
            "object",
            Value::from_json(&serde_json::json!({"a": 1})),
            "[object Object]",
        ),
        // `Array.prototype.toString` joins with commas, and a nested
        // array joins in turn.
        ("array", Value::from_json(&serde_json::json!([1, 2])), "1,2"),
        (
            "nested-array",
            Value::from_json(&serde_json::json!([[1, 2], [3]])),
            "1,2,3",
        ),
        // `join` renders null and undefined as nothing at all.
        (
            "array-holes",
            Value::array(vec![Value::Null, Value::Undefined, number(1.0)]),
            ",,1",
        ),
        ("null", Value::Null, "null"),
        ("true", Value::Bool(true), "true"),
        ("string", Value::String("plain".into()), "plain"),
        // `Number::toString`, which is not Rust's `f64` formatting: no
        // sign on zero, exponent form outside `(-6, 21]`, and the
        // shortest round-tripping digits.
        ("integer", number(7.0), "7"),
        ("fraction", number(1.5), "1.5"),
        ("negative-zero", number(-0.0), "0"),
        ("exponent-high", number(1e21), "1e+21"),
        ("exponent-low", number(1e-7), "1e-7"),
        ("not-a-number", number(f64::NAN), "NaN"),
        ("infinity", number(f64::INFINITY), "Infinity"),
        ("negative-infinity", number(f64::NEG_INFINITY), "-Infinity"),
    ] {
        let value = parser_with_custom_x(replacement)
            .parse("<a>&x;</a>")
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_eq!(
            item(field(&value, "children"), 0),
            &Value::String(want.to_string()),
            "{label}"
        );
    }

    // `undefined` is the one replacement the canonical callback never
    // coerces: `undefined !== baseEntities[ref]` fails, so the name is
    // declared (no `undeclared_entity`) and the reference is left in the
    // text exactly as written.
    let value = parser_with_custom_x(Value::Undefined)
        .parse("<a>&x;</a>")
        .expect("an undefined replacement is still a declaration");
    assert_eq!(
        item(field(&value, "children"), 0),
        &Value::String("&x;".to_string())
    );
}

// ---------------------------------------------------------------------------
// An astral entity declaration declares nothing
//
// `parseDoctypeEntities` in `ts/src/xml.ts` is the one name scanner of the
// canonical plugin that reads UTF-16 code units (`charCodeAt`), and a
// surrogate is neither a NameStartChar nor a NameChar, so a declaration
// whose name starts outside the BMP is skipped. Everything else in the
// canonical plugin reads code points and admits the same character. That
// looks like an oversight rather than a decision, and XML 1.0 [4] admits
// `#x10000-#xEFFFF`, but TypeScript is canonical: see `src/entity.rs`,
// `read_declaration_name`, and `AGENTS.md`.
//
// The Go port records the declaration (it reads runes), so this cannot be
// a shared fixture row: it is a Go defect of the same shape, to be fixed
// there.
// ---------------------------------------------------------------------------

#[test]
fn an_astral_entity_declaration_declares_nothing() {
    // Skipped, so the reference to it is undeclared.
    let error = parse("<!DOCTYPE a [<!ENTITY \u{1F600} \"x\">]><a>&\u{1F600};</a>")
        .expect_err("the declaration is skipped, so the reference is undeclared");
    assert_eq!(error.code, "undeclared_entity");

    // A name that merely CONTAINS an astral character stops at it, so the
    // declaration is malformed and nothing is recorded either.
    let error = parse("<!DOCTYPE a [<!ENTITY a\u{1F600}b \"x\">]><a>&a;</a>")
        .expect_err("the name stops at the astral character");
    assert_eq!(error.code, "undeclared_entity");

    // The scan resumes after the skipped declaration: a later one is
    // still found.
    let value = parse("<!DOCTYPE a [<!ENTITY \u{1F600} \"x\"><!ENTITY e \"v\">]><a>&e;</a>")
        .expect("the ASCII declaration is still mined out");
    assert_eq!(json(field(&value, "children")), r#"["v"]"#);

    // A BMP name in the same position IS a declaration. The reference is
    // left verbatim all the same: the decoder's reference pattern is
    // ASCII in every port.
    let value = parse("<!DOCTYPE a [<!ENTITY \u{4e2d} \"x\">]><a>&\u{4e2d};</a>")
        .expect("a BMP name declares the entity");
    assert_eq!(json(field(&value, "children")), "[\"&\u{4e2d};\"]");

    // Element, attribute and `<!ATTLIST>` names are read by the
    // code-point scanner and still admit an astral character.
    let value = parse("<\u{1F600} a=\"1\"/>").expect("an astral element name parses");
    assert_eq!(field(&value, "name"), &Value::String("\u{1F600}".into()));
    let value = parse("<e \u{1F600}=\"1\"/>").expect("an astral attribute name parses");
    assert_eq!(json(field(&value, "attributes")), "{\"\u{1F600}\":\"1\"}");
    let value = parse("<!DOCTYPE d [<!ATTLIST d \u{1F600} CDATA \"v\">]><d/>")
        .expect("an astral ATTLIST name parses");
    assert_eq!(json(field(&value, "attributes")), "{\"\u{1F600}\":\"v\"}");
}

// ---------------------------------------------------------------------------
// The ported patterns use the JavaScript character classes
//
// Three of this crate's five patterns came from JavaScript regular
// expressions that spell `\s` or `\b`, and the `regex` crate reads both
// as Unicode: `\s` is `\p{White_Space}`, which HAS U+0085 and has NOT
// U+FEFF, the reverse of the ECMA-262 class; and `\b` is a Unicode word
// boundary, while a JavaScript pattern without the `u` flag uses ASCII
// word characters. All three patterns run over text a DOCTYPE or an XML
// declaration supplies, so every difference below is reachable, and each
// one changes a verdict rather than a position.
//
// Every want was measured through @tabnas/xml 0.7.7. The Go port spells
// the same patterns in RE2, whose `\s` is ASCII-only, so it answers
// differently again on the U+00A0 and U+FEFF rows: these cannot become
// shared fixture rows until that is repaired there.
// ---------------------------------------------------------------------------

#[test]
fn ported_patterns_use_the_javascript_character_classes() {
    // `None` is a clean parse; `Some(code)` the diagnostic wanted.
    for (label, src, want) in [
        // `(^|[\s"'])NDATA([\s"']|$)`: the separator before the NDATA
        // notation of an unparsed entity declaration.
        (
            "ndata-space",
            "<!DOCTYPE d [<!ENTITY e SYSTEM \"u\" NDATA n>]><d>&e;</d>",
            Some("unparsed_entity_ref"),
        ),
        (
            "ndata-nel",
            "<!DOCTYPE d [<!ENTITY e SYSTEM \"u\"\u{85}NDATA n>]><d>&e;</d>",
            None,
        ),
        (
            "ndata-bom",
            "<!DOCTYPE d [<!ENTITY e SYSTEM \"u\"\u{feff}NDATA n>]><d>&e;</d>",
            Some("unparsed_entity_ref"),
        ),
        (
            "ndata-nbsp",
            "<!DOCTYPE d [<!ENTITY e SYSTEM \"u\"\u{a0}NDATA n>]><d>&e;</d>",
            Some("unparsed_entity_ref"),
        ),
        // `(^|[\s>])(SYSTEM|PUBLIC)([\s"'])`: an external subset the
        // processor never reads suspends the "Entity Declared" rule, so
        // matching or not decides whether `&z;` is an error.
        (
            "external-id-space",
            "<!DOCTYPE d SYSTEM \"u\"><d>&z;</d>",
            None,
        ),
        (
            "external-id-nel",
            "<!DOCTYPE d\u{85}SYSTEM \"u\"><d>&z;</d>",
            Some("undeclared_entity"),
        ),
        (
            "external-id-bom",
            "<!DOCTYPE d\u{feff}SYSTEM \"u\"><d>&z;</d>",
            None,
        ),
        (
            "external-id-nbsp",
            "<!DOCTYPE d\u{a0}SYSTEM \"u\"><d>&z;</d>",
            None,
        ),
        (
            "external-id-tail-bom",
            "<!DOCTYPE d SYSTEM\u{feff}\"u\"><d>&z;</d>",
            None,
        ),
        (
            "external-id-tail-nel",
            "<!DOCTYPE d SYSTEM\u{85}\"u\"><d>&z;</d>",
            Some("undeclared_entity"),
        ),
        // `\bstandalone\s*=\s*("yes"|'yes')`: `standalone="yes"` puts the
        // "Entity Declared" rule back, so a match makes `&z;` an error.
        (
            "standalone-plain",
            "<?xml standalone=\"yes\"?><!DOCTYPE d SYSTEM \"u\"><d>&z;</d>",
            Some("undeclared_entity"),
        ),
        (
            "standalone-no-boundary",
            "<?xml astandalone=\"yes\"?><!DOCTYPE d SYSTEM \"u\"><d>&z;</d>",
            None,
        ),
        // The boundary row: `e` then `standalone` is a word boundary in
        // JavaScript, and inside one word to a Unicode `\b`.
        (
            "standalone-boundary-after-latin",
            "<?xml a\u{e9}standalone=\"yes\"?><!DOCTYPE d SYSTEM \"u\"><d>&z;</d>",
            Some("undeclared_entity"),
        ),
        (
            "standalone-equals-bom",
            "<?xml standalone\u{feff}=\u{feff}\"yes\"?><!DOCTYPE d SYSTEM \"u\"><d>&z;</d>",
            Some("undeclared_entity"),
        ),
        (
            "standalone-equals-nel",
            "<?xml standalone\u{85}=\"yes\"?><!DOCTYPE d SYSTEM \"u\"><d>&z;</d>",
            None,
        ),
        (
            "standalone-equals-nbsp",
            "<?xml standalone\u{a0}=\"yes\"?><!DOCTYPE d SYSTEM \"u\"><d>&z;</d>",
            Some("undeclared_entity"),
        ),
    ] {
        match (make().parse(src), want) {
            (Ok(_), None) => {}
            (Ok(value), Some(code)) => {
                panic!("{label}: parsed as {}, want {code}", json(&value))
            }
            (Err(error), None) => panic!("{label}: {} , want a clean parse", error.code),
            (Err(error), Some(code)) => assert_eq!(error.code, code, "{label}"),
        }
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
