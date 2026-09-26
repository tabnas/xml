/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

//! Namespace resolution (Namespaces in XML 1.0) and the inherited
//! `xml:space` / `xml:lang` values (XML 1.0 2.10 and 2.12), walked over
//! the finished element tree exactly as `resolveNamespaces` in
//! `ts/src/xml.ts` walks it.

use std::collections::HashMap;
use std::rc::Rc;

use tabnas::Value;

/// The `xml` prefix is bound to this URI and may be used implicitly.
pub(crate) const XML_NS_URI: &str = "http://www.w3.org/XML/1998/namespace";
/// The `xmlns` prefix is reserved and must never be declared.
pub(crate) const XMLNS_NS_URI: &str = "http://www.w3.org/2000/xmlns/";

/// State inherited down the tree: prefix to namespace name (the empty
/// prefix is the default namespace), the active `xml:space`, and the
/// active `xml:lang`.
struct Scope {
    ns: HashMap<String, String>,
    space: String,
    lang: String,
}

/// A namespace name is a URI reference (RFC 3986), and a URI reference
/// cannot contain white space. Attribute-value normalisation has already
/// turned a literal TAB/LF/CR in the declaration into a space by the time
/// the value gets here.
fn invalid_namespace_uri(uri: &str) -> bool {
    uri.bytes()
        .any(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
}

/// Annotate `element` and its descendants with `prefix`, `localName`,
/// `namespace`, `space` and `lang`. `Err` carries the code of the first
/// reserved-prefix, invalid-namespace-name or (when `strict`)
/// unbound-prefix violation; the tree may then be partly annotated.
pub(crate) fn resolve_namespaces(element: &mut Value, strict: bool) -> Result<(), &'static str> {
    let mut ns = HashMap::new();
    ns.insert("xml".to_string(), XML_NS_URI.to_string());
    let scope = Scope {
        ns,
        space: "default".to_string(),
        lang: String::new(),
    };
    resolve_scope(element, scope, strict)
}

fn attribute_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Walk the tree from `root` with a stack of its own rather than by
/// recursion: a document nested some thousands of elements deep would
/// otherwise overflow the thread's stack, which ends the process where no
/// error can be caught. Children go on in reverse, so they come off in
/// document order: the walk is the pre-order a recursion makes, with the
/// same first error and the same partial annotation.
fn resolve_scope(root: &mut Value, scope: Scope, strict: bool) -> Result<(), &'static str> {
    let mut pending = vec![(root, Rc::new(scope))];
    while let Some((element, scope)) = pending.pop() {
        resolve_element(element, &scope, strict, &mut pending)?;
    }
    Ok(())
}

/// The elements still to resolve, each with the scope it inherits.
type Pending<'a> = Vec<(&'a mut Value, Rc<Scope>)>;

/// Resolve one element against the scope it inherits, and queue its
/// element children, last first, with the scope it passes on.
fn resolve_element<'a>(
    element: &'a mut Value,
    scope: &Scope,
    strict: bool,
    pending: &mut Pending<'a>,
) -> Result<(), &'static str> {
    let Some(map) = element.as_object_mut() else {
        return Ok(());
    };
    let mut ns = scope.ns.clone();
    let mut space = scope.space.clone();
    let mut lang = scope.lang.clone();

    if let Some(Value::Object(attrs)) = map.get("attributes") {
        // Pass 1: every namespace declaration on this element, plus
        // xml:space / xml:lang. Namespaces in XML 1.0 5.2 scopes a
        // declaration over the whole element it appears on, INCLUDING
        // that element's own other attributes, so every declaration must
        // be in hand before any prefixed name is resolved.
        for (key, value) in attrs.iter() {
            let text = attribute_text(value);
            if key == "xmlns" {
                if text == XML_NS_URI || text == XMLNS_NS_URI {
                    return Err("reserved_namespace");
                }
                if invalid_namespace_uri(&text) {
                    return Err("invalid_namespace_uri");
                }
                ns.insert(String::new(), text);
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                match prefix {
                    "xml" => {
                        if text != XML_NS_URI {
                            return Err("reserved_namespace");
                        }
                    }
                    "xmlns" => return Err("reserved_namespace"),
                    _ => {
                        if text == XML_NS_URI || text == XMLNS_NS_URI {
                            return Err("reserved_namespace");
                        }
                    }
                }
                if invalid_namespace_uri(&text) {
                    return Err("invalid_namespace_uri");
                }
                ns.insert(prefix.to_string(), text);
            } else if key == "xml:space" {
                space = text;
            } else if key == "xml:lang" {
                lang = text;
            }
        }

        // Pass 2: prefixed attribute names against the completed in-scope
        // declarations. A namespace constraint only: an unbound prefix on
        // an attribute leaves the document XML 1.0 well-formed.
        if strict {
            for key in attrs.keys() {
                if key == "xmlns" || key.starts_with("xmlns:") {
                    continue;
                }
                if let Some((prefix, _)) = key.split_once(':') {
                    if !prefix.is_empty() && !ns.contains_key(prefix) {
                        return Err("unbound_prefix");
                    }
                }
            }
        }
    }

    let name = match map.get("name") {
        Some(Value::String(name)) => name.clone(),
        _ => String::new(),
    };
    if let Some((prefix, local)) = name.split_once(':') {
        map.insert("prefix".to_string(), Value::String(prefix.to_string()));
        map.insert("localName".to_string(), Value::String(local.to_string()));
        if let Some(uri) = ns.get(prefix) {
            map.insert("namespace".to_string(), Value::String(uri.clone()));
        } else if strict {
            return Err("unbound_prefix");
        }
        // Not strict: `namespace` stays unset. The element is named but
        // unqualified, which is what a namespace-unaware (yet XML 1.0
        // conformant) consumer sees.
    } else {
        map.insert("localName".to_string(), Value::String(name));
        if let Some(uri) = ns.get("").filter(|uri| !uri.is_empty()) {
            map.insert("namespace".to_string(), Value::String(uri.clone()));
        }
    }

    if space != "default" {
        map.insert("space".to_string(), Value::String(space.clone()));
    }
    if !lang.is_empty() {
        map.insert("lang".to_string(), Value::String(lang.clone()));
    }

    let child_scope = Rc::new(Scope { ns, space, lang });
    if let Some(children) = map.get_mut("children").and_then(Value::as_array_mut) {
        for child in children.iter_mut().rev() {
            if matches!(child, Value::Object(_)) {
                pending.push((child, Rc::clone(&child_scope)));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn element(name: &str, attrs: &[(&str, &str)], children: Vec<Value>) -> Value {
        let mut map = IndexMap::new();
        map.insert("name".to_string(), Value::String(name.to_string()));
        map.insert("localName".to_string(), Value::String(name.to_string()));
        let attrs: IndexMap<String, Value> = attrs
            .iter()
            .map(|(key, value)| ((*key).to_string(), Value::String((*value).to_string())))
            .collect();
        map.insert("attributes".to_string(), Value::object(attrs));
        map.insert("children".to_string(), Value::array(children));
        Value::object(map)
    }

    fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
        match value {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// A tree `depth` elements deep, built from the inside out: the
    /// innermost element is `innermost`, and `xmlns:p` is declared on the
    /// outermost.
    fn deep(depth: usize, innermost: &str) -> Value {
        let mut tree = element(innermost, &[], vec![]);
        for level in 1..depth {
            let declared: &[(&str, &str)] = if level + 1 == depth {
                &[("xmlns:p", "urn:p")]
            } else {
                &[]
            };
            tree = element("p:a", declared, vec![tree]);
        }
        tree
    }

    /// The innermost element, reached a level at a time.
    fn innermost(tree: &Value) -> &Value {
        let mut here = tree;
        while let Some(Value::Array(children)) = field(here, "children") {
            match children.first() {
                Some(child) => here = child,
                None => break,
            }
        }
        here
    }

    /// Take a tree apart a level at a time: a value drops by recursion, and
    /// one this deep would overflow the stack doing it.
    fn dismantle(tree: Value) {
        let mut pending = vec![tree];
        while let Some(mut value) = pending.pop() {
            let children = value
                .as_object_mut()
                .and_then(|map| map.get_mut("children"))
                .and_then(Value::as_array_mut);
            if let Some(children) = children {
                pending.append(children);
            }
        }
    }

    #[test]
    fn a_deep_tree_is_walked_without_the_call_stack() {
        // The walk used to recurse once per level, which ended the process
        // with a stack overflow some thousands of levels deep (tabnas/xml#68).
        // It keeps a stack of its own now, so 20,000 levels resolve on a
        // thread with 256 KiB of stack, where each level used to take a
        // frame. The tree is built, walked and taken apart on that thread.
        let walk = |innermost_name: &'static str| {
            std::thread::Builder::new()
                .stack_size(256 << 10)
                .spawn(move || {
                    let mut tree = deep(20_000, innermost_name);
                    let outcome = resolve_namespaces(&mut tree, true);
                    let deepest = innermost(&tree);
                    let found = ["namespace", "localName"].map(|key| match field(deepest, key) {
                        Some(Value::String(text)) => Some(text.clone()),
                        _ => None,
                    });
                    dismantle(tree);
                    (outcome, found)
                })
                .expect("spawns")
                .join()
                .expect("walked without overflowing the stack")
        };
        let (outcome, [namespace, local_name]) = walk("p:z");
        assert_eq!(outcome, Ok(()));
        assert_eq!(namespace.as_deref(), Some("urn:p"));
        assert_eq!(local_name.as_deref(), Some("z"));
        // The first error is still the one a pre-order walk meets first:
        // here, the innermost element's unbound prefix.
        let (outcome, _) = walk("q:z");
        assert_eq!(outcome, Err("unbound_prefix"));
    }

    #[test]
    fn a_declaration_scopes_over_the_element_and_its_children() {
        let mut root = element(
            "p:a",
            &[("xmlns:p", "urn:p"), ("p:x", "1")],
            vec![element("p:b", &[], vec![]), Value::String("t".into())],
        );
        assert_eq!(resolve_namespaces(&mut root, true), Ok(()));
        assert_eq!(field(&root, "prefix"), Some(&Value::String("p".into())));
        assert_eq!(
            field(&root, "namespace"),
            Some(&Value::String("urn:p".into()))
        );
        let Some(Value::Array(children)) = field(&root, "children") else {
            panic!("children")
        };
        assert_eq!(
            field(&children[0], "namespace"),
            Some(&Value::String("urn:p".into()))
        );
    }

    #[test]
    fn unbound_prefixes_are_an_error_only_when_strict() {
        let mut lenient = element("q:c", &[], vec![]);
        assert_eq!(resolve_namespaces(&mut lenient, false), Ok(()));
        assert_eq!(
            field(&lenient, "localName"),
            Some(&Value::String("c".into()))
        );
        assert_eq!(field(&lenient, "namespace"), None);
        let mut strict = element("q:c", &[], vec![]);
        assert_eq!(resolve_namespaces(&mut strict, true), Err("unbound_prefix"));
    }

    #[test]
    fn reserved_prefixes_and_spaces_in_names_are_rejected() {
        let mut root = element("a", &[("xmlns:xmlns", "urn:x")], vec![]);
        assert_eq!(
            resolve_namespaces(&mut root, false),
            Err("reserved_namespace")
        );
        let mut root = element("a", &[("xmlns", "urn:x y")], vec![]);
        assert_eq!(
            resolve_namespaces(&mut root, false),
            Err("invalid_namespace_uri")
        );
    }
}
