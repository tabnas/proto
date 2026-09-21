// The Rust crate's version is one of the release sites that must agree.
// A bump that updates `ts/package.json` and forgets these fails here
// rather than shipping a crate whose version disagrees with the package
// it is a port of. Mirrors `ts/test/version.test.ts` and
// `go/version_test.go`.
//
// The package.json check is deliberately fatal and never skipped: a
// version check that silently does not run is the failure mode the whole
// family of tests exists to prevent.

mod common;

use std::fs;

use common::repo_dir;

#[test]
fn version_looks_like_a_version() {
    let parts: Vec<&str> = tabnas_proto::VERSION.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "VERSION is not x.y.z: {}",
        tabnas_proto::VERSION
    );
    for part in parts {
        assert!(
            !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()),
            "VERSION segment is not numeric: {}",
            tabnas_proto::VERSION
        );
    }
}

#[test]
fn version_matches_cargo_toml() {
    let manifest =
        fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the manifest is readable");
    let declared = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .expect("the manifest declares a version");
    assert_eq!(
        declared,
        tabnas_proto::VERSION,
        "Cargo.toml disagrees with VERSION"
    );
}

#[test]
fn version_matches_package_json() {
    let package = fs::read_to_string(repo_dir().join("ts").join("package.json"))
        .expect("ts/package.json is readable, so VERSION can be checked");
    let declared: serde_json::Value =
        serde_json::from_str(&package).expect("ts/package.json is JSON");
    assert_eq!(
        declared["version"]
            .as_str()
            .expect("package.json has a version"),
        tabnas_proto::VERSION,
        "ts/package.json disagrees with VERSION"
    );
}

// The Go port carries the same constant, and the release orchestrator
// rewrites all three. A checkout without the Go source is not a reason to
// skip: this repository always has one.
#[test]
fn version_matches_the_go_port() {
    let source = fs::read_to_string(repo_dir().join("go").join("proto.go"))
        .expect("go/proto.go is readable");
    let declared = source
        .lines()
        .find_map(|line| line.strip_prefix("const VERSION = \""))
        .and_then(|rest| rest.split('"').next())
        .expect("go/proto.go declares a VERSION");
    assert_eq!(
        declared,
        tabnas_proto::VERSION,
        "go/proto.go disagrees with VERSION"
    );
}

// And the canonical TypeScript, whose export is what the other two
// mirror.
#[test]
fn version_matches_the_typescript_source() {
    let source = fs::read_to_string(repo_dir().join("ts").join("src").join("proto.ts"))
        .expect("ts/src/proto.ts is readable");
    let declared = source
        .lines()
        .find_map(|line| line.strip_prefix("const VERSION = '"))
        .and_then(|rest| rest.split('\'').next())
        .expect("ts/src/proto.ts declares a VERSION");
    assert_eq!(
        declared,
        tabnas_proto::VERSION,
        "ts/src/proto.ts disagrees with VERSION"
    );
}
