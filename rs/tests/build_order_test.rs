//! The order the rules build an element in (`xml-grammar.jsonic` sets it
//! out), and the two things that order must not cost: the value, which is
//! what it always was, member order included, and rule depth, which a
//! list of siblings must not grow. Mirrors `ts/test/build-order.test.ts`.
//!
//! The order is what lets a reader of the rule events (tabnas-transduce's
//! incremental source) stream a document as the parse proceeds: each
//! element in a cell of its own, made when its start tag is read, with
//! its members added in their order; the `children` member named in
//! `u.key` before its list opens; the list in a cell of its own, each
//! child appended when it is done; and the root element, once it is done,
//! in the start rule's cell, set aside while the white space after it is
//! read, so that the rule replacing the start rule to read it does not
//! open holding the finished document, and put back when the last rule
//! closes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tabnas::{RuleState, Tabnas, Value};

/// A container's identity: its allocation, which the grammar builds in
/// place.
fn identity(value: &Value) -> Option<(char, usize)> {
    match value {
        Value::Object(map) => Some(('M', Arc::as_ptr(map) as usize)),
        Value::Array(list) => Some(('L', Arc::as_ptr(list) as *const () as usize)),
        _ => None,
    }
}

/// Each rule pass as `[state, rule, depth, node, u.key, transition]`, a
/// container named by the order it was first seen (`M` a map, `L` a list)
/// and shown with its keys or length at the end of the pass.
fn passes(mut parser: Tabnas, src: &str) -> Vec<[String; 6]> {
    let out = Arc::new(Mutex::new((Vec::new(), HashMap::new())));
    let seen = Arc::clone(&out);
    parser.subscribe_rule_done(move |rule, _ctx, done| {
        let mut guard = seen.lock().unwrap();
        let (passes, ids): &mut (Vec<[String; 6]>, HashMap<usize, usize>) = &mut guard;
        let node = rule.node.borrow();
        let shown = match identity(&node) {
            Some((kind, ptr)) => {
                let next = ids.len();
                let id = *ids.entry(ptr).or_insert(next);
                match &*node {
                    Value::Object(map) => format!(
                        "{kind}{id}{{{}}}",
                        map.keys().cloned().collect::<Vec<_>>().join(",")
                    ),
                    Value::Array(list) => format!("{kind}{id}[{}]", list.len()),
                    _ => unreachable!(),
                }
            }
            None if node.is_undefined() => "undefined".to_string(),
            None => node.to_string(),
        };
        let step = done
            .alt
            .as_ref()
            .map(|alt| {
                let mut step = String::new();
                if !alt.p.is_empty() {
                    step.push_str(&format!("p:{}", alt.p));
                }
                if !alt.r.is_empty() {
                    step.push_str(&format!("r:{}", alt.r));
                }
                step
            })
            .unwrap_or_default();
        let key = match rule.u.get("key") {
            Some(Value::String(key)) => key.clone(),
            _ => String::new(),
        };
        let state = match done.state {
            RuleState::Open => "o",
            RuleState::Close => "c",
        };
        passes.push([
            state.to_string(),
            rule.name.as_str().to_string(),
            rule.d.to_string(),
            shown,
            key,
            step,
        ]);
    });
    parser.parse(src).expect("parses");
    drop(parser);
    let passes = std::mem::take(&mut out.lock().unwrap().0);
    passes
}

#[test]
fn each_element_is_built_in_a_cell_of_its_own_member_by_member_in_document_order() {
    // Each pass as `state rule depth node u.key transition`, `-` for an
    // empty field.
    let want = [
        "o xml 0 undefined - p:element",
        // The element's cell, with the members its start tag gives.
        "o element 1 M0{name,localName,attributes} - p:head",
        // head reads nothing: its close comes after those members.
        "o head 2 M0{name,localName,attributes} - -",
        "c head 2 M0{name,localName,attributes} - r:content",
        // content names the member it builds before the list opens.
        "o content 2 M0{name,localName,attributes} children p:children",
        // The list, in a cell of its own.
        "o children 3 L1[0] - p:child",
        // A text child is in the list at once.
        "o child 4 L1[1] - -",
        "c child 4 L1[1] - r:child",
        // The next child replaces the last, at the same depth.
        "o child 4 L1[1] - p:element",
        // A self-closing element is complete when its start tag is read.
        "o element 5 M2{name,localName,attributes,children} - -",
        "c element 5 M2{name,localName,attributes,children} - -",
        // A finished element child is appended when the child closes.
        "c child 4 L1[2] - -",
        // The last child hands the list back to the rule whose cell it is.
        "c children 3 L1[2] - -",
        // The finished list becomes `children`.
        "c content 2 M0{name,localName,attributes,children} children -",
        "c element 1 M0{name,localName,attributes,children} - -",
        // The white space after the root replaces the start rule, so the
        // root is set aside and the rule replacing it shares a cell holding
        // nothing; the root is the result at the last close.
        "c xml 0 undefined - r:xml",
        "o xml 0 undefined - -",
        "c xml 0 M0{name,localName,attributes,children} - -",
    ];
    let got: Vec<String> = passes(tabnas_xml::make(), "<a x=\"1\">t<b/></a>\n")
        .iter()
        .map(|pass| {
            pass.iter()
                .map(|field| {
                    if field.is_empty() {
                        "-"
                    } else {
                        field.as_str()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    assert_eq!(got, want);
}

/// The deepest rule a parse of `src` reaches, and the value.
fn max_depth(src: &str) -> (usize, Value) {
    let mut parser = tabnas_xml::make();
    let max = Arc::new(Mutex::new(0usize));
    let seen = Arc::clone(&max);
    parser.subscribe_rule_done(move |rule, _ctx, _done| {
        let mut max = seen.lock().unwrap();
        *max = (*max).max(rule.d);
    });
    let value = parser.parse(src).expect("parses");
    let depth = *max.lock().unwrap();
    (depth, value)
}

#[test]
fn rule_depth_over_10000_siblings_is_what_one_sibling_needs() {
    let (one, _) = max_depth("<r><i>v</i></r>");
    let (many, value) = max_depth(&format!("<r>{}</r>", "<i>v</i>t".repeat(10_000)));
    let Value::Object(root) = &value else {
        panic!("an element")
    };
    let Some(Value::Array(children)) = root.get("children") else {
        panic!("children")
    };
    assert_eq!(children.len(), 20_000);
    assert_eq!(many, one);
    // xml 0, element 1, head and content 2, children 3, child 4, the
    // element <i> 5, its head and content 6, its children 7, its text
    // child 8.
    assert_eq!(one, 8);
    // A level of nesting costs four rules: element, head (then content),
    // children and child.
    let (nested, _) = max_depth("<r><i><i><i>v</i></i></i></r>");
    assert_eq!(nested, one + 8);
}

#[test]
fn the_value_keeps_its_members_in_order() {
    let parse = |parser: &Tabnas, src: &str| parser.parse(src).expect("parses").to_string();
    let pure = tabnas_xml::make();
    assert_eq!(
        parse(
            &pure,
            r#"<p:a xmlns:p="urn:p" xml:lang="en">hi<p:b/></p:a>"#
        ),
        concat!(
            r#"{"name":"p:a","localName":"a","attributes":{"xmlns:p":"urn:p","xml:lang":"en"},"#,
            r#""children":["hi",{"name":"p:b","localName":"b","attributes":{},"children":[],"#,
            r#""prefix":"p","namespace":"urn:p","lang":"en"}],"prefix":"p","namespace":"urn:p","lang":"en"}"#
        )
    );
    assert_eq!(
        parse(
            &pure,
            r#"<a xmlns:p="u"><b xmlns:p="v" xml:space="preserve"><p:c/></b><p:d/></a>"#
        ),
        concat!(
            r#"{"name":"a","localName":"a","attributes":{"xmlns:p":"u"},"children":["#,
            r#"{"name":"b","localName":"b","attributes":{"xmlns:p":"v","xml:space":"preserve"},"#,
            r#""children":[{"name":"p:c","localName":"c","attributes":{},"children":[],"#,
            r#""prefix":"p","namespace":"v","space":"preserve"}],"space":"preserve"},"#,
            r#"{"name":"p:d","localName":"d","attributes":{},"children":[],"prefix":"p","namespace":"u"}]}"#
        )
    );
    assert_eq!(
        parse(
            &pure,
            "<?xml version=\"1.0\"?>\n<!-- c -->\n<a xmlns=\"A\"/>\n<!-- d -->\n"
        ),
        r#"{"name":"a","localName":"a","attributes":{"xmlns":"A"},"children":[],"namespace":"A"}"#
    );
    let plain = tabnas_xml::make_with(&tabnas_xml::XmlOptions {
        namespaces: false,
        ..Default::default()
    });
    assert_eq!(
        parse(&plain, r#"<p:a xmlns:p="urn:p"><p:b/></p:a>"#),
        concat!(
            r#"{"name":"p:a","localName":"p:a","attributes":{"xmlns:p":"urn:p"},"#,
            r#""children":[{"name":"p:b","localName":"p:b","attributes":{},"children":[]}]}"#
        )
    );
}

#[test]
fn a_namespace_violation_fails_the_document_where_it_always_did() {
    let strict = tabnas_xml::make_with(&tabnas_xml::XmlOptions {
        strict_namespaces: true,
        ..Default::default()
    });
    let code = |src: &str| strict.parse(src).expect_err("refused").code;
    // The first violation in document order, raised when the root element
    // is done: a later one, or a later prefix, does not change the code.
    assert_eq!(code(r#"<a><p:b/><c xmlns:xml="x"/></a>"#), "unbound_prefix");
    assert_eq!(
        code(r#"<a><c xmlns:xml="x"/><p:b/></a>"#),
        "reserved_namespace"
    );
    // A malformed document fails as malformed first.
    assert_eq!(code("<a><p:b/></c>"), "xml_mismatched_tag");
}

#[test]
fn embed_mode_resolves_each_xml_literal_on_its_own_and_stops_at_its_first_violation() {
    let mut parser = tabnas_jsonic::make();
    parser
        .use_plugin(
            tabnas_xml::plugin(),
            Some(Value::from_json(
                &serde_json::json!({"embed": true, "strictNamespaces": true}),
            )),
        )
        .expect("installs on a jsonic host");
    let value = parser
        .parse(r#"{k: <a xmlns:q="u"><p:b/><q:c><q:d/></q:c></a>, j: <q:e xmlns:q="w"/>}"#)
        .expect("parses");
    // p:b is the first violation: it keeps its prefix and local name, and
    // the elements after it in document order are left unresolved; the
    // second literal starts afresh.
    assert_eq!(
        value.to_string(),
        concat!(
            r#"{"k":{"name":"a","localName":"a","attributes":{"xmlns:q":"u"},"children":["#,
            r#"{"name":"p:b","localName":"b","attributes":{},"children":[],"prefix":"p"},"#,
            r#"{"name":"q:c","localName":"q:c","attributes":{},"children":["#,
            r#"{"name":"q:d","localName":"q:d","attributes":{},"children":[]}]}]},"#,
            r#""j":{"name":"q:e","localName":"e","attributes":{"xmlns:q":"w"},"children":[],"#,
            r#""prefix":"q","namespace":"w"}}"#
        )
    );
}
