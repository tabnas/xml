// The translation parts: what the manifest says and what the crate
// embeds are the same files.
//
// A packaged crate holds nothing outside `rs/`, so the crate embeds its
// own copies, `rs/translate/manifest.json` of `tabnas.plugin.json`,
// `rs/translate/render.alc` of the render the manifest names and
// `rs/translate/embed.alc` of the embedding it names, as
// `manifest_text()`, `render_text()` and `translate().embed`. The copies
// are the only texts a host sees, so they must be the files: this holds
// the embedded manifest to the repository's, and the render and the
// embedding the manifest names, read from the repository, to the
// embedded ones. Change the file at the root and run `npm run embed`
// (from `ts/`), which copies it into `rs/translate/`; this fails until
// both are the same.

mod common;

use std::fs;

use serde_json::Value;

fn translate() -> Value {
    let manifest: Value =
        serde_json::from_str(tabnas_xml::manifest_text()).expect("the manifest is JSON");
    manifest
        .get("translate")
        .cloned()
        .expect("the manifest carries a translate object")
}

#[test]
fn the_manifest_the_crate_embeds_is_the_repositorys() {
    let on_disk = fs::read_to_string(common::repo_root().join("tabnas.plugin.json"))
        .expect("the repository has its manifest");
    assert_eq!(
        on_disk,
        tabnas_xml::manifest_text(),
        "rs/translate/manifest.json is not tabnas.plugin.json: copy the manifest into rs/translate"
    );
}

#[test]
fn the_render_the_manifest_names_is_the_one_the_crate_embeds() {
    let translate = translate();
    let path = translate["render"]
        .as_str()
        .expect("translate.render names a file");
    let on_disk = fs::read_to_string(common::repo_root().join(path))
        .unwrap_or_else(|e| panic!("translate.render names {path}, which cannot be read: {e}"));
    assert_eq!(
        on_disk,
        tabnas_xml::render_text(),
        "translate.render names {path}, and rs/translate/render.alc, which render_text() \
         embeds, is another text: copy the render into rs/translate"
    );
}

#[test]
fn the_embed_the_manifest_names_is_the_one_the_crate_embeds() {
    let translate = translate();
    let path = translate["embed"]
        .as_str()
        .expect("translate.embed names a file");
    let on_disk = fs::read_to_string(common::repo_root().join(path))
        .unwrap_or_else(|e| panic!("translate.embed names {path}, which cannot be read: {e}"));
    let parts = tabnas_xml::translate().expect("XML carries translation parts");
    let embed = parts.embed.expect("XML carries an embedding");
    assert_eq!(
        Some(on_disk.as_str()),
        embed.source,
        "translate.embed names {path}, and rs/translate/embed.alc, which the crate \
         embeds, is another text: run npm run embed"
    );
}

#[test]
fn the_structural_interface_names_the_render_and_embed_entries() {
    let parts = tabnas_xml::translate().expect("XML carries translation parts");
    assert_eq!(parts.manifest, tabnas_xml::manifest_text());
    assert_eq!(parts.lift, None);
    let render = parts.render.expect("XML carries a render");
    assert_eq!(render.entry, "xml-render");
    assert_eq!(render.source, Some(tabnas_xml::render_text()));
    let embed = parts.embed.expect("XML carries an embedding");
    assert_eq!(embed.entry, "xml-embed");
    assert!(embed.source.is_some(), "the embedding is XML's own file");
}

/// XML is read as a tree and written from one: the element tree its
/// reader builds, whose events carry it already, so there is no lift,
/// and no accessor for one. That tree has a schema of its own, which a
/// plain tree reaches through the embedding, and the render takes a
/// root of any kind.
#[test]
fn xml_reads_and_writes_a_tree_with_no_lift() {
    let translate = translate();
    assert_eq!(translate["reads"], "tree");
    assert_eq!(translate["writes"], "tree");
    assert_eq!(translate["root"], "any");
    assert_eq!(translate["schema"], "xml-element");
    assert_eq!(translate["embed"], "alchemy/embed.alc");
    assert_eq!(translate.get("lift"), None);
}

/// The host prints the loss lines verbatim, so each is a sentence. The
/// render writes the reader's element tree and no other, so the first
/// says which trees it carries.
#[test]
fn the_loss_is_a_list_of_sentences() {
    let translate = translate();
    let loss = translate["loss"]
        .as_array()
        .expect("translate.loss is a list");
    assert!(!loss.is_empty());
    for line in loss {
        let line = line.as_str().expect("each loss line is a string");
        assert!(
            line.starts_with(char::is_uppercase) && line.ends_with('.'),
            "{line:?} is not a sentence"
        );
    }
    let first = loss[0].as_str().expect("the first loss line is a string");
    assert!(
        first.contains("name, localName, attributes and children"),
        "the first loss line does not say which trees the render writes: {first:?}"
    );
    assert!(
        loss.iter()
            .filter_map(Value::as_str)
            .any(|line| line.contains("the element document")
                && line.contains("member elements")
                && line.contains("item elements")),
        "no loss line says how the embedding writes a plain tree: {loss:?}"
    );
}

/// The names a library of alchemy definitions defines, in its order.
fn definitions(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.strip_prefix("def "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect()
}

/// A host links the render with its own program and other formats'
/// parts, so every definition is named for XML, the entry point is
/// `xml-render`, and the file defines no `export` of its own.
#[test]
fn the_render_is_a_library_named_for_xml() {
    let names = definitions(tabnas_xml::render_text());
    assert!(names.contains(&"xml-render"), "{names:?}");
    for name in &names {
        assert!(name.starts_with("xml-"), "{name} is not named for XML");
    }
}

/// The embedding is linked beside the render, so it is a library named
/// for XML too, with no `export`, and with both entry points: the
/// embedding and its reverse. None of its names is one of the render's,
/// since the two are linked into one namespace.
#[test]
fn the_embed_is_a_library_named_for_xml() {
    let parts = tabnas_xml::translate().expect("XML carries translation parts");
    let embed = parts
        .embed
        .and_then(|part| part.source)
        .expect("XML carries an embedding");
    let names = definitions(embed);
    assert!(names.contains(&"xml-embed"), "{names:?}");
    assert!(names.contains(&"xml-unembed"), "{names:?}");
    let render = definitions(tabnas_xml::render_text());
    for name in &names {
        assert!(name.starts_with("xml-"), "{name} is not named for XML");
        assert!(
            !render.contains(name),
            "{name} is defined by the render too"
        );
    }
}
