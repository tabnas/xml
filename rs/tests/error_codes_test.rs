// The error catalogue is the contract this port shares with the other
// two runtimes, and until this file existed nothing executed that claim.
// `../AGENTS.md` states that the package declares twenty codes with a
// matching hint for each, that the two catalogues are "exactly in step",
// and that `../tabnas.plugin.json` is the machine-readable list of them.
// All three were prose. A code added to `ts/src/xml.ts` and not here, or
// here and not there, means the two runtimes reject the same document for
// different reasons, which is the one thing the shared fixtures cannot
// catch on their own: a fixture row pins `ERROR:<code>` only for the
// codes somebody wrote a row for, so a code nobody wrote a row for could
// change name or wording with every suite still green.
//
// So the canonical TypeScript source and the plugin descriptor are both
// read, and the installed catalogue is read back off a parser rather than
// off the constants, since what an installed plugin actually contributes
// is the thing a caller sees.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
        .to_path_buf()
}

/// The codes the descriptor declares, in declaration order.
fn descriptor_codes() -> Vec<String> {
    let text = fs::read_to_string(repo_root().join("tabnas.plugin.json"))
        .expect("tabnas.plugin.json is readable");
    let descriptor: serde_json::Value =
        serde_json::from_str(&text).expect("tabnas.plugin.json is JSON");
    descriptor["errorCodes"]
        .as_array()
        .expect("the descriptor declares errorCodes")
        .iter()
        .map(|code| {
            code.as_str()
                .expect("every errorCodes entry is a string")
                .to_string()
        })
        .collect()
}

/// The keys of one object literal in `ts/src/xml.ts`, named by the line
/// that opens it. The block is read to its closing brace at the opening
/// line's indent, and a key is a line whose first non-space run is an
/// identifier followed by a colon. Every value in these two tables is a
/// quoted string or a template literal, so a continuation line begins
/// with a quote and cannot be mistaken for a key.
fn canonical_table_keys(opener: &str) -> Vec<String> {
    let source = fs::read_to_string(repo_root().join("ts").join("src").join("xml.ts"))
        .expect("ts/src/xml.ts is readable");
    let mut lines = source.lines();
    let open = lines
        .find(|line| line.trim() == opener)
        .unwrap_or_else(|| panic!("ts/src/xml.ts opens a `{opener}` block"));
    let indent = open.len() - open.trim_start().len();
    let mut keys = Vec::new();
    for line in lines {
        let body = line.trim_start();
        let depth = line.len() - body.len();
        if depth <= indent && (body.starts_with('}') || body.starts_with("},")) {
            return keys;
        }
        if let Some(name) = body.split(':').next() {
            let identifier = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if identifier && body.len() > name.len() && body.as_bytes()[name.len()] == b':' {
                keys.push(name.to_string());
            }
        }
    }
    panic!("the `{opener}` block in ts/src/xml.ts is not closed");
}

/// The canonical `error` table, read as key and message. Every value in
/// that one table is a single-quoted string literal, possibly wrapped
/// onto the next line, and none of them escapes a quote; the `hint`
/// table uses template literals carrying real newlines and is compared
/// by key alone.
fn canonical_error_table() -> BTreeMap<String, String> {
    let source = fs::read_to_string(repo_root().join("ts").join("src").join("xml.ts"))
        .expect("ts/src/xml.ts is readable");
    let mut lines = source.lines();
    let open = lines
        .find(|line| line.trim() == "error: {")
        .expect("ts/src/xml.ts opens an `error` block");
    let indent = open.len() - open.trim_start().len();
    let mut table = BTreeMap::new();
    let mut entry: Option<(String, String)> = None;
    let flush = |entry: Option<(String, String)>, table: &mut BTreeMap<String, String>| {
        if let Some((code, raw)) = entry {
            let value = raw.trim().strip_suffix(',').unwrap_or(raw.trim()).trim();
            let text = value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
                .unwrap_or_else(|| {
                    panic!(
                        "the {code} entry in ts/src/xml.ts is not a single-quoted string: {value}"
                    )
                });
            table.insert(code, text.to_string());
        }
    };
    for line in lines {
        let body = line.trim_start();
        let depth = line.len() - body.len();
        if depth <= indent && body.starts_with('}') {
            flush(entry, &mut table);
            return table;
        }
        match body.split_once(':') {
            Some((name, rest))
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') =>
            {
                flush(entry.take(), &mut table);
                entry = Some((name.to_string(), rest.trim().to_string()));
            }
            _ => {
                if let Some((_, value)) = entry.as_mut() {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(body);
                }
            }
        }
    }
    panic!("the `error` block in ts/src/xml.ts is not closed");
}

/// The canonical `hint` table, read as key and template. Every value in
/// that table is a BACKTICK template literal, and several carry real
/// newlines, which is why this file compared the hints by key alone: the
/// keys said "the same twenty codes", and nothing said the twenty hints
/// said the same thing.
///
/// A rewording, or a dropped `{placeholder}`, reached a user with every
/// test green, and the fixtures could not catch it either -- a row pins
/// the CODE, and only the rows carrying a `msg` cell pin any text at all,
/// none of it hint text.
///
/// The extraction is the same shape as the `error` one, reading from the
/// first backtick after the colon to the closing backtick and joining the
/// intervening lines with `\n`, which is what a template literal means and
/// what the installed catalogue holds.
///
/// One of the twenty carries a `${IDENT}` substitution, which the port
/// resolves at build time -- `reserved_namespace` names `${XML_NS_URI}`,
/// and the installed hint holds the URI itself. Every `${IDENT}` is
/// resolved against a `const IDENT = '...'` in the same file rather than
/// skipped, because a hint the comparison steps over is a hint nothing
/// checks, which is the state this test exists to end.
fn canonical_hint_table() -> BTreeMap<String, String> {
    let source = fs::read_to_string(repo_root().join("ts").join("src").join("xml.ts"))
        .expect("ts/src/xml.ts is readable");
    let mut lines = source.lines();
    let open = lines
        .find(|line| line.trim() == "hint: {")
        .expect("ts/src/xml.ts opens a `hint` block");
    let indent = open.len() - open.trim_start().len();
    let mut table = BTreeMap::new();
    let mut open_entry: Option<(String, Vec<String>)> = None;
    for line in lines {
        let body = line.trim_start();
        let depth = line.len() - body.len();
        if open_entry.is_none() && depth <= indent && body.starts_with('}') {
            return table;
        }
        if let Some((code, parts)) = open_entry.as_mut() {
            // Inside a template literal: everything up to the closing
            // backtick, verbatim, including leading whitespace.
            match line.split_once('`') {
                Some((before, _)) => {
                    parts.push(before.to_string());
                    table.insert(code.clone(), parts.join("\n"));
                    open_entry = None;
                }
                None => parts.push(line.to_string()),
            }
            continue;
        }
        let Some((name, rest)) = body.split_once(':') else {
            continue;
        };
        let is_code = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !is_code {
            continue;
        }
        let Some(after) = rest.trim_start().strip_prefix('`') else {
            panic!("the {name} hint in ts/src/xml.ts is not a template literal: {rest}");
        };
        match after.split_once('`') {
            Some((text, _)) => {
                table.insert(name.to_string(), text.to_string());
            }
            None => open_entry = Some((name.to_string(), vec![after.to_string()])),
        }
    }
    panic!("the `hint` block in ts/src/xml.ts is not closed");
}

/// Resolve every `${IDENT}` against a `const IDENT = '…'` in `xml.ts`.
/// An unresolved one panics rather than being left in place: a template
/// compared with its own source text is a comparison that cannot fail.
fn resolve_substitutions(table: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let source = fs::read_to_string(repo_root().join("ts").join("src").join("xml.ts"))
        .expect("ts/src/xml.ts is readable");
    let constant = |name: &str| -> Option<String> {
        let needle = format!("const {name} = '");
        let at = source.find(&needle)? + needle.len();
        let end = source[at..].find('\'')?;
        Some(source[at..at + end].to_string())
    };
    table
        .into_iter()
        .map(|(code, mut hint)| {
            while let Some(at) = hint.find("${") {
                let end = at + hint[at..]
                    .find('}')
                    .unwrap_or_else(|| panic!("the {code} hint has an unclosed substitution"));
                let name = &hint[at + 2..end];
                let value = constant(name).unwrap_or_else(|| {
                    panic!("the {code} hint names ${{{name}}}, and ts/src/xml.ts declares no such const")
                });
                hint.replace_range(at..=end, &value);
            }
            (code, hint)
        })
        .collect()
}

/// The error and hint catalogues an installed plugin actually carries,
/// and the ones a bare jsonic parser carries. The two are returned
/// together because one of the twenty codes, `unterminated_comment`, is
/// also a base code every grammar inherits: the plugin overrides its
/// message rather than adding a name, so "what this plugin contributed"
/// cannot be read off the installed catalogue alone.
struct Catalogues {
    error: std::collections::HashMap<String, String>,
    hint: std::collections::HashMap<String, String>,
    base_error: std::collections::HashMap<String, String>,
    base_hint: std::collections::HashMap<String, String>,
}

fn installed_catalogue() -> Catalogues {
    let base = tabnas_jsonic::make().config();
    let installed = tabnas_xml::make().config();
    Catalogues {
        error: installed.error,
        hint: installed.hint,
        base_error: base.error,
        base_hint: base.hint,
    }
}

/// The descriptor is what agent tooling reads, so it is the list the
/// canonical source is measured against first.
#[test]
fn the_descriptor_lists_the_canonical_codes() {
    let descriptor: BTreeSet<String> = descriptor_codes().into_iter().collect();
    let canonical: BTreeSet<String> = canonical_table_keys("error: {").into_iter().collect();
    assert_eq!(
        canonical, descriptor,
        "tabnas.plugin.json and the `error` table in ts/src/xml.ts declare different codes"
    );
}

/// Every declared code carries a hint on the canonical side, which is the
/// property `../AGENTS.md` states and the reason the port carries two
/// tables rather than one.
#[test]
fn the_canonical_tables_are_paired() {
    let errors: BTreeSet<String> = canonical_table_keys("error: {").into_iter().collect();
    let hints: BTreeSet<String> = canonical_table_keys("hint: {").into_iter().collect();
    assert_eq!(
        errors, hints,
        "the `error` and `hint` tables in ts/src/xml.ts do not cover the same codes"
    );
}

/// The port declares neither fewer codes nor more. A port-only code would
/// reject a document this package's other runtimes accept, under a name
/// no fixture and no descriptor knows.
#[test]
fn the_port_installs_exactly_the_canonical_codes() {
    let catalogues = installed_catalogue();
    let canonical: BTreeSet<String> = canonical_table_keys("error: {").into_iter().collect();

    let missing: BTreeSet<&String> = canonical
        .iter()
        .filter(|code| !catalogues.error.contains_key(*code))
        .collect();
    assert!(
        missing.is_empty(),
        "the port does not install these canonical codes: {missing:?}"
    );

    // An inherited base code is exempt only while it is UNCHANGED. The
    // filter used to exempt it on the name alone, so the port could
    // override the message or the hint of any code jsonic declares --
    // `unexpected`, which this parser does raise -- and pass every test
    // in this file: the template comparison below walks the canonical
    // codes, and an override of an inherited one is not among them.
    let extra: BTreeSet<&String> = catalogues
        .error
        .keys()
        .filter(|code| {
            !canonical.contains(*code)
                && catalogues.base_error.get(*code) != catalogues.error.get(*code)
        })
        .collect();
    assert!(
        extra.is_empty(),
        "the port installs or overrides codes the canonical `error` table does not declare: {extra:?}"
    );

    let unhinted: BTreeSet<&String> = canonical
        .iter()
        .filter(|code| !catalogues.hint.contains_key(*code))
        .collect();
    assert!(
        unhinted.is_empty(),
        "these canonical codes are installed with no hint: {unhinted:?}"
    );

    let extra_hints: BTreeSet<&String> = catalogues
        .hint
        .keys()
        .filter(|code| {
            !canonical.contains(*code)
                && catalogues.base_hint.get(*code) != catalogues.hint.get(*code)
        })
        .collect();
    assert!(
        extra_hints.is_empty(),
        "the port installs or overrides hints for codes it does not declare: {extra_hints:?}"
    );
}

/// The message template itself, not only the name. The canonical table
/// is read out of `ts/src/xml.ts` and compared entry for entry, so a
/// placeholder dropped on one side, or a rewording, fails here. The
/// shared fixtures cannot stand in for this: a row pins the CODE, and
/// only the rows carrying a `msg` cell pin any of the text.
///
/// One of the twenty, `unterminated_comment`, is also a base code the
/// engine declares, and the engine's wording currently happens to match.
/// The comparison is against the canonical table either way, so the day
/// either text moves this says so.
#[test]
fn the_port_installs_the_canonical_message_templates() {
    let catalogues = installed_catalogue();
    let canonical = canonical_error_table();
    // An extraction that silently returned nothing would make every
    // assertion below vacuous, which is the failure mode this whole file
    // exists to close.
    assert_eq!(
        canonical.len(),
        descriptor_codes().len(),
        "the messages read out of ts/src/xml.ts do not cover every declared code"
    );
    for (code, message) in canonical {
        assert_eq!(
            catalogues.error.get(&code),
            Some(&message),
            "the {code} message differs from the canonical one in ts/src/xml.ts"
        );
    }
}

/// The hint template itself, for the same reason and by the same route.
/// `the_canonical_tables_are_paired` and the key comparisons above answer
/// "the same codes"; this answers "the same words", which is what a
/// reader of a diagnostic actually gets.
#[test]
fn the_port_installs_the_canonical_hint_templates() {
    let catalogues = installed_catalogue();
    let canonical = resolve_substitutions(canonical_hint_table());
    assert_eq!(
        canonical.len(),
        descriptor_codes().len(),
        "the hints read out of ts/src/xml.ts do not cover every declared code"
    );
    // A multi-line hint is the case the extraction could quietly get
    // wrong, so one is named rather than left to the loop.
    assert!(
        canonical
            .get("xml_mismatched_tag")
            .is_some_and(|hint| hint.contains('\n')),
        "the multi-line hint came back as one line; the extraction is wrong"
    );
    // The substitution case, named rather than left to the loop: this is
    // the entry that would otherwise be compared against its own source
    // text and could never fail.
    assert!(
        canonical
            .get("reserved_namespace")
            .is_some_and(|hint| hint.contains("http://www.w3.org/XML/1998/namespace")
                && !hint.contains("${")),
        "the ${{XML_NS_URI}} substitution was not resolved"
    );
    for (code, hint) in canonical {
        assert_eq!(
            catalogues.hint.get(&code),
            Some(&hint),
            "the {code} hint differs from the canonical one in ts/src/xml.ts"
        );
    }
}

/// Every declared code is pinned by at least one shared fixture row, so
/// a code that stops being raised fails a suite rather than becoming a
/// name nothing produces. The root `AGENTS.md` carried this as a count
/// in prose, and the count had gone stale in the safe direction: it
/// named six codes as unpinned that fixture rows had since covered.
#[test]
fn every_declared_code_is_pinned_by_a_fixture() {
    let spec = repo_root().join("test").join("spec");
    // The EXPECTED cell, not the row. A substring search over the whole
    // line counts a code named in the test's name, its XML input, its
    // options or its message column, so the gate could stay green after
    // the only assertion of that code was deleted -- which is the state
    // it exists to report. Column 3 (`name input expected opts [msg]`),
    // compared exactly.
    let mut expected: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(&spec).expect("the spec directory lists") {
        let path = entry.expect("a directory entry").path();
        if path.extension().is_none_or(|ext| ext != "tsv") {
            continue;
        }
        // Comment lines are dropped. A code named in a fixture's
        // legend is prose, and counting it would make this gate agree
        // with the prose it replaces rather than with the rows.
        for line in fs::read_to_string(&path)
            .expect("a fixture is readable")
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        {
            let fields: Vec<&str> = line.split('\t').collect();
            let Some(cell) = fields.get(2) else { continue };
            if let Some(code) = cell.trim().strip_prefix("ERROR:") {
                expected.insert(code.to_string());
            }
        }
    }
    assert!(
        !expected.is_empty(),
        "no ERROR: expectations were read from {spec:?}"
    );
    let unpinned: Vec<String> = descriptor_codes()
        .into_iter()
        .filter(|code| !expected.contains(code))
        .collect();
    assert!(
        unpinned.is_empty(),
        "these declared codes are pinned by no fixture row: {unpinned:?}. \
         Add a row to test/spec, which every runtime discovers, rather than \
         recording the gap in prose"
    );
}
