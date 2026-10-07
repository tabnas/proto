# Contributing to proto

Thanks for your interest in contributing! The organization-wide conventions
in [tabnas/.github](https://github.com/tabnas/.github/blob/main/CONTRIBUTING.md) are
canonical and apply here. This file adds what is specific to
**tabnas/proto**.

Start with [`AGENTS.md`](AGENTS.md) — it is the working guide to this
repository for humans and agents alike.

## Build & test

This repository is *polyglot*: `ts/`, `go/` and `rs/` hold three parallel
implementations of the same package. **`ts/` is canonical; `go/` and
`rs/` track it** — a behaviour change normally lands in all three, with
tests in all three.

```bash
make build   # builds ts/, go/ and rs/
make test    # tests ts/, go/ and rs/

# or per stack:
cd ts && npm install && npm run build && npm test
cd go && go build ./... && go test ./...
cd rs && cargo test --all-targets && cargo test --doc
```

The TypeScript and Go sides install published packages, `@tabnas/*` from
the npm registry and `github.com/tabnas/*/go` from the module proxy, so they
need no other checkout. Sibling checkouts are optional there: to work
against unreleased siblings, clone them into the same parent directory and
run admin's `scripts/link.sh`, which links them over
`ts/node_modules/@tabnas/*` and writes a `go.work` one level up. Never commit
that wiring. CI builds the siblings named in `.github/workflows/ci.yml`'s
`deps` from source. `rs/` is the exception: `rs/Cargo.toml` takes the engine
as a path dependency, and the ABNF compiler and the shared fixture runner as
path dev-dependencies, so it needs `parser`, `abnf`, `bnf` (which `abnf`
takes by path) and `support` checked out beside this repository even for a
plain build. The crates are on crates.io, but the committed manifest stays
path-only: only when it publishes `tabnas-proto` does the release workflow
swap the engine's path for a crates.io version and drop the two test-only
ones.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/) are required,
for commit messages and PR titles alike. PRs are squash-merged, so a PR's
title is its commit message, and the GitHub Release that each release creates
lists those titles in its generated notes. They do not set the version: a
release is its own version-bump pull request, then a `release.yml` dispatch
(see [`AGENTS.md`](AGENTS.md), "Releasing"). For example:

```
feat: add lax mode for trailing commas
fix: handle CRLF inside block scalars
docs: clarify plugin ordering
```

Use `feat!:` / `fix!:` (or a `BREAKING CHANGE:` footer) for breaking changes.

## Pull requests

1. Open an issue first for anything larger than a small fix.
2. Branch from `main`; keep the PR focused on one change.
3. `make test` must pass for **every** implementation.
4. PR titles follow Conventional Commits — PRs are squash-merged, so the
   title becomes the commit message.
5. CI must be green before merge.

## Security issues

Never open a public issue for a vulnerability — see [SECURITY.md](SECURITY.md).

## Code of conduct

Participation is covered by the org
[Code of Conduct](https://github.com/tabnas/.github/blob/main/CODE_OF_CONDUCT.md).
