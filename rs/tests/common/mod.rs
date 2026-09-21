// Shared test helpers. Cargo compiles this module into EVERY integration
// test binary, so an item only one binary uses is dead code in the
// others; the allow keeps that from being a warning rather than hiding
// anything real.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use tabnas::{Tabnas, Value};
use tabnas_support::{find_spec_dir, Failure};

/// The repository root: the parent of `rs/`.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
        .to_path_buf()
}

/// The shared `test/spec` directory, found by walking up from the crate
/// rather than by counting `..` hops.
pub fn spec_dir() -> PathBuf {
    find_spec_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .expect("a test/spec directory above rs/")
}

/// An engine value as the fixture data model, through JSON: the
/// `jsonFlatten` of the Go runner. An element tree holds only strings,
/// objects and arrays, so nothing is lost on the way.
pub fn to_value(value: &Value) -> tabnas_support::Value {
    tabnas_support::Value::from(value.to_json())
}

/// A parse error as the runner's failure: the code the fixture pins, and
/// the rendered report for the `msg` column and the failure message.
pub fn to_failure(error: tabnas::TabnasError) -> Failure {
    Failure::new(error.code.clone())
        .at(error.row, error.col)
        .with_message(error.to_string())
}

/// The parser every fixture row without options gets, built the way the
/// README says to build one.
pub fn default_parser() -> Tabnas {
    tabnas_xml::make()
}

/// The SGR colour sequences the engine writes into rendered error
/// messages, removed so the `msg` column can stay plain text.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

/// The one thing this repository does not take from the support crate:
/// its own escape codec, because xml's fixtures need a sixth escape.
///
/// `\uXXXX` names a character that must not be written literally into a
/// fixture, a leading U+FEFF byte-order mark above all, which is
/// invisible in a diff. The shared codec passes `\u` through on purpose:
/// an XML fixture, like a JSON one, has to be able to carry a literal
/// `A` as source text. So it is decoded here, in one pass over the
/// RAW cell; after the shared codec an escaped backslash followed by
/// `uFEFF` is indistinguishable from a plain `﻿`.
///
/// Kept byte-identical to `unescapeInput` in `ts/test/xml-spec.test.ts`
/// and `specUnescape` in `go/xml_test.go`.
pub fn spec_unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                'n' => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                'r' => {
                    out.push('\r');
                    i += 2;
                    continue;
                }
                't' => {
                    out.push('\t');
                    i += 2;
                    continue;
                }
                '\\' => {
                    out.push('\\');
                    i += 2;
                    continue;
                }
                'u' if i + 5 < chars.len() => {
                    let hex: String = chars[i + 2..i + 6].iter().collect();
                    if hex.chars().all(|h| h.is_ascii_hexdigit()) {
                        if let Some(decoded) =
                            u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            out.push(decoded);
                            i += 6;
                            continue;
                        }
                    }
                }
                _ => {}
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The JSON text of an engine value, for short assertions.
pub fn json(value: &Value) -> String {
    value.to_json().to_string()
}
