# Agents Guide — protoc parser conformance corpus

`.proto` has no CommonMark-style example suite. The nearest thing to an
authoritative oracle for an IDL parser is **protoc's own parser unit test**,
which pairs `.proto` source with the exact `FileDescriptorProto` protoc's
parser produces. This directory is that test, extracted into JSON so it can
be run from TypeScript.

## Provenance

Extracted from upstream protobuf
`src/google/protobuf/compiler/parser_unittest.cc` at **v36.2** (commit
`2c74169b`), cross-checked against the `protoc 36.2` binary and against the
parser built from that tag. Each case keeps its upstream test name
(`ParseMessageTest.SimpleMessage`, …) so it can be traced back.

The corpus JSON is vendored — the suite runs offline, in CI, with no
fetch step. The protoc binary/zip and the original `parser_unittest.cc`
are **not** committed (they are large and are upstream's to distribute);
`.gitignore` keeps them out. Nothing in the test run needs them.

## Refreshing it

The corpus tracks upstream's latest protobuf release. `tools/` holds the
extractor that writes it and the checks that hold it to protoc, and
[`tools/README.md`](tools/README.md) is the procedure: extract with
`tools/extract.py` and `tools/lanes.py` in a throwaway virtual environment
holding the PyPI `protobuf` of the same release, cross-check with the
protoc binary (`tools/crosscheck.py`, `tools/probes.py`) and with protoc's
own parser (`tools/oracle/`, `tools/oracle-check.py`), then move every
count that records the corpus and regenerate `test/spec/protobuf-suite.tsv`
with `tools/suite-tsv.js`. Run on the same inputs, the scripts reproduce
the v35.1 and the v36.2 corpora byte for byte.

## Lanes

| File | Upstream helper | Meaning |
|---|---|---|
| `valid.json` | `ExpectParsesTo` | source + the descriptor protoc's parser produces. The real conformance bar. |
| `accept-only.json` | `ExpectHasWarnings`, … | source protoc's parser accepts; it asserts no error but publishes no descriptor golden. Must parse without throwing. |
| `invalid.json` | `ExpectHasErrors` | source protoc **rejects**, with the expected diagnostic. |
| `excluded.json` | — | upstream cases that cannot be extracted: their input or golden is computed in C++ at run time rather than written as a string literal. Each row carries a reason. |
| `leniency.json` | — | hand-written probes for lexer edges where the shared tabnas lexer may be more permissive than `.proto`. Records protoc's answer (`accepted`) and ours (`tabnas`). |
| `raw.json` | — | the unfiltered extraction the lanes were split from; kept so the split is auditable. |

## Who runs what

`ts/test/protobuf-conformance.test.ts` runs `valid`, `accept-only` and
`leniency`. It never skips: the corpus is in-repo, so an absent file is a
failure, not a silent pass.

`go/protobuf_conformance_test.go` and `rs/tests/protobuf_conformance_test.rs`
run the same three lanes with the same contracts and the same
normalisation, so Go and Rust are measured against protoc's goldens
directly rather than transitively. (`test/spec/protobuf-suite.tsv` also
feeds the `valid` lane to both, but its `expected` column is *this
parser's* output — it proves a port matches TypeScript, not that it
matches protoc, and it covers neither `accept-only` nor `leniency`.)
Neither skips either.

`invalid` is deliberately **not** a pass/fail gate. `proto-grammar/common.abnf`
is a permissive union across proto2/proto3/editions and per-version legality
is the walk's / protoc's concern, not recognition's — so roughly half of that
lane parses here. Rejection is not part of this package's contract; see the
root `AGENTS.md`.

## Rules

- The first two declared output-shape deviations (options as a plain map;
  `defaultValue` kept as written) are **bridged** in the runner, not waived:
  protoc's encoding is translated into ours before comparing, so an option
  name or value we failed to capture still fails. The third, a string
  written as one literal keeping its escapes, has no bridge and needs none
  today: no `valid.json` case holds a string literal with an escape.
- The only excluded `valid` cases are those declaring protoc-internal
  editions (`UNSTABLE`, `NNNNN_TEST_ONLY`). The runner asserts the exclusion
  set is exactly those and no larger — do not widen it to hide a failure.
- If a case fails, fix the parser. Changing a golden here means claiming
  protoc is wrong, which it almost never is; say so explicitly if you do.
