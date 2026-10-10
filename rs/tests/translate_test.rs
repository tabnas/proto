// The translation parts: what the manifest says and what the crate
// embeds are the same files.
//
// A packaged crate holds nothing outside `rs/`, so the crate embeds its
// own copies, `rs/translate/manifest.json` of `tabnas.plugin.json` and
// `rs/translate/render.alc` of the render the manifest names, as
// `manifest_text()` and `render_text()`. The copies are the only texts a
// host sees, so they must be the files: this holds the embedded manifest
// to the repository's, and the render the manifest names, read from the
// repository, to the embedded one, as it would an embed the manifest
// named. Change the file at the root and run `npm run embed` in `ts/`,
// which copies it into `rs/translate/`; this fails until both are the
// same.

mod common;

use std::fs;

use serde_json::Value;

fn translate() -> Value {
    let manifest: Value =
        serde_json::from_str(tabnas_proto::manifest_text()).expect("the manifest is JSON");
    manifest
        .get("translate")
        .cloned()
        .expect("the manifest carries a translate object")
}

#[test]
fn the_manifest_the_crate_embeds_is_the_repositorys() {
    let on_disk = fs::read_to_string(common::repo_dir().join("tabnas.plugin.json"))
        .expect("the repository has its manifest");
    assert_eq!(
        on_disk,
        tabnas_proto::manifest_text(),
        "rs/translate/manifest.json is not tabnas.plugin.json: run npm run embed in ts"
    );
}

#[test]
fn the_render_the_manifest_names_is_the_one_the_crate_embeds() {
    let translate = translate();
    let path = translate["render"]
        .as_str()
        .expect("translate.render names a file");
    let on_disk = fs::read_to_string(common::repo_dir().join(path))
        .unwrap_or_else(|e| panic!("translate.render names {path}, which cannot be read: {e}"));
    assert_eq!(
        on_disk,
        tabnas_proto::render_text(),
        "translate.render names {path}, and rs/translate/render.alc, which render_text() \
         embeds, is another text: run npm run embed in ts"
    );
}

/// An embed takes a plain tree into a format's own schema. proto's render
/// writes the descriptor the reader builds and nothing else, so its
/// manifest names no embed and the crate carries none; a manifest that
/// named one would be held to its file here, as the render is above.
#[test]
fn the_embed_the_manifest_names_is_the_one_the_crate_embeds() {
    let translate = translate();
    let parts = tabnas_proto::translate().expect("proto carries translation parts");
    let Some(path) = translate.get("embed").and_then(Value::as_str) else {
        assert_eq!(
            parts.embed, None,
            "the manifest names no embed, and the crate carries one"
        );
        return;
    };
    let on_disk = fs::read_to_string(common::repo_dir().join(path))
        .unwrap_or_else(|e| panic!("translate.embed names {path}, which cannot be read: {e}"));
    let embed = parts
        .embed
        .unwrap_or_else(|| panic!("translate.embed names {path}, and the crate carries no embed"));
    assert_eq!(embed.entry, "proto-embed");
    assert_eq!(
        embed.source,
        Some(on_disk.as_str()),
        "translate.embed names {path}, and the crate embeds another text: run npm run embed in ts"
    );
}

#[test]
fn the_structural_interface_names_the_render_entry() {
    let parts = tabnas_proto::translate().expect("proto carries translation parts");
    assert_eq!(parts.manifest, tabnas_proto::manifest_text());
    assert_eq!(parts.lift, None);
    let render = parts.render.expect("proto carries a render");
    assert_eq!(render.entry, "proto-render");
    assert_eq!(render.source, Some(tabnas_proto::render_text()));
}

/// A `.proto` file is read as a tree and written from one, the
/// descriptor's own shape, which the manifest names as its schema; the
/// render needs the descriptor, an object, at the root.
#[test]
fn proto_reads_and_writes_its_descriptor_with_no_lift() {
    let translate = translate();
    assert_eq!(translate["reads"], "tree");
    assert_eq!(translate["writes"], "tree");
    assert_eq!(translate["root"], "object");
    assert_eq!(translate["schema"], "proto-descriptor");
    assert_eq!(translate.get("lift"), None);
}

/// The host prints the loss lines verbatim, so each is a sentence.
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
}

/// A host links the render with its own program and other formats'
/// parts, so every definition is named for proto, the entry point is
/// `proto-render`, and the file defines no `export` of its own.
#[test]
fn the_render_is_a_library_named_for_proto() {
    let names: Vec<&str> = tabnas_proto::render_text()
        .lines()
        .filter_map(|line| line.strip_prefix("def "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect();
    assert!(names.contains(&"proto-render"), "{names:?}");
    for name in &names {
        assert!(name.starts_with("proto-"), "{name} is not named for proto");
    }
}
