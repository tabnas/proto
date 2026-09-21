// Shared test helpers. Cargo compiles this module into EVERY integration
// test binary, so an item only one binary uses is dead code in the
// others; the allow keeps that from being a warning rather than hiding
// anything real.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use tabnas_proto::{FileDescriptorProto, ProtoError, ProtoOptions};
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

/// Parse one row's input through the crate's shared instance, reading the
/// row's `opts` column, as the Go and TypeScript fixture runners do.
pub fn parse_row(input: &str, row: &Row) -> Result<Value, Failure> {
    let raw = row.named("opts");
    let options = if raw.trim().is_empty() {
        None
    } else {
        Some(
            serde_json::from_str::<ProtoOptions>(raw).unwrap_or_else(|error| {
                panic!("{}: bad opts cell {raw:?}: {error}", row.location())
            }),
        )
    };
    tabnas_proto::parse(input, options.as_ref())
        .map(|file| to_value(&file))
        .map_err(to_failure)
}
