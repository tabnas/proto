// The shared conformance fixtures, every one of them.
//
// `test/spec/*.tsv` at the repository root is the parity contract: the
// TypeScript suite (`ts/test/parity.test.ts`), the Go suite
// (`go/parity_test.go` `TestSpec`) and this file run the same rows. A row
// green in one runtime and red in another is a failure, not a
// discrepancy.
//
// Discovery is by LISTING the directory, in all three runtimes, so adding
// a `.tsv` runs it everywhere without touching a runner. That is also why
// there is no exemption list here and no tripwire test beside it: nothing
// can be left unrun.
//
// What is specific to proto is the row's `opts` column and what an
// `ERROR:` cell means. This package declares no error codes of its own
// (see `AGENTS.md`, "Error codes"), so the one `ERROR:` row in the
// fixtures pins a fragment of the rendered MESSAGE instead: the rejection
// it names is the plugin's own version check rather than a parse failure
// the engine gives a code to.

mod common;

use tabnas_support::Runner;

use common::{parse_row, spec_dir};

fn runner() -> Runner {
    Runner::new_with_row(parse_row).match_error(|failure, want, _row| {
        // Both other runtimes compare with a substring of the message.
        failure.message.contains(want)
    })
}

#[test]
fn spec() {
    runner().dir(spec_dir());
}

// A runner that found nothing to run reports green while measuring
// nothing, so the count is asserted rather than assumed. Ratcheted at
// what is on disk today: adding a fixture is a deliberate act and moving
// this number with it is part of that act.
#[test]
fn every_fixture_file_is_run() {
    let files: Vec<String> = std::fs::read_dir(spec_dir())
        .expect("test/spec is readable")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tsv"))
        .collect();
    assert_eq!(
        files.len(),
        10,
        "test/spec holds {} fixtures, not the 10 this suite was measured against: {files:?}",
        files.len()
    );
}
