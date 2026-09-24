# ci/

Staging area for GitHub Actions workflow changes.

This directory exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file in `workflows/`.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

## Pending

Nothing. The prose gate and the Rust port gate were promoted on
2026-09-22 and run from `.github/workflows/`: `docs.yml` runs Vale and
the recorded-count check, and `rust.yml` runs [`rust/run.sh`](rust/run.sh)
(formatting, build, tests, doctests, Clippy with `-D warnings`, and a
lockfile check), the same script a local run uses. Each workflow's own
comments say why it is shaped as it is.
