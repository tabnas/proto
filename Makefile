# Build, test and publish the TypeScript (ts/), Go (go/) and Rust (rs/)
# implementations. ts/ is canonical; go/ and rs/ track it.
#
# Local build/test resolve the unpublished @tabnas siblings via the
# repo-set go.work + node_modules symlinks (admin/scripts/link.sh). The
# Rust crate resolves them as path dependencies on sibling checkouts; see
# rs/AGENTS.md.

.PHONY: all build test clean build-ts build-go build-rs test-ts test-go test-rs \
        clean-ts clean-go clean-rs publish-ts publish-go tags-go reset embed generate \
        prose prose-counts

all: build test

build: build-ts build-go build-rs

test: test-ts test-go test-rs

clean: clean-ts clean-go clean-rs

# --- TypeScript (package in ts/) ---
build-ts:
	cd ts && npm run build

test-ts:
	cd ts && npm test

clean-ts:
	rm -rf ts/dist ts/dist-test

embed:
	cd ts && npm run embed

# Publish the TypeScript package at its current package.json version.
publish-ts: test-ts
	cd ts && npm publish --access public

# --- Go (module in go/) ---
build-go:
	cd go && go build ./...

test-go:
	cd go && go test -v ./...

clean-go:
	cd go && go clean

# Regenerate go/grammar.go from proto-grammar/*.abnf (Go counterpart of embed).
generate:
	cd go && go generate ./...

# Publish the Go module: make publish-go V=x.y.z
# Injects V into the Go `VERSION` const, commits, tags go/vX.Y.Z, and
# (when gh is available) creates a GitHub release.
publish-go: test-go
	@test -n "$(V)" || (echo "Usage: make publish-go V=x.y.z" && exit 1)
	sed -i.bak 's/^const VERSION = ".*"/const VERSION = "$(V)"/' go/proto.go
	rm -f go/proto.go.bak
	git add go/proto.go
	git commit -m "go: v$(V)"
	git tag go/v$(V)
	git push origin main go/v$(V)
	@command -v gh >/dev/null 2>&1 && gh release create go/v$(V) --title "go/v$(V)" --notes "Go module release v$(V)" || true

# List published Go module tags, newest first.
tags-go:
	git tag -l 'go/v*' --sort=-version:refname

# --- Rust (crate in rs/) ---
#
# Nothing to fetch: the engine, the ABNF compiler and the shared fixture
# runner are path dependencies on sibling checkouts (parser, abnf, bnf,
# support). `ci/rust/run.sh` is the full gate; these are the inner loop.
build-rs:
	cd rs && cargo build --all-targets

# `--all-targets` does NOT include doctests, and rs/README.md is
# doctested, so both selections are run.
test-rs:
	cd rs && cargo test --all-targets
	cd rs && cargo test --doc
	cd rs && cargo clippy --all-targets --all-features -- -D warnings

clean-rs:
	cd rs && cargo clean

reset:
	cd ts && npm run reset
	cd go && go clean -cache && go build ./... && go test -v ./...

# The prose gate (see docs/STYLE-GUIDE.md). Vale over the reader-facing
# pages, at the levels set in .vale.ini, on the same file list
# ts/test/docs.test.js reads. Requires `vale` on PATH and one
# `vale sync`. Warnings are advisory, errors fail.
prose:
	vale --minAlertLevel=error $$(node ts/scripts/gated-docs.cjs)
	node ts/scripts/vale-counts.cjs

# Re-measure what .vale.ini and the style guide record, after
# a change to the pages or to the rules moves the numbers.
prose-counts:
	node ts/scripts/vale-counts.cjs --write
