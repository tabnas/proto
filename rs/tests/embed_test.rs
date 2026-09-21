// The embedded grammar is the grammar.
//
// `proto-grammar/*.abnf` at the repository root is the single source of
// truth, and `ts/embed-grammar.js` writes it into `ts/src/grammar.ts` and
// `rs/src/grammar.rs`, while `go/grammar_gen.go` writes the identical
// string into `go/grammar.go`. This holds the Rust copy to the files on
// disk AND to both other runtimes' copies, so an edit to a `.abnf` file
// that forgets the embed step, or a hand edit to a generated file, fails
// here rather than shipping a runtime that compiles a different language
// from the other two.

mod common;

use std::fs;
use std::path::Path;

use common::repo_dir;

/// common first (it defines every base rule); the deltas extend it with
/// `name =/ alt`. The same order `ts/embed-grammar.js` and
/// `go/grammar_gen.go` use.
const ORDER: &[&str] = &["common", "proto2", "proto3", "edition-2023", "edition-2024"];

/// The grammar as the embedders assemble it: a leading newline, then each
/// file behind a banner comment, joined by newlines.
fn assembled() -> String {
    let dir = repo_dir().join("proto-grammar");
    let parts: Vec<String> = ORDER
        .iter()
        .map(|name| {
            let path = dir.join(format!("{name}.abnf"));
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
            format!("; ===== {name}.abnf =====\n{text}")
        })
        .collect();
    format!("\n{}", parts.join("\n"))
}

#[test]
fn the_embedded_grammar_is_the_files_on_disk() {
    assert_eq!(
        tabnas_proto::GRAMMAR_TEXT,
        assembled(),
        "rs/src/grammar.rs differs from proto-grammar/*.abnf: run `node embed-grammar.js` \
         from ts/"
    );
}

#[test]
fn the_grammar_text_fits_the_raw_string() {
    // A Rust raw string `r#"..."#` ends at the first `"#`, so the embedder
    // refuses a grammar holding that pair. Pinned here so the two halves
    // of the claim agree.
    assert!(!tabnas_proto::GRAMMAR_TEXT.contains("\"#"));
}

// The embedded text is also identical in the other two runtimes, which is
// what makes the shared fixtures a parity contract rather than three
// separate suites that happen to agree. Read both copies out of their
// sources and compare them with this one.
#[test]
fn all_three_runtimes_embed_the_same_text() {
    let grammar = assembled();

    let ts_path = repo_dir().join("ts").join("src").join("grammar.ts");
    let ts = read(&ts_path);
    let body = between(&ts, "export const grammarText = `", "`\n")
        .unwrap_or_else(|| panic!("{} has no embedded grammar", ts_path.display()));
    assert_eq!(
        unescape_template_literal(body),
        grammar,
        "{} embeds a different grammar",
        ts_path.display()
    );

    let go_path = repo_dir().join("go").join("grammar.go");
    let go = read(&go_path);
    let quoted = between(&go, "const GrammarText = \"", "\"\n")
        .unwrap_or_else(|| panic!("{} has no embedded grammar", go_path.display()));
    assert_eq!(
        unescape_go_string(quoted),
        grammar,
        "{} embeds a different grammar",
        go_path.display()
    );
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()))
}

/// The text between the first `opener` and the LAST `closer` after it.
/// Last, not first: both embedded literals contain the closer's first
/// character throughout.
fn between<'a>(text: &'a str, opener: &str, closer: &str) -> Option<&'a str> {
    let start = text.find(opener)? + opener.len();
    let end = text[start..].rfind(closer)? + start;
    Some(&text[start..end])
}

/// Undo the escaping `embed-grammar.js` applies for a JavaScript template
/// literal: a backslash, a backtick and a template expression.
fn unescape_template_literal(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if '\\' != ch {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('`') => out.push('`'),
            Some('$') => out.push('$'),
            Some(other) => panic!("unexpected escape \\{other} in the TypeScript embed"),
            None => panic!("trailing backslash in the TypeScript embed"),
        }
    }
    out
}

/// Undo `strconv.Quote` for the escapes this grammar can produce. Any
/// other escape is a loud failure rather than a silent mis-read: the
/// point of the comparison is that the three texts are the same, and a
/// decoder that guesses would undermine it.
fn unescape_go_string(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if '\\' != ch {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => panic!("unexpected escape \\{other} in the Go embed"),
            None => panic!("trailing backslash in the Go embed"),
        }
    }
    out
}

// The embedder is the only writer of rs/src/grammar.rs, and it is guarded
// on the Rust crate being present so a checkout that predates this port
// still embeds the other runtimes. Both halves of that are asserted:
// the generated file says who wrote it, and the embedder still carries
// the guard.
#[test]
fn the_generated_file_names_its_generator() {
    let generated = read(&repo_dir().join("rs").join("src").join("grammar.rs"));
    assert!(
        generated.contains("GENERATED by ts/embed-grammar.js"),
        "rs/src/grammar.rs does not say it is generated"
    );
    let embedder = read(&repo_dir().join("ts").join("embed-grammar.js"));
    assert!(
        embedder.contains("fs.existsSync(RS_DIR)"),
        "ts/embed-grammar.js no longer guards the Rust block on the crate being present"
    );
}
