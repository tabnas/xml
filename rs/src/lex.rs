/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

//! The one lexer matcher, `xmltag`: the port of `buildXmlTagMatcher` in
//! `ts/src/xml.ts`. It recognises every XML construct that starts with
//! `<`, and, while an element is open, claims the character data up to
//! the next `<` so that the engine's own text and fixed-token matchers
//! never split it on `,` or `:`.
//!
//! It emits one of:
//!
//! ```text
//! <name attr="v" ...>     #XOP  val = { name, attributes }
//! <name attr="v" ... />   #XSC  val = { name, attributes }
//! </name>                 #XCL  val = name
//! <!-- comment -->        #XIG  (ignored)
//! <?target ...?>          #XIG  (ignored)
//! <!DOCTYPE ...>          #XIG  (ignored; entity and ATTLIST declarations
//!                               are mined into the parse context first)
//! <![CDATA[ ... ]]>       #TX   (verbatim text, no entity decoding)
//! text                    #TX   (validated and decoded; only while an
//!                               element is open)
//! ```
//!
//! The per-parse state (the element depth, the DOCTYPE declarations, the
//! standalone flag) lives on `Context::u`, exactly where `lex.ctx.u` and
//! `lex.Ctx.U` keep it in the other two ports, so the rule actions in
//! `lib.rs` read it from the same place.

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use tabnas::{Context, ImperativeLexMatcher, Lexer, Rule, Tin, Token, Value, TIN_TX};

use crate::entity::{
    after_char, check_chars, check_entity_refs, external_id_pattern, is_space,
    normalise_attr_whitespace, normalise_line_endings, parse_doctype_attlists,
    parse_doctype_entities, pe_ref_pattern, read_name, standalone_yes_pattern, EntityDeclState,
    EntityDecoder,
};

/// The token identities the plugin mints, minted before the matcher is
/// built so it can close over them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tokens {
    pub(crate) xig: Tin,
    pub(crate) xop: Tin,
    pub(crate) xcl: Tin,
    pub(crate) xsc: Tin,
}

/// Everything the matcher needs that is fixed when the plugin installs.
pub(crate) struct MatcherState {
    pub(crate) decoder: EntityDecoder,
    /// `entities`: decode references in character data.
    pub(crate) entities: bool,
    /// `strictEntities`: an undeclared entity is an error.
    pub(crate) strict: bool,
    pub(crate) tokens: Tokens,
}

// The keys under `Context::u`, spelled as the canonical plugin spells
// them so a caller inspecting the context sees the same names.
const DEPTH: &str = "xmlDepth";
const DTD_ENTITIES: &str = "dtdEntities";
const DTD_EXTERNAL: &str = "dtdExternalEntities";
const DTD_UNPARSED: &str = "dtdUnparsedEntities";
const DTD_ATTR_DEFAULTS: &str = "dtdAttrDefaults";
const DTD_UNREAD: &str = "dtdUnread";
const STANDALONE: &str = "xmlStandalone";
/// The row a byte-order mark was skipped on, and the one key here the
/// canonical plugin has no counterpart for: it keeps its own cursor and
/// never charges the mark a column, while this port hands the cursor to
/// the engine, which charges one for every character it passes. See
/// [`discount_bom`].
const BOM_ROW: &str = "xmlBomRow";

/// Undo the display column the engine charged for a skipped byte-order
/// mark.
///
/// The mark is an encoding signature, not document content: it occupies
/// no column, so `ts/src/xml.ts` advances `pnt.sI` alone and `go/xml.go`
/// advances `pnt.SI` alone, and every token after one keeps the column
/// it would have had in a document without it. This port cannot move the
/// engine's cursor without also moving its column, so the column comes
/// off the token instead, on the row the mark was on and no other.
///
/// It reaches every token this matcher mints, and `check_doc_text` in
/// `lib.rs` applies it to the one token the plugin reports against but
/// does not mint, so every diagnostic the plugin raises lands where the
/// canonical plugin lands it. What it cannot reach is the engine's own
/// `unexpected`: that is raised against a token the ENGINE minted, at
/// end of source (`#ZZ`, returned before any matcher runs) or over
/// markup this matcher declined, and those keep the extra column.
/// `a_byte_order_mark_costs_no_column` pins both halves.
pub(crate) fn discount_bom(context: &Context, site: &mut tabnas::Site) {
    let Some(Value::Number(row)) = context.u.get(BOM_ROW) else {
        return;
    };
    if site.ri == *row as usize && 1 < site.ci {
        site.ci -= 1;
    }
}

/// How many elements may be open at once. The tag that would open one
/// more is refused where it is read, with the engine's `cancel` code.
///
/// The engine walks a nested value by recursion, a call per level, to
/// display, convert, clone, compare or drop it, and drops the snapshots of
/// the rules on its stack, three to an element, the same way. A stack
/// overflow ends the process where no error can be caught. The parse
/// itself got through 16,000 elements on a 2 MiB thread in a release
/// build, but displaying the value overflowed that stack about 400
/// elements deep in a debug build. 256, the depth libxml2 allows by
/// default, leaves room for every one of those on a default thread.
/// TypeScript and Go have no limit (`README.md` records the difference),
/// and no document a person writes comes near this one.
///
/// The lexer enforces it, not a parse budget, because a caller's
/// `parse_budget` replaces the budget in place: a limit kept there would
/// go whenever a caller set one after this plugin. The lexer keeps the
/// count anyway, so the check costs a comparison per open tag.
pub(crate) const DEPTH_LIMIT: i64 = 256;

/// The XML nesting depth of the parse so far: open tags minus close tags.
fn depth(context: &Context) -> i64 {
    match context.u.get(DEPTH) {
        Some(Value::Number(depth)) => *depth as i64,
        _ => 0,
    }
}

fn set_depth(context: &mut Context, depth: i64) {
    context
        .u
        .insert(DEPTH.to_string(), Value::Number(depth.max(0) as f64));
}

fn flag(context: &Context, key: &str) -> bool {
    matches!(context.u.get(key), Some(Value::Bool(true)))
}

/// A string-valued map stored under `Context::u`, empty when absent.
fn string_map(context: &Context, key: &str) -> HashMap<String, String> {
    let Some(Value::Object(map)) = context.u.get(key) else {
        return HashMap::new();
    };
    map.iter()
        .map(|(name, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            (name.clone(), text)
        })
        .collect()
}

/// Merge string entries into the map stored under `key`, creating it.
fn merge_string_map(context: &mut Context, key: &str, entries: &HashMap<String, String>) {
    if entries.is_empty() {
        return;
    }
    let mut merged: IndexMap<String, Value> = match context.u.get(key) {
        Some(Value::Object(existing)) => (**existing).clone(),
        _ => IndexMap::new(),
    };
    // Sorted so the stored map is deterministic whatever order the
    // HashMap yields; the fixtures compare values, not key order.
    let mut names: Vec<&String> = entries.keys().collect();
    names.sort();
    for name in names {
        merged.insert(name.clone(), Value::String(entries[name].clone()));
    }
    context.u.insert(key.to_string(), Value::object(merged));
}

/// The DOCTYPE-declared internal entities of this parse.
pub(crate) fn dtd_entities(context: &Context) -> HashMap<String, String> {
    string_map(context, DTD_ENTITIES)
}

/// The DOCTYPE-supplied default attributes for `element`, if any.
pub(crate) fn dtd_attr_defaults(
    context: &Context,
    element: &str,
) -> Option<IndexMap<String, Value>> {
    let Some(Value::Object(all)) = context.u.get(DTD_ATTR_DEFAULTS) else {
        return None;
    };
    match all.get(element) {
        Some(Value::Object(defaults)) => Some((**defaults).clone()),
        _ => None,
    }
}

/// What the parse knows about entity declarations, for the 4.1 "Entity
/// Declared" check. `unread` is true only when part of the DTD went
/// unread AND the document did not declare `standalone="yes"`: with
/// `standalone="yes"` the document asserts that nothing outside the
/// internal subset matters, so the constraint applies again.
fn entity_decl_state(context: &Context) -> EntityDeclState {
    EntityDeclState {
        external: string_map(context, DTD_EXTERNAL),
        unparsed: string_map(context, DTD_UNPARSED),
        unread: flag(context, DTD_UNREAD) && !flag(context, STANDALONE),
    }
}

/// What one call of the matcher decided, in byte offsets from the cursor.
/// The token or diagnostic is built afterwards, once the borrow of the
/// remaining source has ended.
enum Scan {
    /// A token whose source is the next `end` bytes.
    Token {
        name: &'static str,
        tin: Tin,
        val: Value,
        end: usize,
    },
    /// A well-formedness error over the span `from..to`.
    Bad {
        code: &'static str,
        from: usize,
        to: usize,
    },
    /// Not XML; let the engine's own matchers have the cursor.
    NoMatch,
}

fn bad(code: &'static str, from: usize, to: usize) -> Scan {
    Scan::Bad { code, from, to }
}

/// Build the `xmltag` matcher.
pub(crate) fn matcher(state: Arc<MatcherState>) -> ImperativeLexMatcher {
    Arc::new(
        move |lexer: &mut Lexer<'_>, _rule: &mut Rule, context: &mut Context| {
            run(&state, lexer, context)
        },
    )
}

fn run(state: &MatcherState, lexer: &mut Lexer<'_>, context: &mut Context) -> Option<Token> {
    // Strip a byte-order mark at the very start of the input. The mark is
    // not part of the document, so it is skipped and matching carries on
    // in this same call: handing the cursor back here would leave a
    // lexer with no other matcher enabled in pure mode, and the `<?xml`
    // that follows would be reported as an unexpected character.
    if lexer.point().site.pos == 0 && lexer.remaining().starts_with('\u{FEFF}') {
        let row = lexer.point().site.ri;
        lexer.advance_chars(1);
        // The engine charges a display column for every character the
        // cursor passes, and a byte-order mark is not one. Both other
        // ports own the cursor arithmetic and simply do not charge it
        // (`pnt.sI = bomLen` in `ts/src/xml.ts`, `pnt.SI = 3` in
        // `go/xml.go`); here the row it sat on is remembered instead, and
        // `discount_bom` gives the column back to every token minted
        // there. See `BOM_ROW`.
        context
            .u
            .insert(BOM_ROW.to_string(), Value::Number(row as f64));
    }

    let scan = scan(state, context, lexer.remaining());
    match scan {
        Scan::NoMatch => None,
        Scan::Bad { code, from, to } => {
            let rest = lexer.remaining();
            let base = lexer.point().site.pos;
            let to = to.min(rest.len());
            let start = base + rest[..from].chars().count();
            let end = base + rest[..to].chars().count();
            let mut token = lexer.bad_span(code, start, end);
            discount_bom(context, &mut token.site);
            Some(token)
        }
        Scan::Token {
            name,
            tin,
            val,
            end,
        } => {
            let source = lexer.remaining()[..end].to_string();
            let mut point = lexer.point();
            discount_bom(context, &mut point.site);
            let token = lexer.token(name, tin, val, source.as_str(), point);
            lexer.advance_chars(source.chars().count());
            Some(token)
        }
    }
}

/// Validate and decode a run of character data (not CDATA): every code
/// point a legal Char, no `]]>`, every `&` a well-formed reference.
fn process_text(
    state: &MatcherState,
    raw: &str,
    dtd: &HashMap<String, String>,
    entdecl: &EntityDeclState,
) -> Result<String, &'static str> {
    if let Some(code) = check_chars(raw) {
        return Err(code);
    }
    if raw.contains("]]>") {
        return Err("cdata_terminator_in_text");
    }
    if let Some(code) = check_entity_refs(
        raw,
        dtd,
        state.decoder.declared(),
        state.strict,
        entdecl,
        false,
    ) {
        return Err(code);
    }
    // 2.11: normalise CR LF and lone CR to LF before anything downstream.
    let normalised = normalise_line_endings(raw);
    Ok(if state.entities {
        state.decoder.decode(&normalised, dtd)
    } else {
        normalised.into_owned()
    })
}

fn scan(state: &MatcherState, context: &mut Context, rest: &str) -> Scan {
    let bytes = rest.as_bytes();
    let Some(&first) = bytes.first() else {
        return Scan::NoMatch;
    };

    // Inside an open element, the characters up to the next `<` are one
    // text token: validated here, and kept whole so the engine's own
    // matchers never see a comma or colon in them.
    if first != b'<' {
        if depth(context) > 0 {
            let end = rest.find('<').unwrap_or(rest.len());
            let dtd = dtd_entities(context);
            return match process_text(state, &rest[..end], &dtd, &entity_decl_state(context)) {
                Ok(text) => Scan::Token {
                    name: "#TX",
                    tin: TIN_TX,
                    val: Value::String(text),
                    end,
                },
                Err(code) => bad(code, 0, end),
            };
        }
        return Scan::NoMatch;
    }

    // Comment: <!-- ... -->
    if let Some(after) = rest.strip_prefix("<!--") {
        let Some(found) = after.find("-->") else {
            return bad("unterminated_comment", 0, rest.len());
        };
        let end = 4 + found + 3;
        let body = &after[..found];
        // WF: `--` must not occur in a comment body.
        if body.contains("--") {
            return bad("comment_double_dash", 0, end);
        }
        if check_chars(body).is_some() {
            return bad("invalid_xml_char", 0, end);
        }
        return ignored(state, rest, end);
    }

    // CDATA: <![CDATA[ ... ]]>
    if let Some(after) = rest.strip_prefix("<![CDATA[") {
        let Some(found) = after.find("]]>") else {
            return bad("unterminated_cdata", 0, rest.len());
        };
        let end = 9 + found + 3;
        let text = &after[..found];
        if check_chars(text).is_some() {
            return bad("invalid_xml_char", 0, end);
        }
        // 2.11 line-end normalisation applies to CDATA too.
        return Scan::Token {
            name: "#TX",
            tin: TIN_TX,
            val: Value::String(normalise_line_endings(text).into_owned()),
            end,
        };
    }

    // DOCTYPE: <!DOCTYPE ... [ ... ] >
    if rest.starts_with("<!DOCTYPE") {
        return scan_doctype(state, context, rest);
    }

    // Processing instruction: <? ... ?>
    if bytes.get(1) == Some(&b'?') {
        let Some(found) = rest[2..].find("?>") else {
            return bad("unterminated_pi", 0, rest.len());
        };
        let body_end = 2 + found;
        let end = body_end + 2;
        // WF: the PI target must be a Name, and not empty.
        let Some((target, after)) = read_name(rest, 2) else {
            return bad("pi_target_invalid", 0, end);
        };
        if after > body_end {
            return bad("pi_target_invalid", 0, end);
        }
        // After the target only white space, then content, is allowed.
        if after < body_end && !is_space(bytes[after]) {
            return bad("pi_target_invalid", 0, end);
        }
        if check_chars(&rest[2..body_end]).is_some() {
            return bad("invalid_xml_char", 0, end);
        }
        // The XML declaration's standalone document declaration (2.9)
        // decides whether the 4.1 "Entity Declared" constraint still
        // applies when part of the DTD went unread.
        if target == "xml" && standalone_yes_pattern().is_match(&rest[after..body_end]) {
            context.u.insert(STANDALONE.to_string(), Value::Bool(true));
        }
        return ignored(state, rest, end);
    }

    // Closing tag: </name>
    if bytes.get(1) == Some(&b'/') {
        // WF: an empty close tag `</>` is invalid.
        let Some((name, after)) = read_name(rest, 2) else {
            return bad("xml_invalid_tag", 0, after_char(rest, 2));
        };
        let mut i = after;
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'>' {
            return bad("xml_invalid_tag", 0, after_char(rest, i));
        }
        let end = i + 1;
        set_depth(context, depth(context) - 1);
        return Scan::Token {
            name: "#XCL",
            tin: state.tokens.xcl,
            val: Value::String(name.to_string()),
            end,
        };
    }

    // Opening or self-closing tag: <name attr="v" ... /> . A `<` that
    // starts no Name is not XML markup at all; the engine reports it.
    let Some((name, after)) = read_name(rest, 1) else {
        return Scan::NoMatch;
    };
    let mut attributes: IndexMap<String, Value> = IndexMap::new();
    let mut i = after;
    loop {
        let ws_start = i;
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            return bad("xml_invalid_tag", 0, rest.len());
        }

        if bytes[i] == b'>' {
            if depth(context) >= DEPTH_LIMIT {
                return bad("cancel", 0, i + 1);
            }
            set_depth(context, depth(context) + 1);
            return Scan::Token {
                name: "#XOP",
                tin: state.tokens.xop,
                val: tag_value(name, attributes),
                end: i + 1,
            };
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'>') {
            // A self-closed element is closed as it opens: depth unchanged.
            return Scan::Token {
                name: "#XSC",
                tin: state.tokens.xsc,
                val: tag_value(name, attributes),
                end: i + 2,
            };
        }

        // Attributes must be separated by white space.
        if ws_start == i {
            return bad("xml_invalid_tag", 0, after_char(rest, i));
        }

        let Some((attr_name, attr_end)) = read_name(rest, i) else {
            return bad("xml_invalid_tag", 0, after_char(rest, i));
        };
        i = attr_end;
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return bad("xml_invalid_tag", 0, after_char(rest, i));
        }
        i += 1;
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            return bad("xml_invalid_tag", 0, rest.len());
        }
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            return bad("xml_invalid_tag", 0, after_char(rest, i));
        }
        i += 1;
        let value_start = i;
        // An attribute value cannot contain a literal `<` (3.1).
        while i < bytes.len() && bytes[i] != quote {
            if bytes[i] == b'<' {
                return bad("lt_in_attr_value", 0, i + 1);
            }
            i += 1;
        }
        if i >= bytes.len() {
            return bad("xml_invalid_tag", 0, rest.len());
        }
        let raw = &rest[value_start..i];
        i += 1; // the closing quote

        if let Some(code) = check_chars(raw) {
            return bad(code, value_start, i);
        }
        let dtd = dtd_entities(context);
        if let Some(code) = check_entity_refs(
            raw,
            &dtd,
            state.decoder.declared(),
            state.strict,
            &entity_decl_state(context),
            true,
        ) {
            return bad(code, value_start, i);
        }
        if attributes.contains_key(attr_name) {
            return bad("duplicate_attribute", 0, i);
        }
        // 3.3.3 attribute-value normalisation: literal white space
        // becomes a single SPACE before entity references are decoded.
        // Without DTD attribute types every attribute is CDATA-typed, so
        // there is no further collapsing or trimming. References are
        // decoded here whatever the `entities` option says, as the
        // canonical plugin decodes them.
        let normalised = normalise_attr_whitespace(raw);
        attributes.insert(
            attr_name.to_string(),
            Value::String(state.decoder.decode(&normalised, &dtd)),
        );
    }
}

/// The value of an open or self-closing tag token.
fn tag_value(name: &str, attributes: IndexMap<String, Value>) -> Value {
    let mut value = IndexMap::new();
    value.insert("name".to_string(), Value::String(name.to_string()));
    value.insert("attributes".to_string(), Value::object(attributes));
    Value::object(value)
}

/// An `#XIG` token over the next `end` bytes, carrying its source as its
/// value, as the canonical matcher does.
fn ignored(state: &MatcherState, rest: &str, end: usize) -> Scan {
    Scan::Token {
        name: "#XIG",
        tin: state.tokens.xig,
        val: Value::String(rest[..end].to_string()),
        end,
    }
}

/// `<!DOCTYPE ... [ ... ] >`: find its end, then mine the internal subset
/// for entity and ATTLIST declarations and stash them on the parse
/// context for the text, attribute and element paths to read back.
fn scan_doctype(state: &MatcherState, context: &mut Context, rest: &str) -> Scan {
    let bytes = rest.as_bytes();
    let mut i = 9;
    let mut nesting = 0i32;
    let mut subset_start = None;
    let mut subset_end = None;
    while i < bytes.len() {
        let tail = &rest[i..];
        // Comments and processing instructions first: their bodies are
        // opaque text, so an apostrophe or a `>` inside one is not markup.
        if let Some(after) = tail.strip_prefix("<!--") {
            match after.find("-->") {
                Some(close) => i += 4 + close + 3,
                None => i = rest.len(),
            }
            continue;
        }
        if let Some(after) = tail.strip_prefix("<?") {
            match after.find("?>") {
                Some(close) => i += 2 + close + 2,
                None => i = rest.len(),
            }
            continue;
        }
        let ch = bytes[i];
        // Quoted strings next, so a `]` or `>` inside an entity value or
        // attribute default cannot end the subset early.
        if ch == b'"' || ch == b'\'' {
            i += 1;
            while i < bytes.len() && bytes[i] != ch {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if ch == b'[' {
            if nesting == 0 {
                subset_start = Some(i + 1);
            }
            nesting += 1;
        } else if ch == b']' {
            nesting -= 1;
            if nesting == 0 {
                subset_end = Some(i);
            }
        } else if ch == b'>' && nesting <= 0 {
            break;
        }
        i = after_char(rest, i);
    }
    if i >= bytes.len() {
        return bad("unterminated_doctype", 0, rest.len());
    }
    let end = i + 1;

    // XML 1.0 4.1, WFC "Entity Declared": the constraint that a referenced
    // entity be declared applies only when the processor has seen every
    // declaration. A DOCTYPE naming an external subset, which is never
    // fetched, suspends it unless the document is `standalone="yes"`. The
    // head is everything up to the internal subset's `[`, or up to the
    // closing `>` when there is no internal subset.
    let head_end = subset_start.map_or(i, |start| start - 1);
    if external_id_pattern().is_match(&rest[9..head_end]) {
        context.u.insert(DTD_UNREAD.to_string(), Value::Bool(true));
    }

    if let (Some(start), Some(stop)) = (subset_start, subset_end) {
        if stop > start {
            let subset = &rest[start..stop];
            // A parameter-entity reference in the internal subset hides
            // declarations from a non-validating processor just as an
            // external subset does.
            if pe_ref_pattern().is_match(subset) {
                context.u.insert(DTD_UNREAD.to_string(), Value::Bool(true));
            }
            let entities = parse_doctype_entities(subset);
            merge_string_map(context, DTD_ENTITIES, &entities.internal);
            merge_string_map(context, DTD_EXTERNAL, &entities.external);
            merge_string_map(context, DTD_UNPARSED, &entities.unparsed);
            let attlists = parse_doctype_attlists(subset);
            if !attlists.is_empty() {
                let mut merged: IndexMap<String, Value> = match context.u.get(DTD_ATTR_DEFAULTS) {
                    Some(Value::Object(existing)) => (**existing).clone(),
                    _ => IndexMap::new(),
                };
                for (element, defaults) in attlists {
                    let mut entry: IndexMap<String, Value> = match merged.get(&element) {
                        Some(Value::Object(existing)) => (**existing).clone(),
                        _ => IndexMap::new(),
                    };
                    for (attr, value) in defaults {
                        entry.insert(attr, Value::String(value));
                    }
                    merged.insert(element, Value::object(entry));
                }
                context
                    .u
                    .insert(DTD_ATTR_DEFAULTS.to_string(), Value::object(merged));
            }
        }
    }
    ignored(state, rest, end)
}
