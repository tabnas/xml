// The engine's error carries a code, position, hint and a formatted
// report, so `Result<_, TabnasError>` trips clippy's `result_large_err`.
// The crate allows the lint at its own root for the same reason; an
// integration test is a separate crate, so it allows it again rather
// than boxing an error the engine hands back unboxed.
#![allow(clippy::result_large_err)]

// ---------------------------------------------------------------------------
// W3C XML Conformance Test Suite (xmlts): the Rust runner
//
// Corpus: https://www.w3.org/XML/Test/xmlts20130923.tar.gz
//         sha256 9b61db9f5dbffa545f4b8d78422167083a8568c59bd1129f94138f936cf6fc1f
// Fetched by scripts/fetch-xml-suite.sh into test/xmlconf/ (gitignored: the
// corpus is W3C-owned and is never committed to this repository). Cargo has
// no `pretest` hook, so `corpus()` below performs the fetch, once, before
// the first test that needs it; a corpus that cannot be obtained FAILS
// every test here. They never skip: a conformance suite that quietly does
// not run is worse than no suite, because the green tick then means
// nothing.
//
// This is the Rust mirror of go/xmlconf_test.go and ts/test/xmlconf.test.ts.
// The runners must agree on scope, or the parity claim is meaningless:
//   valid   -> must be ACCEPTED, and match the catalogue's canonical OUTPUT
//   invalid -> must still be ACCEPTED (this parser is non-validating)
//   not-wf  -> must be REJECTED
//   error   -> reporting is at the processor's discretion, so these are not
//              turned into tests at all, rather than into a test that
//              asserts nothing
// ---------------------------------------------------------------------------

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use regex::Regex;
use tabnas::{Tabnas, Value};
use tabnas_xml::decode_bom;

use common::repo_root;

fn conf_root() -> PathBuf {
    repo_root().join("test").join("xmlconf")
}

fn catalog_path() -> PathBuf {
    conf_root().join("xmlconf.xml")
}

/// The corpus root, fetched on first use. Panics, and so fails the test,
/// when the corpus is missing and cannot be fetched.
fn corpus() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        if !catalog_path().is_file() {
            let script = repo_root().join("scripts").join("fetch-xml-suite.sh");
            eprintln!(
                "W3C XML conformance corpus missing; running {}",
                script.display()
            );
            let status = Command::new("bash")
                .arg(&script)
                .current_dir(repo_root())
                .status();
            match status {
                Ok(status) if status.success() => {}
                other => panic!(
                    "\nFATAL: the W3C XML Conformance Test Suite is missing and could not be fetched.\n  \
                     expected catalogue: {}\n  fetch: {other:?}\n  \
                     fix: run scripts/fetch-xml-suite.sh (needs network access to w3.org)\n  \
                     these tests do NOT skip.\n",
                    catalog_path().display()
                ),
            }
        }
        assert!(
            catalog_path().is_file(),
            "FATAL: corpus still missing after fetch: {}",
            catalog_path().display()
        );
        conf_root()
    })
}

/// One parser for the whole suite: `parse` takes `&self` and builds a
/// fresh context per call, so reuse is the documented setup.
fn parser() -> &'static Tabnas {
    static PARSER: OnceLock<Tabnas> = OnceLock::new();
    PARSER.get_or_init(tabnas_xml::make)
}

/// Parse one corpus document. The corpus mixes UTF-8, UTF-16 and UTF-32
/// files; `decode_bom` transcodes so the encoding is transparent.
fn parse_document(bytes: &[u8]) -> Result<Value, tabnas::TabnasError> {
    parser().parse(&decode_bom(bytes))
}

fn xml_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "xml"))
        .collect();
    files.sort();
    files
}

fn first_n(list: &[String], n: usize) -> String {
    list.iter()
        .take(n)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n  ")
}

// ---------------------------------------------------------------------------
// The narrow guard (xmltest/valid/sa and xmltest/not-wf/sa)
//
// 306 of the catalogue's 2586 documents, asserted as pass-count floors.
// The catalogue-wide runner further down is strictly wider, but these
// floors are a tighter guard on the sub-corpus they cover, so they stay.
// ---------------------------------------------------------------------------

/// Minimum `valid/sa/*.xml` documents that must parse without error.
/// Every one of the 120 does, so the floor is the total.
const VALID_SA_PASS_FLOOR: usize = 120;

/// Minimum `not-wf/sa/*.xml` documents that must be rejected (of 186).
/// The parser catches structural well-formedness errors but not most
/// character-level constraints or DTD-declaration syntax, so the floor
/// is well below the total and serves as a regression guard. Measured: 74.
const NOT_WF_SA_REJECT_FLOOR: usize = 74;

#[test]
fn xmlconf_valid_standalone() {
    let files = xml_files(&corpus().join("xmltest").join("valid").join("sa"));
    assert!(!files.is_empty(), "no files under xmltest/valid/sa");
    let mut pass = 0;
    let mut failures = Vec::new();
    for path in &files {
        let body =
            fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        match parse_document(&body) {
            Ok(_) => pass += 1,
            Err(error) => failures.push(format!(
                "{}: {}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                error.to_string().lines().next().unwrap_or_default()
            )),
        }
    }
    eprintln!("valid/sa: {pass} / {} parsed successfully", files.len());
    assert!(
        pass >= VALID_SA_PASS_FLOOR,
        "valid/sa pass count {pass} dropped below floor {VALID_SA_PASS_FLOOR} (total {}). Sample failures:\n  {}",
        files.len(),
        first_n(&failures, 5)
    );
}

#[test]
fn xmlconf_not_well_formed_standalone() {
    let files = xml_files(&corpus().join("xmltest").join("not-wf").join("sa"));
    assert!(!files.is_empty(), "no files under xmltest/not-wf/sa");
    let mut rejected = 0;
    let mut false_accepts = Vec::new();
    for path in &files {
        let body =
            fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        if parse_document(&body).is_err() {
            rejected += 1;
        } else {
            false_accepts.push(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    eprintln!(
        "not-wf/sa: {rejected} / {} rejected as expected",
        files.len()
    );
    assert!(
        rejected >= NOT_WF_SA_REJECT_FLOOR,
        "not-wf/sa reject count {rejected} dropped below floor {NOT_WF_SA_REJECT_FLOOR} (total {}). Sample false accepts:\n  {}",
        files.len(),
        first_n(&false_accepts, 5)
    );
}

// ---------------------------------------------------------------------------
// Catalogue reader
//
// xmlconf.xml is itself XML; parsing it with the parser under test would be
// circular. The <TEST> elements are flat and attribute-only, so a scanner is
// enough. Sub-catalogues arrive through internal-subset SYSTEM entities, and
// xml:base on the enclosing <TESTCASES> gives the directory each URI is
// relative to.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ConfTest {
    id: String,
    /// valid | invalid | not-wf | error
    kind: String,
    recommendation: String,
    sections: String,
    /// The document.
    uri: PathBuf,
    /// The expected canonical output, when the catalogue gives one.
    output: Option<PathBuf>,
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"([A-Za-z:._-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("literal")
    })
}

fn entity_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<!ENTITY\s+([A-Za-z0-9._-]+)\s+SYSTEM\s+"([^"]+)"\s*>"#).expect("literal")
    })
}

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?s)<TESTCASES\b([^>]*)>|</TESTCASES\s*>|<TEST\b([^>]*?)/?>(.*?)</TEST\s*>|&([A-Za-z0-9._-]+);")
            .expect("literal")
    })
}

fn attrs_of(tag: &str) -> BTreeMap<String, String> {
    attr_re()
        .captures_iter(tag)
        .map(|found| {
            let value = found
                .get(2)
                .or_else(|| found.get(3))
                .map_or(String::new(), |value| value.as_str().to_string());
            (found[1].to_string(), value)
        })
        .collect()
}

/// The 2013 catalogue's final <TESTCASES> declares
/// xml:base="eduni/namespaces/misc/" but ships those files in eduni/misc/.
/// Upstream inconsistency; try both. If neither exists the census test
/// fails: a missing corpus file is an error, never a silent skip.
fn resolve_in_corpus(base: &Path, rel: &str) -> PathBuf {
    let primary = corpus().join(base).join(rel);
    if primary.exists() {
        return primary;
    }
    let alt = PathBuf::from(primary.to_string_lossy().replacen(
        "eduni/namespaces/misc",
        "eduni/misc",
        1,
    ));
    if alt.exists() {
        return alt;
    }
    primary
}

fn read_catalog(file: &Path, base: &Path, into: &mut Vec<ConfTest>) {
    let body = fs::read_to_string(file)
        .unwrap_or_else(|error| panic!("read catalogue {}: {error}", file.display()));
    let dir = file.parent().expect("a catalogue has a directory");

    let entities: BTreeMap<String, String> = entity_re()
        .captures_iter(&body)
        .map(|found| (found[1].to_string(), found[2].to_string()))
        .collect();

    let ws = Regex::new(r"\s+").expect("literal");
    let mut stack: Vec<PathBuf> = vec![base.to_path_buf()];
    for found in token_re().captures_iter(&body) {
        let token = &found[0];
        let top = stack
            .last()
            .expect("the base is always on the stack")
            .clone();
        if token.starts_with("<TESTCASES") {
            let attrs = attrs_of(found.get(1).map_or("", |m| m.as_str()));
            match attrs.get("xml:base").filter(|base| !base.is_empty()) {
                Some(base) => stack.push(top.join(base)),
                None => stack.push(top),
            }
        } else if token.starts_with("</TESTCASES") {
            if stack.len() > 1 {
                stack.pop();
            }
        } else if token.starts_with("<TEST") {
            let attrs = attrs_of(found.get(2).map_or("", |m| m.as_str()));
            let get = |name: &str| attrs.get(name).cloned().unwrap_or_default();
            let recommendation = match attrs.get("RECOMMENDATION") {
                Some(rec) if !rec.is_empty() => rec.clone(),
                _ => "XML1.0".to_string(),
            };
            let output = attrs
                .get("OUTPUT")
                .filter(|output| !output.is_empty())
                .map(|output| resolve_in_corpus(&top, output));
            let _description = ws
                .replace_all(found.get(3).map_or("", |m| m.as_str()), " ")
                .trim()
                .to_string();
            into.push(ConfTest {
                id: get("ID"),
                kind: get("TYPE"),
                recommendation,
                sections: get("SECTIONS"),
                uri: resolve_in_corpus(&top, &get("URI")),
                output,
            });
        } else if token.starts_with('&') {
            if let Some(system) = entities.get(&found[4]) {
                read_catalog(&dir.join(system), &top, into);
            }
        }
    }
}

fn load_catalog() -> Vec<ConfTest> {
    let mut all = Vec::new();
    // Through `corpus()`, never `catalog_path()` directly: `corpus()` is the
    // only thing that fetches, and libtest starts `xmlconf_catalog` and
    // `xmlconf_census` first (it orders by name), so reading the path
    // directly fails on every fresh checkout, not only in a race.
    read_catalog(&corpus().join("xmlconf.xml"), Path::new("."), &mut all);
    all
}

/// Whether a catalogue RECOMMENDATION is inside what this plugin claims:
/// XML 1.0 (every errata edition) and Namespaces 1.0. XML1.1 / NS1.1 are
/// a different language version.
fn claimed(recommendation: &str) -> bool {
    recommendation.starts_with("XML1.0") || recommendation.starts_with("NS1.0")
}

fn in_scope(all: Vec<ConfTest>) -> Vec<ConfTest> {
    all.into_iter()
        .filter(|test| claimed(&test.recommendation))
        .collect()
}

// ---------------------------------------------------------------------------
// Canonical XML (James Clark's "first canonical form"), the format of the
// suite's OUTPUT files. Serialising the parse result into it is how the
// VALUE is checked; "it did not error" is not a statement about the value.
// ---------------------------------------------------------------------------

fn canon_escape(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            other => out.push(other),
        }
    }
}

fn canonical_into(node: &Value, out: &mut String) {
    match node {
        Value::String(text) => canon_escape(text, out),
        Value::Null | Value::Undefined => {}
        Value::Object(element) => {
            let name = match element.get("name") {
                Some(Value::String(name)) => name.as_str(),
                _ => "",
            };
            out.push('<');
            out.push_str(name);
            if let Some(Value::Object(attrs)) = element.get("attributes") {
                let mut names: Vec<&String> = attrs.keys().collect();
                names.sort();
                for key in names {
                    out.push(' ');
                    out.push_str(key);
                    out.push_str("=\"");
                    match &attrs[key] {
                        Value::String(text) => canon_escape(text, out),
                        other => canon_escape(&other.to_string(), out),
                    }
                    out.push('"');
                }
            }
            out.push('>');
            if let Some(Value::Array(children)) = element.get("children") {
                for child in children.iter() {
                    canonical_into(child, out);
                }
            }
            out.push_str("</");
            out.push_str(name);
            out.push('>');
        }
        other => canon_escape(&other.to_string(), out),
    }
}

fn canonical(node: &Value) -> String {
    let mut out = String::new();
    canonical_into(node, &mut out);
    out
}

// ---------------------------------------------------------------------------
// The catalogue-wide sweep
//
// One pass over every in-scope document. The assertions are counts, set to
// what this parser achieves, measured with the other two runtimes. A floor
// pinned to the measured value fails the moment conformance drops by a
// single document. Raise a floor when conformance genuinely improves; never
// lower one to make a regression pass.
// ---------------------------------------------------------------------------

/// The corpus census. A silently shrinking corpus would drag every count
/// below down with it and read as "no worse than before".
const CATALOG_TOTAL: usize = 2586;

const VALID_ACCEPT_FLOOR: usize = 728;
const VALID_CANONICAL_FLOOR: usize = 232;
const NOT_WF_REJECT_FLOOR: usize = 438;

#[test]
fn xmlconf_census() {
    let all = load_catalog();
    assert_eq!(
        all.len(),
        CATALOG_TOTAL,
        "catalogue census changed: read {} <TEST> entries, expected {CATALOG_TOTAL}. \
         Either the corpus snapshot changed (check scripts/fetch-xml-suite.sh) or the \
         catalogue reader regressed. A shrinking corpus must never pass quietly.",
        all.len()
    );
    let missing: Vec<String> = all
        .iter()
        .filter(|test| !test.uri.exists())
        .map(|test| format!("{} -> {}", test.id, test.uri.display()))
        .collect();
    assert!(
        missing.is_empty(),
        "catalogued documents missing from the corpus:\n  {}",
        missing.join("\n  ")
    );
    let scoped = in_scope(all.clone());
    let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
    for test in &scoped {
        *by_type.entry(test.kind.as_str()).or_default() += 1;
    }
    eprintln!(
        "catalogue {}: in-scope (XML1.0/NS1.0) {}, out-of-scope (XML1.1/NS1.1) {}",
        all.len(),
        scoped.len(),
        all.len() - scoped.len()
    );
    eprintln!("in-scope by TYPE: {by_type:?} ('error' tests are reported, not asserted)");
}

#[derive(Default)]
struct Sweep {
    valid_total: usize,
    valid_accepted: usize,
    valid_checked: usize,
    valid_correct: usize,
    notwf_total: usize,
    notwf_rejected: usize,
    invalid_total: usize,
    invalid_accepted: usize,
    valid_rejected: Vec<String>,
    valid_mismatched: Vec<String>,
    notwf_accepted: Vec<String>,
    invalid_rejected: Vec<String>,
}

fn sweep_catalog() -> Sweep {
    let mut sweep = Sweep::default();
    for test in in_scope(load_catalog()) {
        if test.kind == "error" {
            continue;
        }
        let body = fs::read(&test.uri)
            .unwrap_or_else(|error| panic!("read {}: {error}", test.uri.display()));
        let result = parse_document(&body);
        let label = format!("{} ({}): {}", test.id, test.sections, test.uri.display());
        match test.kind.as_str() {
            "valid" => {
                sweep.valid_total += 1;
                let Ok(got) = result else {
                    sweep.valid_rejected.push(label);
                    continue;
                };
                sweep.valid_accepted += 1;
                if let Some(output) = &test.output {
                    sweep.valid_checked += 1;
                    let expected = fs::read(output).unwrap_or_else(|error| {
                        panic!("read expected output {}: {error}", output.display())
                    });
                    if canonical(&got) == decode_bom(&expected) {
                        sweep.valid_correct += 1;
                    } else {
                        sweep.valid_mismatched.push(label);
                    }
                }
            }
            "not-wf" => {
                sweep.notwf_total += 1;
                if result.is_err() {
                    sweep.notwf_rejected += 1;
                } else {
                    sweep.notwf_accepted.push(label);
                }
            }
            "invalid" => {
                sweep.invalid_total += 1;
                if result.is_ok() {
                    sweep.invalid_accepted += 1;
                } else {
                    sweep.invalid_rejected.push(label);
                }
            }
            _ => {}
        }
    }
    sweep
}

#[test]
fn xmlconf_catalog() {
    let s = sweep_catalog();

    // valid: must be ACCEPTED. `rmt-e2e-50` (eduni/errata-2e/E50.xml) is
    // the single valid document currently rejected, which is why the
    // floor is 728 and not 729.
    assert!(
        s.valid_accepted >= VALID_ACCEPT_FLOOR,
        "valid accepted {} / {} dropped below the measured floor {VALID_ACCEPT_FLOOR}. Rejected:\n  {}",
        s.valid_accepted,
        s.valid_total,
        first_n(&s.valid_rejected, 5)
    );

    // valid: and must produce the right VALUE where the catalogue says
    // what that value is. This is the assertion the narrow suite cannot
    // make.
    assert!(
        s.valid_correct >= VALID_CANONICAL_FLOOR,
        "canonical-output matches {} / {} dropped below the measured floor {VALID_CANONICAL_FLOOR}. Mismatches:\n  {}",
        s.valid_correct,
        s.valid_checked,
        first_n(&s.valid_mismatched, 5)
    );

    // invalid: a non-validating parser must accept every one of these, so
    // this is an exact assertion, not a floor.
    assert!(
        s.invalid_rejected.is_empty(),
        "a non-validating parser must accept well-formed documents that are merely \
         DTD-invalid, but {} of {} were rejected:\n  {}",
        s.invalid_rejected.len(),
        s.invalid_total,
        first_n(&s.invalid_rejected, 5)
    );

    // not-wf: must be REJECTED.
    assert!(
        s.notwf_rejected >= NOT_WF_REJECT_FLOOR,
        "not-wf rejected {} / {} dropped below the measured floor {NOT_WF_REJECT_FLOOR}. Sample false accepts:\n  {}",
        s.notwf_rejected,
        s.notwf_total,
        first_n(&s.notwf_accepted, 5)
    );

    // The dial: the true numbers in one place, so the state of conformance
    // can be read off a test log without counting anything by hand.
    let pct = |a: usize, b: usize| {
        if b == 0 {
            "-".to_string()
        } else {
            format!("{:.1}%", 100.0 * a as f64 / b as f64)
        }
    };
    let valid_pass = s.valid_accepted - (s.valid_checked - s.valid_correct);
    eprintln!("=== W3C XML conformance, Rust (xmlts 20130923, XML1.0/NS1.0 scope) ===");
    eprintln!(
        "valid   accepted+correct : {valid_pass} / {}  ({})",
        s.valid_total,
        pct(valid_pass, s.valid_total)
    );
    eprintln!(
        "          of which parsed: {} / {}",
        s.valid_accepted, s.valid_total
    );
    eprintln!(
        "          value-compared : {} / {} documents with catalogue OUTPUT",
        s.valid_correct, s.valid_checked
    );
    eprintln!(
        "not-wf  rejected         : {} / {}  ({})",
        s.notwf_rejected,
        s.notwf_total,
        pct(s.notwf_rejected, s.notwf_total)
    );
    eprintln!(
        "invalid accepted (non-validating): {} / {}  ({})",
        s.invalid_accepted,
        s.invalid_total,
        pct(s.invalid_accepted, s.invalid_total)
    );
}
