# ci/

The script the Rust gate runs, [`rust/run.sh`](rust/run.sh), and notes on
this repository's own CI workflows. `.github/workflows/rust.yml` calls
that script, and you can run it locally too.

The workflows themselves live in `.github/workflows/`. To change CI, edit
them there in a reviewed pull request: session credentials can push
workflow changes (admin `DECISIONS.md` ADR-8, as amended on 2026-09-24),
so staging a workflow here for a maintainer to promote is optional.

`ci.yml`, `crates-release.yml`, `release.yml`, `notify-status.yml` and
`scorecard.yml` also have a template in admin
`rollout/workflows/proto__<file>`. Mirror a change there in the same change:
admin `scripts/verify.sh` compares the two, and
`rollout/apply-workflows.sh --apply` pushes the template's text back.
`clib.yml` and `clib-release.yml` are stamped from admin
`tasks/clib-template/`, so change the template and restamp.

Sessions still cannot push tags. Releases therefore go through
`workflow_dispatch`, and a workflow that runs only on a tag push needs a
maintainer to push that tag.

## Promoted

The prose gate and the Rust port gate were staged here, were promoted on
2026-09-22, and run from `.github/workflows/`: `docs.yml` runs Vale and
the recorded-count check, and `rust.yml` runs [`rust/run.sh`](rust/run.sh)
(formatting, build, tests, doctests, Clippy with `-D warnings`, and a
lockfile check), the same script a local run uses. Each workflow's own
comments say why it is shaped as it is.
