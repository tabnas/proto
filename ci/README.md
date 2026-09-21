# ci/

Staging area for GitHub Actions workflow changes.

This directory exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file in `workflows/`.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

## Pending

- **`workflows/docs.yml`** — the prose gate: Vale over the reader-facing
  pages at the levels set in `.vale.ini`, on the file list
  `ts/scripts/gated-docs.cjs` produces. See `docs/STYLE-GUIDE.md`.

  It needs no sibling checkouts and no secrets, and pins its own Vale
  version. Errors fail the job; warnings go to the run summary as a
  report. `make prose` runs the identical check locally, and the test
  suite already runs the other half of the gate
  (`ts/test/docs.test.js`), so promoting this adds the spelling and
  Google-convention arm rather than the whole gate.

  Its path lists now include `rs/README.md`, which joined the gated set
  with the Rust port. A gated page missing from those lists is a gate
  that never runs on the page it covers.

- **`workflows/rust.yml`** — the Rust port gate: formatting, build,
  tests, doctests, clippy with `-D warnings`, and a lockfile check.
  Everything it does lives in [`rust/run.sh`](rust/run.sh), so the hosted
  run and a local one cannot say different things; `ci/rust/run.sh` IS
  that local run, and `make test-rs` is the fast inner loop.

  Unlike `docs.yml` it needs FOUR sibling checkouts — `tabnas/parser`,
  `tabnas/abnf`, `tabnas/bnf` and `tabnas/support` — because
  `rs/Cargo.toml` takes each as a path dependency and none of them is
  published to a registry. The workflow clones them; the script checks
  they are there first and says which is missing. It needs no secrets.

  It checks this repository out into a NAMED directory, so the siblings
  have somewhere to go beside it. A bare checkout at the workspace root
  leaves `../../parser/rs` pointing outside the workspace.
