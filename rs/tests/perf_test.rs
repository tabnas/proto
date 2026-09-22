// What a realistic `.proto` costs, and what it must not cost.
//
// These are HANG DETECTORS with absolute ceilings, not benchmarks and not
// ratios. A ratio assertion over the element-count dimension would pin
// the engine's quadratic tree build as acceptable and then pass forever;
// `AGENTS.md` records that curve, its cause and where the repair lives,
// and deliberately leaves it unpinned.
//
// The ceilings are generous because this runs on the DEBUG profile, which
// is nowhere near what anyone ships, on a machine whose load is not this
// suite's to control.

mod common;

use std::time::Instant;

use tabnas_proto::{make, parse, to_descriptor};

/// Compiling the grammar dominates everything else, which is why `parse`
/// keeps one instance. Pinned so a change that started recompiling per
/// call shows up as the order-of-magnitude regression it would be.
#[test]
fn compiling_the_grammar_happens_once() {
    let start = Instant::now();
    let parser = make();
    let compile = start.elapsed().as_secs_f64();
    assert!(compile < 60.0, "compiling the grammar took {compile:.1}s");

    let source = "syntax = \"proto3\";\nmessage M { int32 a = 1; }";
    let start = Instant::now();
    for _ in 0..50 {
        let cst = parser.parse(source).expect("parse");
        to_descriptor(&cst, None).expect("descriptor");
    }
    let fifty = start.elapsed().as_secs_f64();
    assert!(
        fifty < compile.max(1.0) * 10.0,
        "50 parses took {fifty:.2}s against a {compile:.2}s compile, which is not the \
         one-off cost this instance reuse exists for"
    );
}

/// A file shaped like a real one: a package, imports, several messages
/// with a realistic number of fields each, an enum, a map, a oneof and a
/// service. This is the case that has to be fast.
#[test]
fn a_realistic_file_parses_quickly() {
    let mut source = String::from(
        "syntax = \"proto3\";\npackage demo.v1;\n\
         import \"google/protobuf/timestamp.proto\";\n\
         import public \"demo/v1/common.proto\";\n\
         option java_package = \"com.example.demo.v1\";\n",
    );
    for message in 0..20 {
        source.push_str(&format!("message M{message} {{\n"));
        for field in 1..=20 {
            source.push_str(&format!("  optional string f{field} = {field};\n"));
        }
        source.push_str("  map<string, int32> counts = 50;\n");
        source.push_str("  oneof choice { string a = 60; int32 b = 61; }\n");
        source.push_str("  enum Kind { UNKNOWN = 0; ONE = 1; }\n");
        source.push_str("  reserved 90 to 99;\n");
        source.push_str("}\n");
    }
    // NOT `rpc Stream`: the shared lexer matches a word keyword without
    // regard to case, so an rpc named `Stream` collides with the `stream`
    // modifier and is refused. That is the canonical behaviour too,
    // measured against ts/src on 2026-09-21, and `proto_test.rs` pins it.
    source.push_str(
        "service Demo {\n  rpc Get (M0) returns (M1);\n  \
         rpc Watch (stream M0) returns (stream M1);\n}\n",
    );

    // Warm the shared instance: the grammar compile is measured above.
    let _ = parse("syntax = \"proto3\";", None);
    let start = Instant::now();
    let file = parse(&source, None).expect("a realistic file parses");
    let elapsed = start.elapsed().as_secs_f64();

    assert_eq!(file.message_type.len(), 20);
    assert_eq!(file.service[0].method.len(), 2);
    // Twenty declared fields, the map field and the two oneof members.
    assert_eq!(file.message_type[0].field.len(), 23);
    assert!(
        elapsed < 30.0,
        "a 20-message file took {elapsed:.1}s on the debug profile"
    );
}

/// The whole shared fixture corpus through one instance, as a smoke test
/// for the shared-instance path under repetition.
#[test]
fn every_fixture_input_parses_through_one_instance() {
    let dir = common::spec_dir();
    let parser = make();
    let mut rows = 0;
    let start = Instant::now();
    for entry in std::fs::read_dir(&dir).expect("test/spec is readable") {
        let path = entry.expect("a directory entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("tsv") {
            continue;
        }
        let spec = tabnas_support::load_spec(&path, &tabnas_support::SpecOptions::default())
            .unwrap_or_else(|error| panic!("{}: {}", path.display(), error.0));
        for row in &spec.rows {
            // Some rows are expected to be refused; what matters here is
            // that none of them hangs or aborts.
            let _ = parser.parse(&row.unesc_named("input"));
            rows += 1;
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    // Ratcheted at what is on disk, so a corpus that shrinks cannot pass
    // by measuring less.
    assert_eq!(rows, 122, "the shared fixtures hold 122 rows");
    assert!(elapsed < 60.0, "{rows} fixture rows took {elapsed:.1}s");
}
