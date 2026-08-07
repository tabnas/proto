#!/usr/bin/env bash
#
# Fetch the third-party .proto conformance corpus.
#
# Upstream : https://github.com/protocolbuffers/protobuf
# Tag      : v35.1
# Commit   : 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03   (PINNED - not a branch)
# File     : src/google/protobuf/compiler/parser_unittest.cc
#
# The corpus is NEVER committed to this repo (project rule: no vendored
# third-party test corpora - licensing and repo hygiene). It is downloaded into
# test/protobuf-suite/, which is gitignored. Only this script, the two
# extraction tools next to it, and the pinned SHA are committed.
#
# Idempotent: re-running is safe and cheap. Pass --force to re-download.
#
# Why parser_unittest.cc and not something else
# ---------------------------------------------
# `.proto` has no "official conformance suite" in the JSONTestSuite/CommonMark
# sense. protobuf's own `conformance/` directory tests WIRE and JSON encoding of
# already-compiled messages - it does not exercise the .proto IDL parser at all,
# and `src/google/protobuf/testdata/` is likewise golden encoded messages.
#
# `compiler/parser_unittest.cc` IS the protobuf project's parser test corpus: it
# is the file that gates every change to protoc's own .proto parser. It is also
# at the right abstraction level for @tabnas/proto, because its goldens are the
# descriptor the PARSER produces, before DescriptorPool cross-file type
# resolution - which is exactly what this package claims to produce.
#
# Running `protoc --descriptor_set_out` over protobuf's own .proto files was
# considered and rejected: those goldens are POST resolution (fully-qualified
# type names, resolved options), so they would measure a claim this package does
# not make.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$REPO_ROOT/test/protobuf-suite"

UPSTREAM_URL="https://github.com/protocolbuffers/protobuf"
UPSTREAM_TAG="v35.1"
UPSTREAM_SHA="35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03"
UPSTREAM_PATH="src/google/protobuf/compiler/parser_unittest.cc"
# sha256 of the file at the pinned commit; a mismatch means the pin moved.
UPSTREAM_SHA256="d2a365cfbdef97cb29cdde6156aaef1cabda378a77859407f00607dc815ca335"

FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1

mkdir -p "$OUT_DIR"
SRC="$OUT_DIR/parser_unittest.cc"

# ---- 1. download (pinned commit, checksum-verified) --------------------------

download() {
  local url="https://raw.githubusercontent.com/protocolbuffers/protobuf/$UPSTREAM_SHA/$UPSTREAM_PATH"
  echo "fetch: $url"
  curl -fsSL --retry 3 -o "$SRC.tmp" "$url"
  mv "$SRC.tmp" "$SRC"
}

verify() {
  local got
  got="$(sha256sum "$SRC" | cut -d' ' -f1)"
  if [ "$got" != "$UPSTREAM_SHA256" ]; then
    echo "FATAL: $UPSTREAM_PATH checksum mismatch at pinned commit $UPSTREAM_SHA" >&2
    echo "  want $UPSTREAM_SHA256" >&2
    echo "  got  $got" >&2
    echo "The pin must be updated deliberately, never silently." >&2
    exit 1
  fi
}

if [ "$FORCE" = 1 ] || [ ! -f "$SRC" ]; then
  download
fi
verify

# ---- 2. extract the corpus out of the C++ test source -----------------------

python3 "$REPO_ROOT/scripts/corpus/extract-parser-corpus.py" \
  "$SRC" "$OUT_DIR/raw.json"

# ---- 3. transcode the descriptor goldens: protobuf text format -> protojson --
#
# Done with the CANONICAL Go protobuf runtime, so the goldens are upstream's own
# values mechanically transcoded, never hand-written by us. The tool is built in
# a throwaway module inside the gitignored output dir, with GOWORK=off, so it
# cannot perturb this repo's go.mod / the shared go.work.

# The build dir lives OUTSIDE the repo on purpose: admin/scripts/link.sh
# regenerates the shared go.work with `find <tabnas-root> -name go.mod`, so a
# throwaway go.mod anywhere under this repo would be added to the workspace and
# break every sibling. Keep it in $TMPDIR.
BUILD="$(mktemp -d "${TMPDIR:-/tmp}/tabnas-proto-corpus.XXXXXX")"
trap 'rm -rf "$BUILD"' EXIT
cp "$REPO_ROOT/scripts/corpus/convert-goldens.go" "$BUILD/main.go"
cat >"$BUILD/go.mod" <<'EOF'
module tabnas.local/protocorpus

go 1.24

require google.golang.org/protobuf v1.36.11
EOF

(
  cd "$BUILD"
  GOWORK=off GOFLAGS=-mod=mod go mod tidy >/dev/null 2>&1
  GOWORK=off go run . "$OUT_DIR/raw.json" "$OUT_DIR"
)

# ---- 4. record real protoc verdicts for the leniency probes ------------------
#
# test/leniency-probes.json holds OUR probe inputs (committed - they are ours,
# a dozen lines). The VERDICTS are not ours: protoc v35.1 itself is downloaded
# here, run over each probe, and its accept/reject answer recorded into
# test/protobuf-suite/leniency.json (gitignored). So the leniency test asserts
# third-party ground truth, not our opinion of what .proto ought to allow.

PROTOC_VER="35.1"
PROTOC_ZIP_SHA256="6930ebf62bd4ea607b98fff052596c6ee564b9835b4ce172c75a3f53ae9d91b7"
PROTOC_ZIP="$OUT_DIR/protoc-$PROTOC_VER-linux-x86_64.zip"
PROTOC_DIR="$OUT_DIR/protoc"
PROTOC="$PROTOC_DIR/bin/protoc"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) PROTOC_SUPPORTED=1 ;;
  *)            PROTOC_SUPPORTED=0 ;;
esac

if [ "$PROTOC_SUPPORTED" = 1 ]; then
  if [ "$FORCE" = 1 ] || [ ! -x "$PROTOC" ]; then
    curl -fsSL --retry 3 -o "$PROTOC_ZIP" \
      "$UPSTREAM_URL/releases/download/v$PROTOC_VER/protoc-$PROTOC_VER-linux-x86_64.zip"
    got="$(sha256sum "$PROTOC_ZIP" | cut -d' ' -f1)"
    if [ "$got" != "$PROTOC_ZIP_SHA256" ]; then
      echo "FATAL: protoc-$PROTOC_VER zip checksum mismatch" >&2
      echo "  want $PROTOC_ZIP_SHA256" >&2
      echo "  got  $got" >&2
      exit 1
    fi
    rm -rf "$PROTOC_DIR"
    mkdir -p "$PROTOC_DIR"
    unzip -oq "$PROTOC_ZIP" -d "$PROTOC_DIR"
  fi
  python3 "$REPO_ROOT/scripts/corpus/record-protoc-verdicts.py" \
    "$PROTOC" "$REPO_ROOT/test/leniency-probes.json" "$OUT_DIR/leniency.json"
else
  echo "FATAL: no pinned protoc build for $(uname -s)-$(uname -m)." >&2
  echo "The leniency lane records REAL protoc verdicts and must not be faked" >&2
  echo "or skipped. Add the platform's pinned release + checksum above." >&2
  exit 1
fi

echo
echo "corpus ready in $OUT_DIR (gitignored)"
echo "upstream: $UPSTREAM_URL $UPSTREAM_TAG @ $UPSTREAM_SHA"
