// The divergence register: where this repo's ports DISAGREE, executed.
//
// `test/divergent.tsv` holds one row per disagreement, with a cell per
// runtime. This file reads the `rust` column through
// `tabnas_support::Register`, which checks each row three ways: that it
// records a disagreement at all, that this runtime still produces what it
// claims, and, when it does not, whether it has CONVERGED on another
// runtime's answer, which means the divergence is closed and the row must
// go.
//
// WHY THIS IS NOT A FIXTURE. A fixture fails when behaviour REGRESSES.
// This fails both ways, so a row cannot outlive the divergence it
// records. The register also sits OUTSIDE `test/spec/`, because both
// other runtimes run every file in that directory by listing and every
// row here is expected to disagree with one of them.
//
// `DIVERGENCE.md` at the repository root carries the prose and the
// measured tables; this file is the half that runs.

mod common;

use tabnas_support::{Register, Runner};

use common::{parse_row, repo_dir};

fn runner() -> Runner {
    Runner::new_with_row(parse_row)
        .input("input")
        .match_error(|failure, want, _row| failure.message.contains(want))
}

#[test]
fn divergence_register() {
    Register::new(runner(), "rust", &["ts", "go", "rust"])
        .file(repo_dir().join("test").join("divergent.tsv"));
}

// An EMPTY register is legitimate, a repo with no divergences, but an
// empty FILE is not: it cannot be told apart from a loader that read
// nothing. The count is ratcheted at what was measured, so a row that
// quietly disappears fails here rather than reducing the coverage in
// silence.
#[test]
fn the_register_has_the_rows_it_is_measured_against() {
    let path = repo_dir().join("test").join("divergent.tsv");
    let spec = tabnas_support::load_spec(&path, &tabnas_support::SpecOptions::default())
        .unwrap_or_else(|error| panic!("{}: {}", path.display(), error.0));
    assert_eq!(
        spec.rows.len(),
        11,
        "{} holds {} rows, not the 11 recorded in DIVERGENCE.md",
        path.display(),
        spec.rows.len()
    );
}
