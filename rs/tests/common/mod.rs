// Shared test helpers. Cargo compiles this module into EVERY integration
// test binary, so an item only one binary uses is dead code in the
// others; the allow keeps that from being a warning rather than hiding
// anything real.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use tabnas_proto::{FileDescriptorProto, ProtoError, ProtoOptions, MAX_NESTING_DEPTH};
use tabnas_support::{find_spec_dir, Failure, Row, Value};

/// The shared `test/spec` directory, found by walking up from the crate
/// rather than by counting `..` hops.
pub fn spec_dir() -> PathBuf {
    find_spec_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .expect("a test/spec directory above rs/")
}

/// The repository root, one level above `test/`.
pub fn repo_dir() -> PathBuf {
    spec_dir()
        .parent()
        .and_then(Path::parent)
        .expect("test/spec sits two levels below the repository root")
        .to_path_buf()
}

/// The vendored protoc parser corpus, beside `test/spec`.
pub fn suite_dir() -> PathBuf {
    spec_dir()
        .parent()
        .expect("test/spec has a parent")
        .join("protobuf-suite")
}

/// A descriptor as the fixture data model, through JSON.
///
/// This is the Rust half of the Go runner's `jsonFlatten` and the
/// TypeScript runner's round trip, so a value compares the same way in
/// all three: absent fields and key order do not affect the comparison,
/// and a number no JSON can hold arrives as `null` in every runtime.
pub fn to_value(file: &FileDescriptorProto) -> Value {
    Value::from(serde_json::to_value(file).expect("a descriptor is JSON"))
}

/// A refusal as the runner's failure.
///
/// This package declares no error codes of its own, so the code is the
/// engine's where there is one and empty where the plugin refused on its
/// own account; the fixture's one `ERROR:` row pins a fragment of the
/// MESSAGE, which `parity_test.rs` wires up.
pub fn to_failure(error: ProtoError) -> Failure {
    let failure = Failure::new(error.code()).with_message(error.to_string());
    match error.position() {
        Some((row, col)) => failure.at(row, col),
        None => failure,
    }
}

/// A row's `opts` column as the plugin's options; an empty cell means the
/// defaults.
pub fn row_options(row: &Row) -> Option<ProtoOptions> {
    let raw = row.named("opts");
    if raw.trim().is_empty() {
        return None;
    }
    Some(
        serde_json::from_str::<ProtoOptions>(raw)
            .unwrap_or_else(|error| panic!("{}: bad opts cell {raw:?}: {error}", row.location())),
    )
}

/// Parse one row's input through the crate's shared instance, reading the
/// row's `opts` column, as the Go and TypeScript fixture runners do.
pub fn parse_row(input: &str, row: &Row) -> Result<Value, Failure> {
    let options = row_options(row);
    tabnas_proto::parse(input, options.as_ref())
        .map(|file| to_value(&file))
        .map_err(to_failure)
}

// ---------------------------------------------------------------------
// The documents the window, streaming and history tests read
// ---------------------------------------------------------------------

/// The input of every row of every fixture file of `dir`.
fn rows(dir: PathBuf, into: &mut Vec<(String, String)>) {
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "tsv"))
        .collect();
    files.sort();
    for file in files {
        let text = fs::read_to_string(&file).expect("a fixture file reads");
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        for (index, line) in text.lines().enumerate() {
            if index == 0 || line.is_empty() || (line.starts_with('#') && !line.contains('\t')) {
                continue;
            }
            let input = line.split('\t').next().unwrap_or_default();
            into.push((
                format!("{name}:{}", index + 1),
                tabnas_support::unescape(input),
            ));
        }
    }
}

/// The input of every row of the shared fixtures, every input of the
/// protoc corpus and its leniency probes, and descriptor.proto.
pub fn documents() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let test = repo_dir().join("test");
    rows(spec_dir(), &mut out);
    rows(test.join("passes"), &mut out);
    rows(test.join("tree"), &mut out);
    for file in ["valid", "invalid", "accept-only"] {
        let path = suite_dir().join(format!("{file}.json"));
        let cases: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("the corpus reads"))
                .expect("the corpus is JSON");
        for (index, case) in cases
            .as_array()
            .expect("a list of cases")
            .iter()
            .enumerate()
        {
            let input = case["input"].as_str().expect("a case has an input");
            out.push((format!("{file}:{index}"), input.to_string()));
        }
    }
    let leniency: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(suite_dir().join("leniency.json")).expect("the probes read"),
    )
    .expect("the probes are JSON");
    for (index, probe) in leniency["probes"]
        .as_array()
        .expect("a list of probes")
        .iter()
        .enumerate()
    {
        if let Some(input) = probe["input"].as_str() {
            out.push((format!("leniency:{index}"), input.to_string()));
        }
    }
    let descriptor = test.join("descriptor").join("descriptor.proto");
    out.push((
        "descriptor.proto".to_string(),
        fs::read_to_string(descriptor).expect("descriptor.proto reads"),
    ));
    out
}

/// A `.proto` file of every kind of token, the long ones longer than the
/// engine's 64 KiB window, with characters past ASCII in its comments and
/// strings and aggregate option values of every shape, so that tokens of
/// every kind straddle the window's edges as a reader hands the text over.
/// The same document as `longDocument` in ts/test/documents.ts and
/// go/window_test.go.
pub fn long_document() -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "// {}\r\n",
        "a comment past the window, \u{e9} \u{1d11e}; ".repeat(2000)
    ));
    parts.push(format!(
        "/* {}*/\r\n",
        "a block comment past the window\r\n\t".repeat(2000)
    ));
    parts.push("syntax = \"proto2\";\r\npackage a.b.c;\r\n".into());
    parts.push(format!(
        "option long = \"{}\";\r\n",
        "a string past the window \u{fc} ".repeat(3000)
    ));
    parts.push(format!(
        "option tick = '{}';\r\n",
        "single quotes past the window ".repeat(3000)
    ));
    parts.push(format!("option (named) = {};\r\n", "n".repeat(70000)));
    parts.push(format!(
        "option (big) = {{\r\n{}}};\r\n",
        "  a: 1 /* c\t*/ b { c: \"d\" } # e\r\n\t".repeat(2000)
    ));
    parts.push(format!(
        "message {} {{ optional int32 f = 1; }}\r\n",
        "N".repeat(70000)
    ));
    for i in 0..250 {
        parts.push(format!("message M{i} {{ // r\u{e9}gle {i}\r\n"));
        parts.push(format!(
            "  optional int32 a{i} = {} [default = -0x{i:X}, (o) = {{ x: {i} y: \"z\" }}];\r\n",
            i + 1
        ));
        parts.push(format!(
            "  repeated string optionals = {};\r\n  required group G{i} = {} {{ optional bool b = 1; }}\r\n",
            i + 2,
            i + 3
        ));
        parts.push(format!(
            "  enum E{i} {{ option allow_alias = true; V{i} = 0; W{i} = 1 [deprecated = true]; }}\r\n"
        ));
        parts.push(format!(
            "  oneof o{i} {{ int64 one = {}; }}\r\n  map<string, int32> m{i} = {};\r\n",
            i + 4,
            i + 5
        ));
        parts.push(format!(
            "  extensions 100 to max;\r\n  reserved 50 to 60;\r\n  reserved \"res{i}\";\r\n}}\r\n"
        ));
        parts.push(format!(
            "service S{i} {{ rpc R{i}(stream M{i}) returns (M{i}) {{ option (x) = {{a: 1}}; }} }}\r\n"
        ));
    }
    parts.push("extend M0 { optional int32 ext = 1000; }\r\n".into());
    parts.concat()
}

/// Long documents that fail: a token that runs past the engine's window and
/// is then refused, or never ends, and nesting past the cap after a long
/// run.
pub fn long_error_documents() -> Vec<String> {
    let pad = format!(
        "// {}\r\nsyntax = \"proto3\";\r\n",
        "padding \u{fc} ".repeat(9000)
    );
    let cap = MAX_NESTING_DEPTH + 1;
    vec![
        format!("{pad}option a = \"{}\r\n", "unclosed string ".repeat(6000)),
        format!("{pad}/* {}\r\n", "an unclosed comment ".repeat(4000)),
        format!("{pad}message M {{ {}\r\n", "int32 a = 1; ".repeat(6000)),
        format!("{pad}option b = {{{} ) }};\r\n", " c: 1".repeat(15000)),
        format!("{pad}message {} }} \r\n", "m".repeat(70000)),
        format!(
            "{pad}option (x) = {}{};\r\n",
            "{a ".repeat(cap),
            "}".repeat(cap)
        ),
        format!("{pad}{}{}", "message M{".repeat(cap), "}".repeat(cap)),
        format!("{pad}{}", "message M {".repeat(100_000)),
    ]
}

/// Every document the window, streaming and history tests read: the
/// fixtures and the corpus, the long document and the long documents that
/// fail.
pub fn every_document() -> Vec<(String, String)> {
    let mut all = documents();
    all.push(("the long document".to_string(), long_document()));
    for (index, text) in long_error_documents().into_iter().enumerate() {
        all.push((format!("long error document {index}"), text));
    }
    all
}
