// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! The compiled grammar, and the generator that writes it.
//!
//! `rs/proto-grammar.json` is the engine's serialized rule set for the
//! union grammar, compiled from `GRAMMAR_TEXT` by `tabnas-abnf` at build
//! time; `src/lib.rs` embeds it with `include_str!` and installs it, so the
//! crate never runs the compiler and `tabnas-abnf` is a dev-dependency
//! only. This test compiles the grammar again and fails when the committed
//! file is stale. With `TABNAS_WRITE_GRAMMAR=1` in the environment it
//! writes the file instead, which is how the file is (re)generated:
//!
//! ```bash
//! TABNAS_WRITE_GRAMMAR=1 cargo test --test grammar_spec_test
//! ```
//!
//! The compile is the plugin's former install with `builtins` on, so the
//! `{rule, src, kids}` builders are the engine's own `@node$` /
//! `@capture$` / `@bubble$` builtins rather than closures, and
//! `to_pure_spec` refuses any closure. It is this crate's own compiler's
//! output, not the TypeScript port's file: that compiler guards a
//! whole-word keyword with a lookahead, which the `regex` crate does not
//! have, where this one writes `\b` (see the root `AGENTS.md`, "The
//! compiled grammar").

use std::fs;
use std::path::Path;

use tabnas_abnf::{abnf_convert, to_jsonic, to_pure_spec, AbnfConvertOptions, JsonicOptions};

/// The committed file, beside `Cargo.toml`.
fn spec_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("proto-grammar.json")
}

/// The compiled grammar as the text of `proto-grammar.json`.
fn compile() -> String {
    // The options the plugin passed at every install, plus `builtins`.
    // `word_keywords` and `token_classes` are both required; the root
    // `AGENTS.md`, "Grammar conventions", says why.
    let convert = AbnfConvertOptions {
        start: Some("proto".to_string()),
        tag: Some("proto".to_string()),
        word_keywords: true,
        token_classes: true,
        builtins: true,
        ..AbnfConvertOptions::default()
    };
    let spec =
        abnf_convert(tabnas_proto::GRAMMAR_TEXT, Some(&convert)).expect("the grammar compiles");
    let document = to_pure_spec(&spec).expect("the compiled grammar is pure data");
    let mut text = to_jsonic(
        &document,
        JsonicOptions {
            strict: true,
            indent: Some(2),
        },
    );
    text.push('\n');
    text
}

#[test]
fn the_compiled_grammar_is_current() {
    let compiled = compile();
    if std::env::var("TABNAS_WRITE_GRAMMAR").as_deref() == Ok("1") {
        fs::write(spec_path(), &compiled).expect("rs/proto-grammar.json is writable");
        return;
    }
    let committed = fs::read_to_string(spec_path()).expect("rs/proto-grammar.json is readable");
    assert!(
        committed == compiled,
        "rs/proto-grammar.json is stale: run `TABNAS_WRITE_GRAMMAR=1 cargo test \
         --test grammar_spec_test` from rs/ and commit the result"
    );
}

#[test]
fn the_compiled_grammar_names_only_engine_builtins() {
    let text = fs::read_to_string(spec_path()).expect("rs/proto-grammar.json is readable");
    let document: serde_json::Value = serde_json::from_str(&text).expect("it is JSON");
    assert!(
        document.get("ref").is_none(),
        "a pure spec carries no ref map"
    );
    // Every `@` string in the rules is a `$` builtin: a closure would have
    // to arrive by name from a ref map, and there is none.
    fn walk(value: &serde_json::Value, named: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) if s.starts_with('@') => named.push(s.clone()),
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, named)),
            serde_json::Value::Object(map) => map.values().for_each(|v| walk(v, named)),
            _ => {}
        }
    }
    let mut named = Vec::new();
    walk(&document["rule"], &mut named);
    assert!(!named.is_empty());
    for name in &named {
        assert!(name.ends_with('$'), "{name} is not an engine builtin");
    }
    for builder in ["@node$", "@capture$", "@bubble$"] {
        assert!(named.iter().any(|name| name == builder), "{builder}");
    }
}
