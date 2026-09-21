/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

//! Character classes, entity references and the DOCTYPE internal subset:
//! the text-level half of the plugin, ported function for function from
//! `ts/src/xml.ts` (`isNameStartCP`, `checkChars`, `checkEntityRefs`,
//! `buildEntityDecoder`, `parseDoctypeEntities`, `parseDoctypeAttlists`,
//! and the two normalisers).
//!
//! Everything here works on `&str` with BYTE offsets. Every offset that
//! is returned or stored is a character boundary: names are read by
//! `char_indices`, and the markup characters the scanners stop on (`<`,
//! `>`, quotes, white space, `&`, `;`) are ASCII, which in UTF-8 never
//! appears inside a multi-byte sequence. The TypeScript code indexes
//! UTF-16 units and the Go code indexes bytes; both reach the same
//! boundaries for the same reason.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;

/// XML 1.0 Fifth Edition NameStartChar (2.3 [4]).
pub(crate) fn is_name_start(cp: char) -> bool {
    let cp = cp as u32;
    cp == 0x3a // ':'
        || cp == 0x5f // '_'
        || (0x41..=0x5a).contains(&cp)
        || (0x61..=0x7a).contains(&cp)
        || (0xc0..=0xd6).contains(&cp)
        || (0xd8..=0xf6).contains(&cp)
        || (0xf8..=0x2ff).contains(&cp)
        || (0x370..=0x37d).contains(&cp)
        || (0x37f..=0x1fff).contains(&cp)
        || (0x200c..=0x200d).contains(&cp)
        || (0x2070..=0x218f).contains(&cp)
        || (0x2c00..=0x2fef).contains(&cp)
        || (0x3001..=0xd7ff).contains(&cp)
        || (0xf900..=0xfdcf).contains(&cp)
        || (0xfdf0..=0xfffd).contains(&cp)
        || (0x10000..=0xeffff).contains(&cp)
}

/// XML 1.0 NameChar (2.3 [4a]): NameStartChar plus digits, hyphen, full
/// stop, the middle dot and the combining-mark blocks.
pub(crate) fn is_name_char(cp: char) -> bool {
    if is_name_start(cp) {
        return true;
    }
    let cp = cp as u32;
    cp == 0x2d
        || cp == 0x2e
        || (0x30..=0x39).contains(&cp)
        || cp == 0xb7
        || (0x300..=0x36f).contains(&cp)
        || (0x203f..=0x2040).contains(&cp)
}

/// XML white space (2.3 [3]).
pub(crate) fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// The byte offset just past the character at `at`, or `s.len()` when
/// `at` is at or beyond the end. The Rust spelling of `i + 1` over a
/// UTF-16 string, where the unit after `i` is always a boundary.
pub(crate) fn after_char(s: &str, at: usize) -> usize {
    if at >= s.len() {
        return s.len();
    }
    s[at..]
        .chars()
        .next()
        .map_or(s.len(), |ch| at + ch.len_utf8())
}

/// Read an XML Name starting at byte offset `start`: the name and the
/// offset after it, or `None` when the character there is not a
/// NameStartChar (or there is no character at all).
pub(crate) fn read_name(s: &str, start: usize) -> Option<(&str, usize)> {
    let mut chars = s[start..].char_indices();
    let (_, first) = chars.next()?;
    if !is_name_start(first) {
        return None;
    }
    let mut end = start + first.len_utf8();
    for (offset, ch) in chars {
        if !is_name_char(ch) {
            break;
        }
        end = start + offset + ch.len_utf8();
    }
    Some((&s[start..end], end))
}

/// `Some("invalid_xml_char")` when `s` holds a C0 control other than tab,
/// newline or carriage return. Only the C0 band is checked; the full Char
/// production (which also excludes U+FFFE, U+FFFF and unpaired
/// surrogates) is not enforced, as in the other two ports.
pub(crate) fn check_chars(s: &str) -> Option<&'static str> {
    s.bytes()
        .any(|byte| byte < 0x20 && !matches!(byte, 0x09 | 0x0a | 0x0d))
        .then_some("invalid_xml_char")
}

/// 2.11 end-of-line handling: CR LF and a lone CR become LF.
pub(crate) fn normalise_line_endings(s: &str) -> Cow<'_, str> {
    if !s.contains('\r') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            out.push('\n');
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    Cow::Owned(out)
}

/// 3.3.3 attribute-value normalisation for CDATA-typed attributes: TAB,
/// LF, CR and CR LF each become one SPACE; runs are not collapsed and
/// the value is not trimmed.
pub(crate) fn normalise_attr_whitespace(s: &str) -> Cow<'_, str> {
    if !s.bytes().any(|byte| matches!(byte, b'\t' | b'\n' | b'\r')) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\t' | '\n' => out.push(' '),
            '\r' => {
                out.push(' ');
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// The five predefined entities (4.6).
const PREDEFINED: [(&str, &str); 5] = [
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
];

/// One entity reference: a hexadecimal or decimal character reference,
/// or a named reference. The name class is the canonical `entityRE`'s
/// (ASCII), not the Unicode Name production `check_entity_refs` uses.
fn entity_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|[A-Za-z_:][A-Za-z0-9_\-.:]*);")
            .expect("the entity pattern is a literal and compiles")
    })
}

/// Decodes entity references in text and attribute values: the five
/// predefined entities, numeric character references, the caller's
/// `customEntities`, and per-parse DOCTYPE entities, the last expanded
/// recursively with cycle detection. The plugin-time map is fixed when
/// the plugin is installed; the DTD map arrives with each call.
#[derive(Debug, Clone)]
pub(crate) struct EntityDecoder {
    base: HashMap<String, String>,
}

impl EntityDecoder {
    pub(crate) fn new(custom: &IndexMap<String, String>) -> Self {
        let mut base: HashMap<String, String> = PREDEFINED
            .iter()
            .map(|(name, text)| ((*name).to_string(), (*text).to_string()))
            .collect();
        for (name, text) in custom {
            base.insert(name.clone(), text.clone());
        }
        EntityDecoder { base }
    }

    /// The names that are always declared: predefined plus custom.
    pub(crate) fn declared(&self) -> &HashMap<String, String> {
        &self.base
    }

    pub(crate) fn decode(&self, src: &str, dtd: &HashMap<String, String>) -> String {
        let mut seen = HashSet::new();
        self.expand(src, dtd, &mut seen)
    }

    fn expand(
        &self,
        src: &str,
        dtd: &HashMap<String, String>,
        seen: &mut HashSet<String>,
    ) -> String {
        if !src.contains('&') {
            return src.to_string();
        }
        let mut out = String::with_capacity(src.len());
        let mut last = 0;
        for found in entity_pattern().captures_iter(src) {
            let whole = found.get(0).expect("a match has a whole");
            out.push_str(&src[last..whole.start()]);
            last = whole.end();
            let reference = &found[1];
            if let Some(digits) = reference.strip_prefix('#') {
                // Lowercase `x` only, per XML 1.0 [66]; `&#X26;` never
                // reaches here because the pattern does not match it.
                let code = match digits.strip_prefix('x') {
                    Some(hex) => u32::from_str_radix(hex, 16),
                    None => digits.parse::<u32>(),
                };
                match code {
                    Ok(code) if code <= 0x10ffff => {
                        // A surrogate code point has no `char`: JavaScript
                        // keeps the lone surrogate, this port folds it to
                        // U+FFFD, the engine-wide rule for lone surrogates.
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    }
                    _ => out.push_str(whole.as_str()),
                }
                continue;
            }
            // Predefined and option-supplied entities take precedence over
            // anything declared in the DTD (the five predefined entities
            // are always available).
            if let Some(text) = self.base.get(reference) {
                out.push_str(text);
                continue;
            }
            match dtd.get(reference) {
                Some(text) if !seen.contains(reference) => {
                    seen.insert(reference.to_string());
                    let expanded = self.expand(text, dtd, seen);
                    seen.remove(reference);
                    out.push_str(&expanded);
                }
                // A recursive reference is a well-formedness violation;
                // the cycle is broken by leaving the text unexpanded.
                _ => out.push_str(whole.as_str()),
            }
        }
        out.push_str(&src[last..]);
        out
    }
}

/// What the current parse knows about entity declarations, for the 4.1
/// "Entity Declared" check.
#[derive(Debug, Clone, Default)]
pub(crate) struct EntityDeclState {
    /// External general entities: declared, never fetched.
    pub(crate) external: HashMap<String, String>,
    /// Unparsed (NDATA) entities: may never be referenced.
    pub(crate) unparsed: HashMap<String, String>,
    /// True only when part of the DTD went unread AND the document did
    /// not declare `standalone="yes"`.
    pub(crate) unread: bool,
}

/// Validate every `&` in `s` as a well-formed entity reference, and apply
/// the 4.1 constraints: "Parsed Entity" (no reference to an unparsed
/// entity), "No External Entity References" (none in an attribute value)
/// and "Entity Declared" (when `strict`, unless part of the DTD went
/// unread). Returns the error code of the first violation.
pub(crate) fn check_entity_refs(
    s: &str,
    dtd: &HashMap<String, String>,
    declared: &HashMap<String, String>,
    strict: bool,
    entdecl: &EntityDeclState,
    in_attr: bool,
) -> Option<&'static str> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            i += 1;
            continue;
        }
        let Some(semi) = s[i + 1..].find(';').map(|at| at + i + 1) else {
            return Some("bad_entity_ref");
        };
        let reference = &s[i + 1..semi];
        if reference.is_empty() {
            return Some("bad_entity_ref");
        }
        if let Some(digits) = reference.strip_prefix('#') {
            if digits.is_empty() {
                return Some("bad_entity_ref");
            }
            // XML 1.0 4.1 [66]: the `x` marker is lowercase-only, so
            // `&#X26;` is neither a character reference nor (since `X26`
            // is not a Name after `#`) an entity reference.
            let (hex, digits) = match digits.strip_prefix('x') {
                Some(rest) => (true, rest),
                None => (false, digits),
            };
            if digits.is_empty() {
                return Some("bad_entity_ref");
            }
            let valid = digits.chars().all(|ch| {
                if hex {
                    ch.is_ascii_hexdigit()
                } else {
                    ch.is_ascii_digit()
                }
            });
            if !valid {
                return Some("bad_entity_ref");
            }
        } else {
            let mut chars = reference.chars();
            if !chars.next().is_some_and(is_name_start) || !chars.all(is_name_char) {
                return Some("bad_entity_ref");
            }
            // 4.1 WFC "Parsed Entity": an entity reference must not name
            // an unparsed (NDATA) entity, anywhere.
            if entdecl.unparsed.contains_key(reference) {
                return Some("unparsed_entity_ref");
            }
            if entdecl.external.contains_key(reference) {
                // Declared externally: "Entity Declared" is satisfied and
                // the replacement text is simply not included. But 4.1
                // WFC "No External Entity References" forbids the
                // reference in an attribute value.
                if in_attr {
                    return Some("external_entity_in_attr");
                }
            } else if strict
                && !entdecl.unread
                && !declared.contains_key(reference)
                && !dtd.contains_key(reference)
            {
                // 4.1 WFC "Entity Declared", suspended when the processor
                // never got to see the whole DTD.
                return Some("undeclared_entity");
            }
        }
        i = semi + 1;
    }
    None
}

/// The general entity declarations of a DOCTYPE internal subset.
#[derive(Debug, Default)]
pub(crate) struct DoctypeEntities {
    /// `<!ENTITY name "value">`: stored verbatim, expanded on reference.
    pub(crate) internal: HashMap<String, String>,
    /// `<!ENTITY name SYSTEM "...">` / `PUBLIC ...`: declared, never
    /// fetched, so a reference stays verbatim in the output (4.4.3).
    pub(crate) external: HashMap<String, String>,
    /// External entities carrying an `NDATA` notation (4.1 WFC "Parsed
    /// Entity" forbids referencing these at all).
    pub(crate) unparsed: HashMap<String, String>,
}

/// The NDATA marker of an unparsed entity declaration (4.2.2 [76]).
fn ndata_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(^|[\s"'])NDATA([\s"']|$)"#).expect("the NDATA pattern is a literal")
    })
}

/// A parameter-entity reference in an internal subset. 4.1 (WFC: Entity
/// Declared) treats declarations behind one exactly like an unread
/// external subset.
pub(crate) fn pe_ref_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"%[A-Za-z_:][A-Za-z0-9_\-.:]*;").expect("the PE reference pattern is a literal")
    })
}

/// An ExternalID in a DOCTYPE head (2.8 [75]).
pub(crate) fn external_id_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(^|[\s>])(SYSTEM|PUBLIC)([\s"'])"#)
            .expect("the ExternalID pattern is a literal")
    })
}

/// A `standalone="yes"` standalone document declaration (2.9 [32]). The
/// canonical pattern pairs the quotes with a backreference, which the
/// `regex` crate has no syntax for, so both quotings are spelled out.
pub(crate) fn standalone_yes_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"\bstandalone\s*=\s*("yes"|'yes')"#)
            .expect("the standalone pattern is a literal")
    })
}

fn skip_space(body: &str, mut at: usize) -> usize {
    let bytes = body.as_bytes();
    while at < bytes.len() && is_space(bytes[at]) {
        at += 1;
    }
    at
}

/// Every general entity declaration of an internal subset. Parameter
/// entity declarations (`<!ENTITY % name ...>`) and the other declaration
/// kinds are recognised and skipped.
pub(crate) fn parse_doctype_entities(body: &str) -> DoctypeEntities {
    let mut out = DoctypeEntities::default();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < body.len() {
        let Some(found) = body[i..].find("<!ENTITY") else {
            break;
        };
        let mut j = skip_space(body, i + found + "<!ENTITY".len());
        // Parameter entity: skip.
        if j < bytes.len() && bytes[j] == b'%' {
            i = body[j..].find('>').map_or(body.len(), |end| j + end + 1);
            continue;
        }
        let Some((name, after)) = read_name(body, j) else {
            i = after_char(body, j);
            continue;
        };
        j = skip_space(body, after);
        // Quoted entity value: an internal entity.
        if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
            let quote = bytes[j];
            j += 1;
            let value_start = j;
            while j < bytes.len() && bytes[j] != quote {
                j += 1;
            }
            if j >= bytes.len() {
                break;
            }
            out.internal
                .insert(name.to_string(), body[value_start..j].to_string());
            i = body[j..].find('>').map_or(body.len(), |end| j + end + 1);
            continue;
        }
        let end = body[j..].find('>').map(|end| j + end);
        let tail = &body[j..end.unwrap_or(body.len())];
        if tail.starts_with("SYSTEM") || tail.starts_with("PUBLIC") {
            // External entity: declared, but never fetched. An `NDATA`
            // notation makes it an unparsed entity, which may not be
            // referenced at all.
            if ndata_pattern().is_match(tail) {
                out.unparsed.insert(name.to_string(), String::new());
            } else {
                out.external.insert(name.to_string(), String::new());
            }
        }
        i = end.map_or(body.len(), |end| end + 1);
    }
    out
}

/// Every `<!ATTLIST>` default attribute value of an internal subset,
/// keyed by element name then attribute name. Literal defaults and
/// `#FIXED "value"` defaults are returned; `#REQUIRED` and `#IMPLIED`
/// contribute nothing, having no default value.
pub(crate) fn parse_doctype_attlists(body: &str) -> IndexMap<String, IndexMap<String, String>> {
    let mut out: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < body.len() {
        let Some(found) = body[i..].find("<!ATTLIST") else {
            break;
        };
        let mut j = skip_space(body, i + found + "<!ATTLIST".len());
        let Some((elem_name, after)) = read_name(body, j) else {
            i = after_char(body, j);
            continue;
        };
        j = after;

        // Loop over AttDefs until `>` or the end.
        while j < body.len() {
            j = skip_space(body, j);
            if j >= body.len() {
                break;
            }
            if bytes[j] == b'>' {
                j += 1;
                break;
            }
            let Some((attr_name, after)) = read_name(body, j) else {
                j = after_char(body, j);
                continue;
            };
            j = skip_space(body, after);

            // Skip the AttType: an enumeration `( ... )`, `NOTATION ( ...
            // )`, or a bare type identifier (CDATA, ID, IDREF, ...).
            if j < bytes.len() && bytes[j] == b'(' {
                match body[j..].find(')') {
                    Some(close) => j += close + 1,
                    None => {
                        j = body.len();
                        break;
                    }
                }
            } else if body[j..].starts_with("NOTATION") {
                j = skip_space(body, j + "NOTATION".len());
                if j < bytes.len() && bytes[j] == b'(' {
                    match body[j..].find(')') {
                        Some(close) => j += close + 1,
                        None => {
                            j = body.len();
                            break;
                        }
                    }
                }
            } else {
                while j < bytes.len() && bytes[j].is_ascii_uppercase() {
                    j += 1;
                }
            }
            j = skip_space(body, j);

            // DefaultDecl.
            if body[j..].starts_with("#REQUIRED") {
                j += "#REQUIRED".len();
                continue;
            }
            if body[j..].starts_with("#IMPLIED") {
                j += "#IMPLIED".len();
                continue;
            }
            if body[j..].starts_with("#FIXED") {
                j = skip_space(body, j + "#FIXED".len());
            }
            if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                let quote = bytes[j];
                j += 1;
                let value_start = j;
                while j < bytes.len() && bytes[j] != quote {
                    j += 1;
                }
                if j >= bytes.len() {
                    break;
                }
                out.entry(elem_name.to_string())
                    .or_default()
                    .insert(attr_name.to_string(), body[value_start..j].to_string());
                j += 1;
            }
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_read_by_character() {
        assert_eq!(read_name("<เจมส์>", 1), Some(("เจมส์", 1 + "เจมส์".len())));
        assert_eq!(read_name("<1a>", 1), None);
        assert_eq!(read_name("a-b.c>", 0), Some(("a-b.c", 5)));
        assert_eq!(read_name("", 0), None);
    }

    #[test]
    fn after_char_steps_over_a_whole_character() {
        assert_eq!(after_char("é!", 0), 2);
        assert_eq!(after_char("a", 0), 1);
        assert_eq!(after_char("a", 5), 1);
    }

    #[test]
    fn line_endings_and_attribute_whitespace_normalise() {
        assert_eq!(normalise_line_endings("a\r\nb\rc\n"), "a\nb\nc\n");
        assert_eq!(normalise_attr_whitespace("a\tb\r\nc\rd\ne"), "a b c d e");
    }

    #[test]
    fn decoder_expands_dtd_entities_with_cycle_detection() {
        let decoder = EntityDecoder::new(&IndexMap::new());
        let mut dtd = HashMap::new();
        dtd.insert("a".to_string(), "&b;".to_string());
        dtd.insert("b".to_string(), "&a;x".to_string());
        assert_eq!(decoder.decode("&a;", &dtd), "&a;x");
        assert_eq!(decoder.decode("&#x1F600;&#65;&amp;", &dtd), "\u{1F600}A&");
        assert_eq!(decoder.decode("&#xD800;", &dtd), "\u{FFFD}");
        assert_eq!(decoder.decode("&#x110000;", &dtd), "&#x110000;");
    }

    #[test]
    fn entity_reference_checks() {
        let none = HashMap::new();
        let decoder = EntityDecoder::new(&IndexMap::new());
        let state = EntityDeclState::default();
        let check = |s: &str, attr: bool| {
            check_entity_refs(s, &none, decoder.declared(), true, &state, attr)
        };
        assert_eq!(check("a &amp; b", false), None);
        assert_eq!(check("a & b", false), Some("bad_entity_ref"));
        assert_eq!(check("&#X26;", false), Some("bad_entity_ref"));
        assert_eq!(check("&nope;", false), Some("undeclared_entity"));
        let mut external = EntityDeclState::default();
        external.external.insert("e".to_string(), String::new());
        assert_eq!(
            check_entity_refs("&e;", &none, decoder.declared(), true, &external, true),
            Some("external_entity_in_attr")
        );
        assert_eq!(
            check_entity_refs("&e;", &none, decoder.declared(), true, &external, false),
            None
        );
    }

    #[test]
    fn doctype_declarations_are_mined() {
        let subset = r#"<!ENTITY % p SYSTEM "p.ent"><!ENTITY g "G"><!ENTITY e SYSTEM "e.ent"><!ENTITY u SYSTEM "u.gif" NDATA gif><!ATTLIST doc x CDATA "default" y (a|b) #FIXED "b" r CDATA #REQUIRED>"#;
        let ents = parse_doctype_entities(subset);
        assert_eq!(ents.internal.get("g").map(String::as_str), Some("G"));
        assert!(ents.external.contains_key("e"));
        assert!(ents.unparsed.contains_key("u"));
        assert!(!ents.internal.contains_key("p"));
        let atts = parse_doctype_attlists(subset);
        assert_eq!(atts["doc"]["x"], "default");
        assert_eq!(atts["doc"]["y"], "b");
        assert!(!atts["doc"].contains_key("r"));
    }
}
