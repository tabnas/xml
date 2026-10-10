/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

//! Namespace resolution (Namespaces in XML 1.0) and the inherited
//! `xml:space` / `xml:lang` values (XML 1.0 2.10 and 2.12), resolved
//! element by element as start tags are read, exactly as `resolveElement`
//! in `ts/src/xml.ts` resolves them.
//!
//! Start tags are read in document order, which is the pre-order a walk
//! of the finished tree makes, so resolving each element as it starts
//! reaches the elements, the first violation and the elements left
//! unresolved after it that the walk of the finished tree did. The scope
//! an element's content inherits travels down the parse in the element
//! rule's `k` bag, as the value [`Scope::to_value`] writes.

use std::collections::HashMap;

use indexmap::IndexMap;
use tabnas::Value;

/// The `xml` prefix is bound to this URI and may be used implicitly.
pub(crate) const XML_NS_URI: &str = "http://www.w3.org/XML/1998/namespace";
/// The `xmlns` prefix is reserved and must never be declared.
pub(crate) const XMLNS_NS_URI: &str = "http://www.w3.org/2000/xmlns/";

/// State inherited down the tree: prefix to namespace name (the empty
/// prefix is the default namespace), the active `xml:space`, and the
/// active `xml:lang`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scope {
    ns: HashMap<String, String>,
    space: String,
    lang: String,
}

impl Scope {
    /// The scope a resolution starts from: the `xml` prefix pre-bound to
    /// its reserved URI, so `xml:lang` and `xml:space` qualify without an
    /// explicit declaration.
    pub(crate) fn root() -> Scope {
        let mut ns = HashMap::new();
        ns.insert("xml".to_string(), XML_NS_URI.to_string());
        Scope {
            ns,
            space: "default".to_string(),
            lang: String::new(),
        }
    }

    /// The scope as a value, for a rule's `k` bag: an object of `ns`,
    /// `space` and `lang`.
    pub(crate) fn to_value(&self) -> Value {
        let ns = self
            .ns
            .iter()
            .map(|(prefix, uri)| (prefix.clone(), Value::String(uri.clone())))
            .collect::<IndexMap<_, _>>();
        let mut scope = IndexMap::new();
        scope.insert("ns".to_string(), Value::object(ns));
        scope.insert("space".to_string(), Value::String(self.space.clone()));
        scope.insert("lang".to_string(), Value::String(self.lang.clone()));
        Value::object(scope)
    }

    /// The scope [`Scope::to_value`] wrote.
    pub(crate) fn from_value(value: &Value) -> Scope {
        let text = |field: &str| match value {
            Value::Object(scope) => match scope.get(field) {
                Some(Value::String(text)) => text.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let ns = match value {
            Value::Object(scope) => match scope.get("ns") {
                Some(Value::Object(ns)) => ns
                    .iter()
                    .filter_map(|(prefix, uri)| match uri {
                        Value::String(uri) => Some((prefix.clone(), uri.clone())),
                        _ => None,
                    })
                    .collect(),
                _ => HashMap::new(),
            },
            _ => HashMap::new(),
        };
        Scope {
            ns,
            space: text("space"),
            lang: text("lang"),
        }
    }
}

/// A namespace name is a URI reference (RFC 3986), and a URI reference
/// cannot contain white space. Attribute-value normalisation has already
/// turned a literal TAB/LF/CR in the declaration into a space by the time
/// the value gets here.
fn invalid_namespace_uri(uri: &str) -> bool {
    uri.bytes()
        .any(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
}

fn attribute_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// What resolving one element gives.
#[derive(Debug, PartialEq)]
pub(crate) struct Resolved {
    /// `None`, or the code of the violation the element commits:
    /// reserved-prefix misuse, a namespace name with white space, or
    /// (when strict) an unbound prefix.
    pub(crate) code: Option<&'static str>,
    /// The element's `localName`.
    pub(crate) local_name: String,
    /// The members the element carries after `children`, in their order:
    /// `prefix`, `namespace`, `space` and `lang`, each only where it
    /// applies. On a violation, what resolution leaves on the element it
    /// stops at: nothing for a bad declaration or attribute, `prefix` for
    /// an unbound element prefix.
    pub(crate) fields: IndexMap<String, Value>,
    /// The scope the element's content inherits, when the element's own
    /// attributes declare anything; `None` when it inherits the element's
    /// scope unchanged, and on a violation.
    pub(crate) scope: Option<Scope>,
}

/// Resolve one element, named `name` and carrying `attributes`, against
/// the scope it inherits.
pub(crate) fn resolve_element(
    name: &str,
    attributes: &IndexMap<String, Value>,
    scope: &Scope,
    strict: bool,
) -> Resolved {
    let stop = |code, local_name: &str, fields| Resolved {
        code: Some(code),
        local_name: local_name.to_string(),
        fields,
        scope: None,
    };
    let mut ns: Option<HashMap<String, String>> = None;
    let mut space = scope.space.clone();
    let mut lang = scope.lang.clone();
    let mut declared = false;

    // Pass 1: every namespace declaration on this element, plus xml:space
    // / xml:lang. Namespaces in XML 1.0 5.2 scopes a declaration over the
    // whole element it appears on, INCLUDING that element's own other
    // attributes, so every declaration must be in hand before any
    // prefixed name is resolved.
    for (key, value) in attributes {
        let text = attribute_text(value);
        if key == "xmlns" {
            if text == XML_NS_URI || text == XMLNS_NS_URI {
                return stop("reserved_namespace", name, IndexMap::new());
            }
            if invalid_namespace_uri(&text) {
                return stop("invalid_namespace_uri", name, IndexMap::new());
            }
            ns.get_or_insert_with(|| scope.ns.clone())
                .insert(String::new(), text);
            declared = true;
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            match prefix {
                "xml" => {
                    if text != XML_NS_URI {
                        return stop("reserved_namespace", name, IndexMap::new());
                    }
                }
                "xmlns" => return stop("reserved_namespace", name, IndexMap::new()),
                _ => {
                    if text == XML_NS_URI || text == XMLNS_NS_URI {
                        return stop("reserved_namespace", name, IndexMap::new());
                    }
                }
            }
            if invalid_namespace_uri(&text) {
                return stop("invalid_namespace_uri", name, IndexMap::new());
            }
            ns.get_or_insert_with(|| scope.ns.clone())
                .insert(prefix.to_string(), text);
            declared = true;
        } else if key == "xml:space" {
            space = text;
            declared = true;
        } else if key == "xml:lang" {
            lang = text;
            declared = true;
        }
    }
    let bound = ns.as_ref().unwrap_or(&scope.ns);

    // Pass 2: prefixed attribute names against the completed in-scope
    // declarations. A namespace constraint only: an unbound prefix on an
    // attribute leaves the document XML 1.0 well-formed.
    if strict {
        for key in attributes.keys() {
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            if let Some((prefix, _)) = key.split_once(':') {
                if !prefix.is_empty() && !bound.contains_key(prefix) {
                    return stop("unbound_prefix", name, IndexMap::new());
                }
            }
        }
    }

    let mut fields = IndexMap::new();
    let local_name = if let Some((prefix, local)) = name.split_once(':') {
        fields.insert("prefix".to_string(), Value::String(prefix.to_string()));
        if let Some(uri) = bound.get(prefix) {
            fields.insert("namespace".to_string(), Value::String(uri.clone()));
        } else if strict {
            return stop("unbound_prefix", local, fields);
        }
        // Not strict: `namespace` stays unset. The element is named but
        // unqualified, which is what a namespace-unaware (yet XML 1.0
        // conformant) consumer sees.
        local.to_string()
    } else {
        if let Some(uri) = bound.get("").filter(|uri| !uri.is_empty()) {
            fields.insert("namespace".to_string(), Value::String(uri.clone()));
        }
        name.to_string()
    };

    if space != "default" {
        fields.insert("space".to_string(), Value::String(space.clone()));
    }
    if !lang.is_empty() {
        fields.insert("lang".to_string(), Value::String(lang.clone()));
    }

    let scope = declared.then(|| Scope {
        ns: ns.unwrap_or_else(|| scope.ns.clone()),
        space,
        lang,
    });
    Resolved {
        code: None,
        local_name,
        fields,
        scope,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> IndexMap<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), Value::String((*value).to_string())))
            .collect()
    }

    fn text(fields: &IndexMap<String, Value>, key: &str) -> Option<String> {
        match fields.get(key) {
            Some(Value::String(text)) => Some(text.clone()),
            _ => None,
        }
    }

    #[test]
    fn a_declaration_scopes_over_the_element_and_its_content() {
        let root = Scope::root();
        let a = resolve_element(
            "p:a",
            &attrs(&[("p:x", "1"), ("xmlns:p", "urn:p")]),
            &root,
            true,
        );
        assert_eq!(a.code, None);
        assert_eq!(a.local_name, "a");
        assert_eq!(
            a.fields.keys().collect::<Vec<_>>(),
            ["prefix", "namespace"],
            "in their order"
        );
        assert_eq!(text(&a.fields, "namespace").as_deref(), Some("urn:p"));
        let inner = a.scope.expect("a declaration changes the scope");
        let b = resolve_element("p:b", &attrs(&[]), &inner, true);
        assert_eq!(text(&b.fields, "namespace").as_deref(), Some("urn:p"));
        assert_eq!(b.scope, None, "no declaration: the scope passes on");
        // The scope survives the round trip a rule's `k` bag makes.
        assert_eq!(Scope::from_value(&inner.to_value()), inner);
    }

    #[test]
    fn space_and_lang_are_inherited_and_written_only_when_not_the_default() {
        let root = Scope::root();
        let a = resolve_element(
            "a",
            &attrs(&[("xml:space", "preserve"), ("xml:lang", "en")]),
            &root,
            false,
        );
        assert_eq!(a.fields.keys().collect::<Vec<_>>(), ["space", "lang"]);
        let b = resolve_element("b", &attrs(&[]), a.scope.as_ref().unwrap(), false);
        assert_eq!(text(&b.fields, "space").as_deref(), Some("preserve"));
        assert_eq!(text(&b.fields, "lang").as_deref(), Some("en"));
        let plain = resolve_element("c", &attrs(&[]), &root, false);
        assert!(plain.fields.is_empty());
        // The xml prefix is bound from the start.
        let x = resolve_element("xml:c", &attrs(&[]), &root, true);
        assert_eq!(text(&x.fields, "namespace").as_deref(), Some(XML_NS_URI));
    }

    #[test]
    fn unbound_prefixes_are_an_error_only_when_strict() {
        let root = Scope::root();
        let lenient = resolve_element("q:c", &attrs(&[]), &root, false);
        assert_eq!(lenient.code, None);
        assert_eq!(lenient.local_name, "c");
        assert_eq!(text(&lenient.fields, "namespace"), None);
        // Strict: the element keeps the prefix and its local name, and
        // nothing after them.
        let strict = resolve_element("q:c", &attrs(&[("xml:lang", "en")]), &root, true);
        assert_eq!(strict.code, Some("unbound_prefix"));
        assert_eq!(strict.local_name, "c");
        assert_eq!(strict.fields.keys().collect::<Vec<_>>(), ["prefix"]);
        // An unbound attribute prefix stops before the element's names.
        let attribute = resolve_element("q:c", &attrs(&[("z:k", "v")]), &root, true);
        assert_eq!(attribute.code, Some("unbound_prefix"));
        assert_eq!(attribute.local_name, "q:c");
        assert!(attribute.fields.is_empty());
    }

    #[test]
    fn reserved_prefixes_and_spaces_in_names_are_rejected() {
        let root = Scope::root();
        let reserved = resolve_element("a", &attrs(&[("xmlns:xmlns", "urn:x")]), &root, false);
        assert_eq!(reserved.code, Some("reserved_namespace"));
        assert_eq!(reserved.local_name, "a");
        assert!(reserved.fields.is_empty());
        let spaced = resolve_element("a", &attrs(&[("xmlns", "urn:x y")]), &root, false);
        assert_eq!(spaced.code, Some("invalid_namespace_uri"));
        // The default namespace undeclared with xmlns="" names nothing.
        let undeclared = resolve_element("a", &attrs(&[("xmlns", "")]), &root, false);
        assert_eq!(undeclared.code, None);
        assert_eq!(text(&undeclared.fields, "namespace"), None);
    }
}
