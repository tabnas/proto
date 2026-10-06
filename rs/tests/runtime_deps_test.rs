// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// The crate installs a grammar compiled at build time
// (`proto-grammar.json`, embedded), so the ABNF compiler is not among
// the crates a consumer builds: neither `tabnas-abnf` nor the
// notation-neutral `tabnas-bnf` under it may appear in a table cargo
// builds into the library. `cargo tree -e normal` from rs/ is the
// command-line form of the same check; this reads the manifest, so it
// runs offline and before anything is resolved.

use std::fs;
use std::path::Path;

const COMPILER: [&str; 2] = ["tabnas-abnf", "tabnas-bnf"];

/// Is this table header one whose entries are built into the library?
/// `[dependencies]`, `[build-dependencies]`, their `[target.*]` forms,
/// and the dotted single-dependency spelling of any of them
/// (`[dependencies.tabnas-abnf]`). `dev-dependencies` is a different
/// segment, so it never matches.
fn builds_into_the_library(header: &str) -> bool {
    header
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split('.')
        .any(|segment| matches!(segment.trim(), "dependencies" | "build-dependencies"))
}

#[test]
fn the_compiler_is_not_a_runtime_dependency() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = fs::read_to_string(&path).expect("rs/Cargo.toml is readable");

    let mut header = String::new();
    let mut tables = 0;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            header = line.to_string();
            if builds_into_the_library(&header) {
                tables += 1;
                for name in COMPILER {
                    assert!(
                        !header.contains(name),
                        "rs/Cargo.toml has a {header} table for {name}, the ABNF \
                         compiler, which only the build needs"
                    );
                }
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') || !builds_into_the_library(&header) {
            continue;
        }
        let key = line.split('=').next().unwrap_or("").trim();
        for name in COMPILER {
            assert!(
                key != name && !line.contains(&format!("package = \"{name}\"")),
                "rs/Cargo.toml lists {name}, the ABNF compiler, under {header}: \
                 the grammar is compiled at build time, so it belongs in \
                 [dev-dependencies] at most"
            );
        }
    }
    // The scan is only evidence if it saw the table it guards.
    assert!(tables > 0, "rs/Cargo.toml has no [dependencies] table");
}
