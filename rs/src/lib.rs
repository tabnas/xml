/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

// The engine's error carries a code, position, hint and a formatted
// report, so it is large by design and `Result<_, TabnasError>` trips
// clippy's `result_large_err`. The engine allows the lint at its own crate
// root for the same reason; boxing here would make `parse` return a
// different shape from `Tabnas::parse` and from the other two ports.
#![allow(clippy::result_large_err)]

//! The XML grammar plugin for the `tabnas` parsing engine.
//!
//! Parses XML text into a tree of elements: attributes, mixed content,
//! namespaces, entities (the five predefined plus numeric character
//! references, custom entities, and DOCTYPE `<!ENTITY>` declarations),
//! CDATA sections, comments, processing instructions and DOCTYPE
//! declarations (including `<!ATTLIST>` attribute defaults and
//! `xml:space` / `xml:lang` inheritance).
//!
//! ```
//! let value = tabnas_xml::parse(r#"<greeting lang="en">Hi <b>world</b></greeting>"#)?;
//! assert_eq!(
//!     value.to_string(),
//!     r#"{"name":"greeting","localName":"greeting","attributes":{"lang":"en"},"children":["Hi ",{"name":"b","localName":"b","attributes":{},"children":["world"]}]}"#
//! );
//! # Ok::<(), tabnas_xml::XmlError>(())
//! ```
//!
//! The plugin is layered on the relaxed-JSON grammar of
//! [`tabnas_jsonic`], as the canonical plugin is layered on
//! `@tabnas/jsonic`: `use(jsonic)` first, then `use(Xml)`. In the default
//! pure-XML mode the XML rules replace the JSON value rules; in embed
//! mode (`embed: true`) they sit beside them so an XML element can appear
//! wherever a jsonic value can.
//!
//! TypeScript is canonical: `ts/src/xml.ts` defines behaviour, option
//! names and defaults, and `xml-grammar.jsonic` at the repository root
//! defines the rule chain, embedded below as JSON. The shared fixtures in
//! `test/spec/*.tsv` are the parity contract across TypeScript, Go and
//! Rust.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;
use tabnas::{
    Context, LexMatcher, Options, Plugin, PluginError, Rule, Tabnas, Token, Value, TIN_CM, TIN_LN,
    TIN_SP,
};

mod bom;
mod entity;
mod lex;
mod namespace;

pub use bom::{decode_bom, strip_bom};

/// This crate's version. It MUST equal `ts/package.json` "version": the
/// release orchestrator rewrites both, and `tests/version_test.rs` fails
/// the build if they drift. Mirrors `VERSION` in `ts/src/xml.ts` and
/// `const VERSION` in `go/xml.go`.
pub const VERSION: &str = "0.7.9";

/// The README's Rust examples run as doctests, so a stale one fails the
/// gate rather than misleading the reader. Its `toml` and `bash` fences
/// are skipped; rustdoc runs only the `rust` ones.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

/// The error a failed parse produces, re-exported so callers need not
/// depend on the engine crate directly.
pub use tabnas::TabnasError as XmlError;

// --- BEGIN EMBEDDED xml-grammar.jsonic ---
/// The grammar, as the JSON that `xml-grammar.jsonic` at the repository
/// root parses to. The TypeScript plugin parses that file's text with
/// jsonic at load time; this crate carries the parsed form so it needs no
/// jsonic parse of its own to start, and `tests/xml_test.rs` holds the two
/// to the same value.
pub const GRAMMAR_TEXT: &str = r##"
{
  "rule": {
    "xml": {
      "open": [
        {
          "s": "#ZZ"
        },
        {
          "s": "#TX",
          "r": "xml",
          "a": "@doc-text-open"
        },
        {
          "p": "element",
          "c": "@no-root-yet"
        }
      ],
      "close": [
        {
          "s": "#ZZ",
          "g": "end"
        },
        {
          "s": "#TX",
          "r": "xml",
          "a": "@doc-text-close",
          "g": "comma"
        }
      ]
    },
    "element": {
      "open": [
        {
          "s": "#XSC",
          "a": "@element-selfclose",
          "u": {
            "selfclose": 1
          }
        },
        {
          "s": "#XOP",
          "p": "content",
          "a": "@element-open"
        }
      ],
      "close": [
        {
          "c": "@element-is-selfclosed"
        },
        {
          "s": "#XCL",
          "a": "@element-close",
          "g": "close"
        }
      ]
    },
    "content": {
      "open": [
        {
          "s": "#XCL",
          "b": 1
        },
        {
          "p": "child"
        }
      ],
      "close": [
        {
          "s": "#XCL",
          "b": 1,
          "g": "close"
        },
        {
          "r": "content"
        }
      ]
    },
    "child": {
      "open": [
        {
          "s": "#TX",
          "a": "@child-text"
        },
        {
          "s": "#XOP",
          "b": 1,
          "p": "element"
        },
        {
          "s": "#XSC",
          "b": 1,
          "p": "element"
        }
      ]
    }
  }
}"##;
// --- END EMBEDDED xml-grammar.jsonic ---

/// The name the plugin registers under, and so the namespace of its
/// plugin options.
const PLUGIN_NAME: &str = "xml";

// The function references the grammar names, registered on the instance
// before the document is installed. The two lifecycle hooks are wired by
// name to their rule and phase when the rule is installed.
const XML_BC: &str = "@xml-bc";
const CHILD_BC: &str = "@child-bc";
const ELEMENT_BC: &str = "@element-bc";
const NO_ROOT_YET: &str = "@no-root-yet";
const DOC_TEXT_OPEN: &str = "@doc-text-open";
const DOC_TEXT_CLOSE: &str = "@doc-text-close";
const ELEMENT_OPEN: &str = "@element-open";
const ELEMENT_SELFCLOSE: &str = "@element-selfclose";
const ELEMENT_CLOSE: &str = "@element-close";
const CHILD_TEXT: &str = "@child-text";
const ELEMENT_IS_SELFCLOSED: &str = "@element-is-selfclosed";

/// Plugin options. [`Default`] is the canonical default set: namespaces
/// resolved, entities decoded, no custom entities, undeclared entities an
/// error, unbound prefixes accepted, pure-XML mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlOptions {
    /// Resolve namespaces: annotate elements with `prefix`, `localName`
    /// and `namespace`. Default: true.
    pub namespaces: bool,
    /// Decode the five predefined entities and numeric character
    /// references in character data. Default: true.
    pub entities: bool,
    /// Additional named entities beyond the five predefined ones.
    pub custom_entities: IndexMap<String, String>,
    /// Enforce XML 1.0 4.1: every named entity reference must resolve to
    /// a declared entity (predefined, custom, or a DOCTYPE `<!ENTITY>`
    /// declaration). Default: true. When off, a reference to an unknown
    /// name is left as it is in the output.
    pub strict_entities: bool,
    /// Enforce the Namespaces in XML 1.0 rule that every prefix on an
    /// element or attribute name be bound by an in-scope `xmlns:prefix`
    /// declaration. Default: false. XML 1.0 well-formedness and
    /// Namespaces in XML are separate specifications: `<a><foo:b/></a>`
    /// is well-formed XML, merely not namespace-well-formed, so an
    /// unbound prefix is reported only when this is on. Either way the
    /// element keeps its `prefix` and `localName`; an unbound prefix
    /// leaves `namespace` unset.
    pub strict_namespaces: bool,
    /// Embed mode. When false (the default) the parser is configured for
    /// pure-XML input: `xml` becomes the start rule, the JSON structural
    /// tokens are dropped and every non-XML lexer is switched off. When
    /// true the jsonic grammar stays in place and an alternate is added
    /// to its `val` rule so a literal element (`<tag>...</tag>` or
    /// `<tag/>`) appears wherever jsonic expects a value.
    pub embed: bool,
}

impl Default for XmlOptions {
    fn default() -> Self {
        XmlOptions {
            namespaces: true,
            entities: true,
            custom_entities: IndexMap::new(),
            strict_entities: true,
            strict_namespaces: false,
            embed: false,
        }
    }
}

impl XmlOptions {
    /// Read an option bag the way `ts/src/xml.ts` reads it: each field on
    /// its own terms (`namespaces !== false`, `embed === true`, and so
    /// on), so one field of the wrong type leaves only that field at its
    /// default. The bag is the JSON shape a fixture's `opts` column
    /// carries and the shape [`plugin`] receives.
    pub fn from_value(value: &Value) -> Self {
        let bag = value.to_json();
        let field = |name: &str| bag.get(name);
        let not_false = |name: &str| field(name) != Some(&serde_json::Value::Bool(false));
        let is_true = |name: &str| field(name) == Some(&serde_json::Value::Bool(true));
        // Read the replacements from the engine value, not from the JSON
        // projection: `to_json` spells `undefined`, `NaN` and an infinity
        // all three `null`, and the canonical coercion tells them apart.
        let custom_entities = custom_entities(value);
        XmlOptions {
            namespaces: not_false("namespaces"),
            entities: not_false("entities"),
            custom_entities,
            strict_entities: not_false("strictEntities"),
            strict_namespaces: is_true("strictNamespaces"),
            embed: is_true("embed"),
        }
    }

    /// The option bag, with the canonical camel-case keys.
    pub fn to_value(&self) -> Value {
        let mut custom = IndexMap::new();
        for (name, text) in &self.custom_entities {
            custom.insert(name.clone(), Value::String(text.clone()));
        }
        let mut bag = IndexMap::new();
        bag.insert("namespaces".to_string(), Value::Bool(self.namespaces));
        bag.insert("entities".to_string(), Value::Bool(self.entities));
        bag.insert("customEntities".to_string(), Value::object(custom));
        bag.insert(
            "strictEntities".to_string(),
            Value::Bool(self.strict_entities),
        );
        bag.insert(
            "strictNamespaces".to_string(),
            Value::Bool(self.strict_namespaces),
        );
        bag.insert("embed".to_string(), Value::Bool(self.embed));
        Value::object(bag)
    }
}

/// The `customEntities` replacements of a loose option bag.
///
/// The canonical plugin hands each replacement straight back from the
/// `String.prototype.replace` callback in `buildEntityDecoder`, so
/// JavaScript coerces it with `ToString`: an object becomes
/// `[object Object]`, an array joins its elements with commas, and a
/// number spells itself the way `Number::toString` does. [`js_string`]
/// is that coercion.
///
/// `undefined` is the one value the callback never coerces: the guard
/// there is `undefined !== baseEntities[ref]`, so the name counts as
/// declared (no `undeclared_entity`) while the reference is left in the
/// text exactly as written. The replacement text recorded here is that
/// verbatim reference, which reads back identically. The one input where
/// the two part company is a name that a DOCTYPE internal subset also
/// declares: the canonical decoder falls through to the DTD value, while
/// this map answers first. A `String` replacement cannot express "no
/// replacement", and [`XmlOptions::custom_entities`] is a map of them.
fn custom_entities(bag: &Value) -> IndexMap<String, String> {
    // A bag built by the engine's plugin merge is an `Object`; one handed
    // in from a parse result may still be the `MapRef` the parse built,
    // and every other field here reads that shape through `to_json`.
    let fields = match bag {
        Value::Object(fields) => &**fields,
        Value::MapRef(map) => &map.value,
        _ => return IndexMap::new(),
    };
    let entries = match fields.get("customEntities") {
        Some(Value::Object(entries)) => &**entries,
        Some(Value::MapRef(map)) => &map.value,
        _ => return IndexMap::new(),
    };
    entries
        .iter()
        .map(|(name, replacement)| {
            let text = match replacement {
                Value::String(text) => text.clone(),
                Value::Text(text) => text.string.clone(),
                Value::Undefined => format!("&{name};"),
                other => js_string(other),
            };
            (name.clone(), text)
        })
        .collect()
}

/// `String(value)` as JavaScript spells it.
///
/// Ported from the same helper in the yaml and csv crates. An array
/// joins its elements with commas, rendering `null` and `undefined` as
/// nothing at all (`Array.prototype.join`), and any object is the bare
/// `[object Object]`, since a value parsed out of an option bag carries
/// no `toString` of its own.
fn js_string(value: &Value) -> String {
    match value {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => js_number_to_string(*number),
        Value::String(text) => text.clone(),
        Value::Text(text) => text.string.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Undefined | Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::ListRef(list) => js_string(&Value::array(list.value.clone())),
        Value::Object(_) | Value::MapRef(_) => "[object Object]".to_string(),
    }
}

/// `String(number)` as JavaScript spells it (ECMA-262 6.1.6.1.20): the
/// shortest digit string that reads back as the same double, in plain
/// decimal while the decimal point stays inside `(-6, 21]` and in
/// exponent form outside it.
///
/// Rust's own formatting differs in three ways that reach a replacement
/// text: it keeps the sign of negative zero, it never switches to
/// exponent form, and a large integral float prints its exact binary
/// value rather than its shortest round-tripping digits. Ported from
/// `js_number_to_string` in the csv and yaml crates.
fn js_number_to_string(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    // Catches -0.0 as well: JavaScript spells both zeros "0".
    if number == 0.0 {
        return "0".to_string();
    }
    if number < 0.0 {
        return format!("-{}", js_number_to_string(-number));
    }
    if number.is_infinite() {
        return "Infinity".to_string();
    }

    // The specification wants the shortest digit string `s` that round
    // trips (length `k`), and `n`, the position of the decimal point
    // relative to it. Rust's `{:e}` yields digits of exactly that length.
    let shortest = format!("{number:e}");
    let shortest_k = shortest
        .split_once('e')
        .map(|(mantissa, _)| mantissa.chars().filter(char::is_ascii_digit).count())
        .expect("a finite f64 always formats with an exponent");

    // Re-render to that same length to settle a tie. Where two digit
    // strings of length `k` are equally close to `number`, the
    // specification takes the one ending in an even digit; Rust's
    // shortest form does not, but its exactly-rounded fixed-precision
    // form does.
    let exponential = format!("{:.*e}", shortest_k - 1, number);
    let (mantissa, exponent) = exponential
        .split_once('e')
        .expect("a finite f64 always formats with an exponent");
    // Rounding can leave trailing zeros (and, on a carry, one digit too
    // many); dropping them keeps `s` shortest, which is what `k` means.
    let digits = mantissa
        .chars()
        .filter(|digit| *digit != '.')
        .collect::<String>();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let k = digits.len() as i32;
    let n = exponent
        .parse::<i32>()
        .expect("a formatted exponent is an integer")
        + 1;

    // The four cases of the specification, in its order. The range bounds
    // are `k <= n <= 21`, `0 < n <= 21` and `-6 < n <= 0`.
    if (k..=21).contains(&n) {
        // Integral, with n - k trailing zeros to restore.
        let mut text = digits.to_string();
        text.push_str(&"0".repeat((n - k) as usize));
        text
    } else if (1..=21).contains(&n) {
        let point = n as usize;
        format!("{}.{}", &digits[..point], &digits[point..])
    } else if (-5..=0).contains(&n) {
        format!("0.{}{}", "0".repeat(-n as usize), digits)
    } else {
        // Exponent form. `n - 1` is never 0 here, so the sign is never "+0".
        let sign = if n - 1 < 0 { '-' } else { '+' };
        let power = (n - 1).abs();
        if k == 1 {
            format!("{digits}e{sign}{power}")
        } else {
            format!("{}.{}e{sign}{power}", &digits[..1], &digits[1..])
        }
    }
}

// ---------------------------------------------------------------------------
// Node helpers
// ---------------------------------------------------------------------------

/// Assign a rule's node: the Rust spelling of TypeScript `r.node = v`.
///
/// A pushed or replaced rule SHARES its parent's node cell, so writing
/// through `rule.node.borrow_mut()` would overwrite the parent's node too.
/// Assigning installs a fresh cell instead; the `content` and `child`
/// rules an element pushes then share THAT cell, which is what lets them
/// append to the element's children in place.
fn set_node(rule: &mut Rule, value: Value) {
    rule.node = Rc::new(RefCell::new(value));
}

fn flag(rule: &Rule, name: &str) -> bool {
    matches!(rule.u.get(name), Some(Value::Bool(true)))
}

fn token_string(token: Option<&Token>) -> Option<String> {
    match token.map(|token| &token.val) {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    }
}

/// The element an open or self-closing tag token describes: `name`,
/// `localName`, `attributes` (with the DOCTYPE defaults filled in) and an
/// empty `children` list, in that order.
fn element_of(token: Option<&Token>, context: &Context) -> Value {
    let (name, attributes) = match token.map(|token| &token.val) {
        Some(Value::Object(tag)) => {
            let name = match tag.get("name") {
                Some(Value::String(name)) => name.clone(),
                _ => String::new(),
            };
            let attributes = match tag.get("attributes") {
                Some(Value::Object(attributes)) => (**attributes).clone(),
                _ => IndexMap::new(),
            };
            (name, attributes)
        }
        _ => (String::new(), IndexMap::new()),
    };
    let attributes = apply_attr_defaults(attributes, &name, context);
    let mut element = IndexMap::new();
    element.insert("name".to_string(), Value::String(name.clone()));
    element.insert("localName".to_string(), Value::String(name));
    element.insert("attributes".to_string(), Value::object(attributes));
    element.insert("children".to_string(), Value::array(Vec::new()));
    Value::object(element)
}

/// Fill in DOCTYPE-supplied default attribute values (`<!ATTLIST element
/// attr ... "default">`) for any attribute the element does not carry.
fn apply_attr_defaults(
    mut attributes: IndexMap<String, Value>,
    element: &str,
    context: &Context,
) -> IndexMap<String, Value> {
    if let Some(defaults) = lex::dtd_attr_defaults(context, element) {
        for (name, value) in defaults {
            if !attributes.contains_key(&name) {
                attributes.insert(name, value);
            }
        }
    }
    attributes
}

/// Append to the element's `children`.
fn push_child(node: &mut Value, child: Value) {
    if let Some(children) = node
        .as_object_mut()
        .and_then(|element| element.get_mut("children"))
        .and_then(Value::as_array_mut)
    {
        children.push(child);
    }
}

/// XML 1.0 2.1 [1] `document ::= prolog element Misc*`: at document level
/// only Misc (comments, PIs, white space) may appear, so character data
/// before or after the root element is not well-formed. Comments, PIs and
/// the DOCTYPE arrive as `#XIG` and are ignored by the token set, so the
/// only document-level token to police is `#TX`.
///
/// The token policed here is one of the few this plugin does not mint:
/// outside the root element the matcher claims nothing, so the engine's
/// own text matcher cuts it, and its column has not been through
/// [`lex::discount_bom`] yet.
fn check_doc_text(token: Option<&Token>, context: &Context) -> Option<Token> {
    let token = token?;
    let Value::String(text) = &token.val else {
        return None;
    };
    if text
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        return None;
    }
    let mut bad = token.clone();
    lex::discount_bom(context, &mut bad.site);
    bad.bad("text_at_top_level");
    Some(bad)
}

// ---------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------

/// Install the XML grammar on `parser`, which should already carry the
/// jsonic grammar (as [`make`] arranges): the port of the `Xml` plugin
/// function.
///
/// In pure mode the parser is reconfigured for XML alone: `xml` becomes
/// the start rule, the JSON structural tokens and every non-XML lexer are
/// switched off, and jsonic's value rules are removed. In embed mode the
/// jsonic grammar stays and XML elements become values. Installation is
/// idempotent: an instance that already carries the `xml` rule is left
/// alone.
///
/// ```
/// let mut parser = tabnas_jsonic::make();
/// tabnas_xml::xml(&mut parser, &tabnas_xml::XmlOptions::default())?;
/// assert_eq!(
///     parser.parse("<a x='1'/>")?.to_string(),
///     r#"{"name":"a","localName":"a","attributes":{"x":"1"},"children":[]}"#
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn xml(parser: &mut Tabnas, options: &XmlOptions) -> Result<(), PluginError> {
    if parser.rules.contains_key("xml") {
        return Ok(());
    }
    let embed = options.embed;
    let namespaces = options.namespaces;
    let strict_namespaces = options.strict_namespaces;

    // Reserve the XML tokens so they have stable identities before the
    // grammar references them; the matcher closes over them.
    let tokens = lex::Tokens {
        xig: parser.token("#XIG"),
        xop: parser.token("#XOP"),
        xcl: parser.token("#XCL"),
        xsc: parser.token("#XSC"),
    };
    let state = Arc::new(lex::MatcherState {
        decoder: entity::EntityDecoder::new(&options.custom_entities),
        entities: options.entities,
        strict: options.strict_entities,
        tokens,
    });
    let matcher = lex::matcher(state);

    parser.set_options(|o: &mut Options| {
        // The custom matcher runs first, ahead of every engine matcher.
        o.lex.matchers.insert(
            "xmltag".to_string(),
            LexMatcher {
                name: "xmltag".to_string(),
                order: 100_000.0,
                matcher: None,
                imperative: Some(matcher),
                factory: None,
            },
        );
        o.lex.empty_result = Value::Undefined;
        // Terminate jsonic text at `<` so a tag start is never absorbed
        // into a text run.
        o.ender = vec!["<".to_string()];

        if !embed {
            // Pure XML mode: the jsonic value grammar is unreachable and
            // every lexer other than the tag matcher is quiescent. No
            // `text.modify` hook: while the root element is open the tag
            // matcher emits the text tokens itself, and the engine's text
            // matcher only sees white space before and after the root.
            o.rule.start = "xml".to_string();
            o.rule.exclude = "jsonic,imp".to_string();
            for name in ["#OB", "#CB", "#OS", "#CS", "#CL", "#CA"] {
                o.fixed.tokens.shift_remove(name);
            }
            o.number.lex = false;
            o.value.lex = false;
            o.string.lex = false;
            o.comment.lex = false;
            o.space.lex = false;
            o.line.lex = false;
            // XML 1.0 2.1: a well-formed document has exactly ONE document
            // element. Input that parses to no value at all (a prolog on
            // its own, a lone comment, white space) satisfies every other
            // rule and then yields nothing, which a caller cannot tell
            // from a successful parse of nothing. It is the
            // well-formedness error it is. Both spellings of "no value"
            // are listed: which one a no-value parse lands on depends on
            // whether the start rule ran at all.
            o.result.fail = vec![Value::Undefined, Value::Null];
            // ...and the same rule makes empty input ill-formed. The
            // engine short-circuits `""` before the rule loop, so
            // `result.fail` never sees it; `lex.empty` governs that path.
            o.lex.empty = false;
        }

        // Drop `#XIG` (comments, PIs, DOCTYPE) with the default ignored
        // tokens. In pure mode the space, line and comment tokens are
        // never produced, but listing them is harmless.
        o.token_set.insert(
            "IGNORE".to_string(),
            vec![TIN_SP, TIN_LN, TIN_CM, tokens.xig],
        );

        // Error templates and hints, in both modes. A template
        // interpolates `{key}` against the failing token's details.
        for (code, message) in ERROR_MESSAGES {
            o.error.insert((*code).to_string(), (*message).to_string());
        }
        for (code, hint) in ERROR_HINTS {
            o.hint.insert((*code).to_string(), (*hint).to_string());
        }
    })?;

    register_refs(parser, namespaces, strict_namespaces, embed);

    parser
        .grammar_json(GRAMMAR_TEXT)
        .map_err(|error| PluginError(format!("xml: apply grammar: {error}")))?;

    if embed {
        // Splice XML literals into the jsonic `val` rule: on `#XOP` or
        // `#XSC`, push `element`, backtracking by one so `element.open`
        // reads the same token and dispatches. A bare alternate list is
        // prepended, as the canonical `rs.open([...])` prepends.
        let splice = serde_json::json!({
            "rule": {
                "val": {
                    "open": [
                        { "s": "#XOP", "b": 1, "p": "element", "g": "xml" },
                        { "s": "#XSC", "b": 1, "p": "element", "g": "xml" },
                    ],
                },
            },
        });
        let splice = tabnas::GrammarSpec::from_value(splice)
            .map_err(|error| PluginError(format!("xml: embed grammar: {error}")))?;
        parser
            .grammar(&splice)
            .map_err(|error| PluginError(format!("xml: embed grammar: {error}")))?;
    } else {
        // Pure XML mode: the `xml` start rule reaches only the XML rules,
        // so jsonic's value rules are dead. Remove them, so the parser
        // (and a railroad diagram of it) carries only the rules XML uses.
        for name in ["val", "map", "list", "pair", "elem"] {
            parser.remove_rule(name);
        }
    }
    Ok(())
}

/// Register every function reference the grammar names.
fn register_refs(parser: &mut Tabnas, namespaces: bool, strict_namespaces: bool, embed: bool) {
    // The root element lands on the `xml` rule's child; copy it to the
    // document node, mark the root as seen so `@no-root-yet` refuses a
    // second one, and resolve namespaces over the finished tree.
    parser.state_action_with_next_ref(XML_BC, move |rule, context, _next, _out| {
        if rule.child_node.is_undefined() {
            return Ok(None);
        }
        let mut root = rule.child_node.clone();
        context.u.insert("rootSeen".to_string(), Value::Bool(true));
        if namespaces {
            if let Err(code) = namespace::resolve_namespaces(&mut root, strict_namespaces) {
                // The canonical writes `ctx.t0.bad(nsErr)` here. Namespace
                // resolution runs at DOCUMENT CLOSE, where no token is
                // current, so the canonical's `t0` is the no-token
                // sentinel, and its report lands at the start of the
                // source (row 1, column 1). Marking the same sentinel
                // keeps the failure inside the engine's error machinery,
                // which is what renders the template registered for
                // `code`. An `ActionError` here would bypass that and
                // hand the caller a literal stand-in message instead.
                let mut token = context.t0().cloned().unwrap_or_else(Token::no_token);
                token.bad(code);
                return Ok(Some(token));
            }
        }
        // The start rule's cell IS the document node, so it is written
        // in place; `root.node = r.child.node` in the canonical grammar.
        *rule.node.borrow_mut() = root;
        Ok(None)
    });

    // Only let `xml` push an `element` while the document has produced
    // no root (XML 1.0 2.1).
    parser.alt_condition(NO_ROOT_YET, |_rule, context| {
        !matches!(context.u.get("rootSeen"), Some(Value::Bool(true)))
    });

    parser.action_with_match_ref(DOC_TEXT_OPEN, |rule, context, _matched| {
        Ok(check_doc_text(rule.o0(), context))
    });
    parser.action_with_match_ref(DOC_TEXT_CLOSE, |rule, context, _matched| {
        Ok(check_doc_text(rule.c0(), context))
    });

    parser.action_with_context(ELEMENT_OPEN, |rule, context| {
        let element = element_of(rule.o0(), context);
        set_node(rule, element);
        Ok(())
    });
    parser.action_with_context(ELEMENT_SELFCLOSE, |rule, context| {
        let element = element_of(rule.o0(), context);
        set_node(rule, element);
        Ok(())
    });

    // The close tag must name the open tag. The offending close tag
    // itself is marked (not the lookahead token) so the caret lands on
    // `</b>`, and the two names ride on the token as details, which is
    // where the `{openname}` / `{closename}` placeholders resolve from.
    parser.action_with_match_ref(ELEMENT_CLOSE, |rule, _context, _matched| {
        let open_name = match &*rule.node.borrow() {
            Value::Object(element) => match element.get("name") {
                Some(Value::String(name)) => name.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let close_name = token_string(rule.c0()).unwrap_or_default();
        if open_name == close_name {
            return Ok(None);
        }
        let Some(mut token) = rule.c0().cloned() else {
            return Ok(None);
        };
        token.bad_with_details(
            "xml_mismatched_tag",
            [
                ("openname".to_string(), Value::String(open_name)),
                ("closename".to_string(), Value::String(close_name)),
            ],
        );
        Ok(Some(token))
    });

    parser.action_with_context(CHILD_TEXT, |rule, _context| {
        let text = rule
            .o0()
            .map_or(Value::Undefined, |token| token.val.clone());
        push_child(&mut rule.node.borrow_mut(), text);
        rule.u_mut().insert("done".to_string(), Value::Bool(true));
        Ok(())
    });

    parser.state_action_ref(CHILD_BC, |rule, _context| {
        if flag(rule, "done") || rule.child_node.is_undefined() {
            return Ok(());
        }
        let child = rule.child_node.clone();
        push_child(&mut rule.node.borrow_mut(), child);
        Ok(())
    });

    parser.alt_condition(
        ELEMENT_IS_SELFCLOSED,
        |rule, _context| matches!(rule.u.get("selfclose"), Some(Value::Number(n)) if *n == 1.0),
    );

    // In embed mode the top-level wrapper is jsonic's `val` rule, so the
    // `@xml-bc` hook that resolves namespaces over the document never
    // runs. Resolve them instead when an element closes directly under a
    // `val`, once its whole subtree is in place.
    if embed && namespaces {
        parser.state_action_ref(ELEMENT_BC, move |rule, _context| {
            let under_val = rule
                .parent_rule
                .as_ref()
                .is_some_and(|parent| parent.name.as_str() == "val");
            if !under_val || !matches!(&*rule.node.borrow(), Value::Object(_)) {
                return Ok(());
            }
            let mut element = rule.node.borrow().clone();
            // The outcome is deliberately not an error here, as in the
            // canonical plugin: a fragment inside a jsonic document is
            // annotated as far as it can be.
            let _ = namespace::resolve_namespaces(&mut element, strict_namespaces);
            *rule.node.borrow_mut() = element;
            Ok(())
        });
    }
}

/// The plugin form of [`xml`], for [`Tabnas::use_plugin`]. Options are
/// read from the plugin option bag with [`XmlOptions::from_value`];
/// installed this way the grammar is re-applied to derived instances, as
/// every native plugin is.
///
/// ```
/// let mut parser = tabnas_jsonic::make();
/// parser.use_plugin(tabnas_xml::plugin(), None)?;
/// assert_eq!(
///     parser.parse("<a>&amp;</a>")?.to_string(),
///     r#"{"name":"a","localName":"a","attributes":{},"children":["&"]}"#
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn plugin() -> Plugin {
    Plugin::new(PLUGIN_NAME, |parser, options| {
        xml(parser, &XmlOptions::from_value(options))
    })
    .with_defaults(XmlOptions::default().to_value())
}

/// Build an XML parser with caller options: the jsonic grammar, then this
/// plugin, the `new Tabnas().use(jsonic).use(Xml, options)` of the
/// canonical plugin and the `jsonic.Make()` plus `UseDefaults` of Go.
///
/// Infallible by design: the grammar documents are fixed literals, so a
/// failure here is a bug in this crate rather than anything a caller did.
///
/// ```
/// let parser = tabnas_xml::make_with(&tabnas_xml::XmlOptions {
///     strict_entities: false,
///     ..Default::default()
/// });
/// assert_eq!(
///     parser.parse("<a>&nope;</a>")?.to_string(),
///     r#"{"name":"a","localName":"a","attributes":{},"children":["&nope;"]}"#
/// );
/// # Ok::<(), tabnas_xml::XmlError>(())
/// ```
pub fn make_with(options: &XmlOptions) -> Tabnas {
    let mut parser = tabnas_jsonic::make();
    parser
        .use_plugin(plugin(), Some(options.to_value()))
        .expect("the xml grammar documents are fixed and valid");
    parser
}

/// Build an XML parser with the default options.
///
/// Reuse the result: building the grammar dominates a parse.
///
/// ```
/// let parser = tabnas_xml::make();
/// assert_eq!(
///     parser.parse("<a><b/></a>")?.to_string(),
///     r#"{"name":"a","localName":"a","attributes":{},"children":[{"name":"b","localName":"b","attributes":{},"children":[]}]}"#
/// );
/// assert_eq!(parser.parse("<a></b>").unwrap_err().code, "xml_mismatched_tag");
/// # Ok::<(), tabnas_xml::XmlError>(())
/// ```
pub fn make() -> Tabnas {
    make_with(&XmlOptions::default())
}

/// Parse an XML document with the shared default parser.
///
/// The engine is built once, on first use, and reused after that; reuse
/// is safe because [`Tabnas::parse`] takes `&self` and builds a fresh
/// parse context per call, and `Tabnas` is `Send + Sync`. Use [`make`] or
/// [`make_with`] when the parser needs configuring.
///
/// ```
/// let value = tabnas_xml::parse("<a>Tom &amp; Jerry</a>")?;
/// assert_eq!(
///     value.to_string(),
///     r#"{"name":"a","localName":"a","attributes":{},"children":["Tom & Jerry"]}"#
/// );
/// assert_eq!(tabnas_xml::parse("<a/>junk").unwrap_err().code, "text_at_top_level");
/// # Ok::<(), tabnas_xml::XmlError>(())
/// ```
pub fn parse(src: &str) -> Result<Value, XmlError> {
    static DEFAULT: OnceLock<Tabnas> = OnceLock::new();
    DEFAULT.get_or_init(make).parse(src)
}

/// The error templates, interpolated against the failing token's
/// details. Kept identical to `ts/src/xml.ts`. `$name` is not a
/// placeholder syntax the engine understands, so it is never used here.
const ERROR_MESSAGES: &[(&str, &str)] = &[
    (
        "xml_mismatched_tag",
        "closing tag </{closename}> does not match opening tag <{openname}>",
    ),
    ("xml_invalid_tag", "invalid tag: {src}"),
    ("unterminated_comment", "unterminated comment: {src}"),
    ("unterminated_cdata", "unterminated CDATA section: {src}"),
    (
        "unterminated_pi",
        "unterminated processing instruction: {src}",
    ),
    (
        "unterminated_doctype",
        "unterminated DOCTYPE declaration: {src}",
    ),
    ("comment_double_dash", "comment body cannot contain \"--\""),
    (
        "cdata_terminator_in_text",
        "character data cannot contain \"]]>\"",
    ),
    (
        "pi_target_invalid",
        "processing instruction target is missing or invalid",
    ),
    (
        "lt_in_attr_value",
        "\"<\" is not allowed in an attribute value",
    ),
    (
        "bad_entity_ref",
        "malformed entity reference (need &name; or &#NNN; or &#xHHH;)",
    ),
    ("duplicate_attribute", "duplicate attribute name in tag"),
    ("invalid_xml_char", "illegal control character in XML data"),
    (
        "reserved_namespace",
        "invalid use of a reserved namespace prefix or URI",
    ),
    (
        "unbound_prefix",
        "element or attribute uses an undeclared namespace prefix",
    ),
    (
        "invalid_namespace_uri",
        "namespace name cannot contain white space",
    ),
    ("undeclared_entity", "reference to undeclared entity"),
    (
        "unparsed_entity_ref",
        "reference to an unparsed (NDATA) entity",
    ),
    (
        "external_entity_in_attr",
        "attribute value cannot reference an external entity",
    ),
    (
        "text_at_top_level",
        "character data is not allowed outside the root element",
    ),
];

/// The hint for each error code. Kept identical to `ts/src/xml.ts`.
const ERROR_HINTS: &[(&str, &str)] = &[
    (
        "xml_mismatched_tag",
        "Each opening tag must be paired with a matching closing tag.\nExpected </{openname}> but found </{closename}>.",
    ),
    ("xml_invalid_tag", "The tag syntax is not valid XML."),
    (
        "unterminated_comment",
        "The comment starting at this position has no closing \"-->\".",
    ),
    (
        "unterminated_cdata",
        "The CDATA section starting at this position has no closing \"]]>\".",
    ),
    (
        "unterminated_pi",
        "The processing instruction starting at this position has no closing \"?>\".",
    ),
    (
        "unterminated_doctype",
        "The DOCTYPE declaration starting at this position has no closing \">\".",
    ),
    (
        "comment_double_dash",
        "XML 1.0 disallows \"--\" inside a comment body.",
    ),
    (
        "cdata_terminator_in_text",
        "The literal \"]]>\" must only appear as the end of a CDATA section.",
    ),
    (
        "pi_target_invalid",
        "A processing instruction must start with a Name; the XML declaration <?xml...?> is the special case.",
    ),
    (
        "lt_in_attr_value",
        "Use the entity reference &lt; to include \"<\" in an attribute value.",
    ),
    (
        "bad_entity_ref",
        "Replace literal \"&\" with &amp;, or terminate the entity reference with \";\".",
    ),
    (
        "duplicate_attribute",
        "Each attribute name in an open tag must be unique.",
    ),
    (
        "invalid_xml_char",
        "Only #x9, #xA, #xD and code points >= #x20 are legal XML characters.",
    ),
    (
        "reserved_namespace",
        "The \"xml\" prefix is fixed to http://www.w3.org/XML/1998/namespace; the \"xmlns\" prefix cannot be redeclared, and neither URI may be bound to any other prefix or as the default namespace.",
    ),
    (
        "unbound_prefix",
        "Declare the prefix with xmlns:prefix=\"...\" on this element or one of its ancestors.",
    ),
    (
        "invalid_namespace_uri",
        "A namespace name is a URI reference, and a URI reference cannot contain white space; note that a line break inside the declaration becomes a space under attribute-value normalisation.",
    ),
    (
        "undeclared_entity",
        "Declare the entity in the DOCTYPE internal subset, add it to the customEntities option, or set strictEntities: false to allow unresolved references through.",
    ),
    (
        "unparsed_entity_ref",
        "XML 1.0 \u{a7}4.1 (WFC: Parsed Entity) \u{2014} an entity declared with an NDATA notation is unparsed, and its name may only appear as the value of an ENTITY-typed attribute, never in an entity reference.",
    ),
    (
        "external_entity_in_attr",
        "XML 1.0 \u{a7}4.1 (WFC: No External Entity References) \u{2014} an attribute value may not reference an entity whose replacement text lives in an external file.",
    ),
    (
        "text_at_top_level",
        "An XML document is \"prolog element Misc*\": outside the single root element only comments, processing instructions and white space may appear.",
    ),
];
