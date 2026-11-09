// The engine's error carries a code, position, hint and a formatted
// report, so `Result<_, TabnasError>` trips clippy's `result_large_err`;
// an integration test is a crate of its own, so it allows it again.
#![allow(clippy::result_large_err)]

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole. `make()` declares `lex.window`: the `xmltag` matcher
// reads the source only through the lexer's window, and reads the window
// on where a construct runs past its end, so a reader is read a window at
// a time.
//
// Every row of the shared fixtures and every document of the W3C XML
// Conformance Test Suite is parsed whole and from a reader that hands its
// bytes over in pieces of 1, 3 and 64 bytes, under the engine's window and
// under windows whose margin and block are a few characters, so that every
// construct meets the window's edge; a long document, whose long tokens of
// every kind run past the engine's window, in pieces of 4 KiB too. The
// value, the tokens the lexer gives, the rule passes, and an error's code,
// position, row, column, token and length must be the same. An error's
// excerpt is not compared: a parse from a reader shows a long line only as
// far back as the window reaches (tabnas/parser's DIVERGENCE.md).
//
// ts/test/window.test.ts and go/window_test.go hold the TypeScript and Go
// ports to the same.

mod common;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tabnas::{RuleState, Tabnas, TabnasError, Value, WindowSizes};
use tabnas_support::{load_spec_dir, SpecOptions};
use tabnas_xml::{decode_bom, XmlOptions};

use common::{corpus, spec_dir, spec_unescape};

/// A reader that hands its bytes over at most `size` at a time.
struct Pieces<'a> {
    bytes: &'a [u8],
    at: usize,
    size: usize,
}

impl Read for Pieces<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.size.min(buf.len()).min(self.bytes.len() - self.at);
        buf[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// A read of a source: the pieces a reader hands its bytes over in, and the
/// window it is read through, the engine's own (64 KiB) where it is `None`,
/// or one whose margin and block are a few characters, which the hidden
/// `parse_reader_with_sizes` sets, its reads as small as the pieces.
type Reading = (usize, Option<(usize, usize)>);

const PIECES: [usize; 3] = [1, 3, 64];
const WINDOWS: [Option<(usize, usize)>; 4] = [None, Some((1, 1)), Some((3, 2)), Some((64, 64))];

/// A fixture row is read in every pieces size through every window.
fn row_reads() -> Vec<Reading> {
    PIECES
        .iter()
        .flat_map(|&size| WINDOWS.iter().map(move |&window| (size, window)))
        .collect()
}

/// A conformance document in pieces of 1, 3 and 64 bytes through the
/// engine's window, and through windows of 1 and 1, 3 and 2, and 64 and 64
/// characters in pieces of 1, 3 and 64.
const DOCUMENT_READS: [Reading; 6] = [
    (1, None),
    (3, None),
    (64, None),
    (1, Some((1, 1))),
    (3, Some((3, 2))),
    (64, Some((64, 64))),
];

/// A long document in pieces of 1, 3, 64 and 4096 bytes through the
/// engine's window, whose edges its long tokens straddle, and through a
/// window of 64 and 64 characters.
const LONG_READS: [Reading; 5] = [
    (1, None),
    (3, None),
    (64, None),
    (4096, None),
    (64, Some((64, 64))),
];

type Log = Arc<Mutex<(Vec<String>, Vec<String>)>>;

/// `parser` with a log of the tokens the lexer gives and the rule passes.
fn logged(mut parser: Tabnas, log: &Log) -> Tabnas {
    let tokens = Arc::clone(log);
    parser.subscribe_tokens(move |token| {
        tokens.lock().unwrap().0.push(format!(
            "{} {:?} {} {} {}:{}",
            token.tin, token.src, token.val, token.site.pos, token.site.ri, token.site.ci
        ));
    });
    let rules = Arc::clone(log);
    parser.subscribe_rule_done(move |rule, _context, done| {
        let state = match done.state {
            RuleState::Open => "open",
            RuleState::Close => "close",
        };
        rules
            .lock()
            .unwrap()
            .1
            .push(format!("{} {state} {}", rule.name.as_str(), rule.d));
    });
    parser
}

/// What a parse showed: its value, or its error without the excerpt; and
/// the tokens and rule passes `log` gathered, which is empty again after.
fn seen(log: &Log, parsed: Result<Value, TabnasError>) -> (String, Vec<String>, Vec<String>) {
    let outcome = match parsed {
        Ok(value) => format!("ok {value}"),
        Err(e) => format!(
            "err {} at {} {}:{} {:?} {} {:?} {:?} {:?} {:?}",
            e.code, e.pos, e.row, e.col, e.src, e.len, e.token, e.rule, e.rule_stack, e.expected
        ),
    };
    let (tokens, rules) = std::mem::take(&mut *log.lock().unwrap());
    (outcome, tokens, rules)
}

/// `text` parsed whole and from a reader in each of `reads`; the count of
/// reader parses compared.
fn differential(parser: &Tabnas, log: &Log, at: &str, text: &str, reads: &[Reading]) -> usize {
    let whole = seen(log, parser.parse(text));
    for &(size, window) in reads {
        let reader = Pieces {
            bytes: text.as_bytes(),
            at: 0,
            size,
        };
        let parsed = match window {
            None => parser.parse_reader(reader),
            Some((margin, block)) => parser.parse_reader_with_sizes(
                reader,
                WindowSizes {
                    margin,
                    block,
                    chunk: size,
                },
            ),
        };
        let read = seen(log, parsed);
        assert_eq!(read, whole, "{at} in pieces of {size}, window {window:?}");
    }
    reads.len()
}

/// The parser a fixture row asks for, built by `make_with`.
fn row_parser(opts: &str, log: &Log) -> Tabnas {
    let options = if opts.trim().is_empty() {
        XmlOptions::default()
    } else {
        let json: serde_json::Value = serde_json::from_str(opts).expect("the opts column is JSON");
        XmlOptions::from_value(&Value::from_json(&json))
    };
    logged(tabnas_xml::make_with(&options), log)
}

/// Every `.xml` file under `dir`, in a stable order.
fn xml_files(dir: &Path, into: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            xml_files(&path, into);
        } else if path.extension().is_some_and(|ext| ext == "xml") {
            into.push(path);
        }
    }
}

/// A document of every kind of token, the long ones longer than the
/// engine's 64 KiB window, and characters past ASCII among them, so that
/// tokens of every kind straddle the window's edges as a reader hands the
/// text over. The same document as `longDocument` in
/// ts/test/window.test.ts and go/window_test.go.
fn long_document() -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push("\u{FEFF}<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n".into());
    parts.push("<!DOCTYPE doc [\n".into());
    parts.push("  <!ENTITY who \"w\u{f6}rld\">\n  <!ATTLIST x:item kind CDATA \"plain\">\n".into());
    for i in 0..1300 {
        parts.push(format!(
            "  <!ENTITY e{i} \"entity \u{20ac} {i}\"> <!-- {i} > ] ' -->\n"
        ));
    }
    parts.push("]>\n".into());
    parts.push(format!(
        "<!-- {}-->\n",
        "a comment past the window, \u{e9} \u{1d11e} - ".repeat(2000)
    ));
    parts.push(format!(
        "<doc xmlns=\"urn:doc\" xmlns:x=\"urn:x\" note=\"{}\">\n",
        "an attribute value past the window &amp; \u{fc} ".repeat(1600)
    ));
    for i in 0..1000 {
        parts.push(format!(
            "  <x:item n=\"{i}\" x:at='{}'>&who; &#{}; &#x{:x}; t\u{eb}xt {i}</x:item>\r\n",
            i % 7,
            60 + (i % 40),
            0x4e00 + i
        ));
        if i == 500 {
            parts.push(format!(
                "  <![CDATA[{}]]>\n",
                "cdata <b>not markup</b> \u{e7} ]] > ".repeat(2200)
            ));
            parts.push(format!(
                "  <?pi {}?>\n",
                "an instruction past the window \u{f1} ? > ".repeat(1800)
            ));
            parts.push(format!(
                "  <text>{}</text>\n",
                "character data past the window, \u{e5} &amp; &lt;x&gt; \r\n".repeat(1300)
            ));
            parts.push(format!("  <e{}/>\n", "n".repeat(70000)));
            parts.push("  <empty   a=\"1\"\n     b = \"2\"  />\n".into());
        }
    }
    parts.push("</doc>\n<!-- after -->\n".into());
    parts.concat()
}

#[test]
fn make_reads_a_reader_a_window_at_a_time() {
    assert!(tabnas_xml::make().options.lex.window);
    let strict = XmlOptions {
        strict_namespaces: true,
        ..XmlOptions::default()
    };
    assert!(tabnas_xml::make_with(&strict).options.lex.window);
    // The plugin itself declares nothing: a grammar built on it (feed)
    // declares the window on its own parser, once its own actions are
    // verified.
    let mut parser = Tabnas::new();
    parser
        .use_plugin(tabnas_xml::plugin(), None)
        .expect("the plugin installs");
    assert!(!parser.options.lex.window);
}

#[test]
fn every_fixture_row_parses_the_same_from_a_reader() {
    let log: Log = Arc::default();
    let files = load_spec_dir(spec_dir(), &SpecOptions::default()).expect("the fixtures load");
    let mut rows = 0;
    let mut compared = 0;
    for (index, file) in files.iter().enumerate() {
        for row in &file.rows {
            let input = spec_unescape(row.named("input"));
            let parser = row_parser(row.named("opts"), &log);
            compared += differential(&parser, &log, &row.location(), &input, &row_reads());
            rows += 1;
        }
        eprintln!(
            "window fixtures: {} of {} files ({}%), {compared} reader parses",
            index + 1,
            files.len(),
            (index + 1) * 100 / files.len()
        );
    }
    assert!(rows > 200, "{rows}");
    assert_eq!(compared, rows * row_reads().len());
}

#[test]
fn embed_mode_on_a_jsonic_parser_reads_a_reader_a_window_at_a_time_too() {
    // Embed mode is built on jsonic's parser, which declares no window, so
    // the window is declared here: the matcher is the same in both modes,
    // and is held to the same.
    let log: Log = Arc::default();
    let mut parser = tabnas_jsonic::make();
    parser
        .use_plugin(
            tabnas_xml::plugin(),
            Some(
                XmlOptions {
                    embed: true,
                    ..XmlOptions::default()
                }
                .to_value(),
            ),
        )
        .expect("embed mode installs on a jsonic parser");
    parser
        .set_options(|options| options.lex.window = true)
        .expect("lex.window sets");
    let parser = logged(parser, &log);
    let mut compared = 0;
    for input in [
        "{a: <x>1, 2</x>, b: [<y/>, <z q=\"r\">s</z>]}",
        "a: <p>T&amp;J <b>bold</b> <![CDATA[<raw>]]> <!-- c --></p>",
        "[<a/>, <b></b>, 3]",
        "{a: <x>unclosed}",
        "{a: <x></y>}",
    ] {
        compared += differential(&parser, &log, input, input, &row_reads());
    }
    assert_eq!(compared, 5 * row_reads().len());
}

#[test]
fn every_document_of_the_w3c_conformance_suite_parses_the_same_from_a_reader() {
    let log: Log = Arc::default();
    let parser = logged(tabnas_xml::make(), &log);
    let root = corpus();
    let mut files = Vec::new();
    xml_files(root, &mut files);
    assert!(files.len() > 3000, "{}", files.len());
    let mut compared = 0;
    for (index, file) in files.iter().enumerate() {
        let bytes =
            fs::read(file).unwrap_or_else(|error| panic!("read {}: {error}", file.display()));
        // The corpus mixes UTF-8, UTF-16 and UTF-32; `decode_bom`
        // transcodes, as the conformance suite reads each document.
        let text = decode_bom(&bytes);
        let at = file
            .strip_prefix(root)
            .unwrap_or(file)
            .display()
            .to_string();
        compared += differential(&parser, &log, &at, &text, &DOCUMENT_READS);
        if index % 250 == 0 || index + 1 == files.len() {
            eprintln!(
                "window conformance: {} of {} documents ({}%), {compared} reader parses",
                index + 1,
                files.len(),
                (index + 1) * 100 / files.len()
            );
        }
    }
}

#[test]
fn a_long_document_parses_the_same_from_a_reader() {
    let log: Log = Arc::default();
    let parser = logged(tabnas_xml::make(), &log);
    let text = long_document();
    assert!(text.len() > 512 * 1024, "{}", text.len());
    let value = parser.parse(&text).expect("the long document parses");
    log.lock().unwrap().0.clear();
    log.lock().unwrap().1.clear();
    assert!(
        value.to_json()["name"] == "doc",
        "{}",
        &value.to_string()[..80]
    );
    assert_eq!(
        differential(&parser, &log, "the long document", &text, &LONG_READS),
        LONG_READS.len()
    );
}

#[test]
fn a_long_document_fails_the_same_from_a_reader() {
    // A construct that runs past the engine's window and is then refused,
    // or never ends: the error is the whole text's, code, place and token.
    let log: Log = Arc::default();
    let parser = logged(tabnas_xml::make(), &log);
    let pad = format!("<p>{}</p>", "x\u{e9} ".repeat(30000));
    let bad = [
        format!("<a>{pad}<!-- {}", "c ".repeat(40000)),
        format!("<a>{pad}<![CDATA[{}", "d ".repeat(40000)),
        format!("<a>{pad}<?pi {}", "p ".repeat(40000)),
        format!(
            "<!DOCTYPE a [{}<a>{pad}</a>",
            "<!ENTITY e \"v\">".repeat(6000)
        ),
        format!("<a>{pad}{}]]>{}</a>", "t ".repeat(40000), "t ".repeat(10)),
        format!("<a>{pad}<b c=\"{}<\"/></a>", "v ".repeat(40000)),
        format!("<a>{pad}<b c=\"{}", "v ".repeat(40000)),
        format!("<a>{pad}<!-- {}-- -->", "c ".repeat(40000)),
        format!("<a>{pad}</b>"),
        format!("<a>{pad}</a>{}", "trailing ".repeat(10000)),
    ];
    let mut compared = 0;
    for (index, text) in bad.iter().enumerate() {
        let whole = seen(&log, parser.parse(text));
        assert!(
            whole.0.starts_with("err "),
            "{}",
            &whole.0[..200.min(whole.0.len())]
        );
        compared += differential(
            &parser,
            &log,
            &format!("long error document {index}"),
            text,
            &LONG_READS,
        );
    }
    assert_eq!(compared, bad.len() * LONG_READS.len());
}
