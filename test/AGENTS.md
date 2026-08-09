# Agents Guide — shared spec fixtures

`spec/*.tsv` holds the cross-runtime conformance fixtures. Both runtimes
auto-discover and run **every** file in this directory, so a change here
affects TypeScript and Go together — edit with that in mind.

## Format

Tab-separated, one case per line, with a header row naming the columns.
Blank lines are skipped, and so are comment lines — a line starting with
`#` that contains no tab. (A data row always has at least one tab, so a
`#`-leading source such as a C preprocessor directive still works.)

| Column | Meaning |
|---|---|
| `input` | `.proto` source. Escapes `\n` `\r` `\t` `\\` are decoded. |
| `expected` | The resulting FileDescriptorProto as JSON, or `ERROR` / `ERROR:<substring>` for input that must be rejected. |
| `opts` | Optional JSON `ProtoOptions` — `{"version":"proto3"}`, `{"reconcile":false}` (empty means auto-detect). |

`expected` and `opts` are **not** escape-decoded — they are raw JSON, so
JSON's own escape rules apply. To put a literal backslash in `input`,
write `\\`.

Results are compared after a JSON round-trip, so absent fields and field
order do not affect the comparison.

## Who runs what

- TypeScript: `ts/test/parity.test.ts` — reads `../../test/spec` at runtime
  from `dist-test/`, one `describe` per file.
- Go: `go/parity_test.go` — `TestSpec` globs `../test/spec/*.tsv`.

Both discover files by directory listing: adding a `.tsv` here runs it in
both runtimes without touching either runner.

## The files

`edition-2023` / `edition-2024` / `proto2` / `proto3` / `version-detect` /
`whitespace` are the hand-written per-topic fixtures.
`descriptor-shape.tsv` is a curated, commented tour of the descriptor
details protoc pins down (range bounds, groups, pseudo-options, synthetic
oneofs, visibility, …).

`protobuf-suite.tsv` is **generated**: the in-scope `valid` lane of the
vendored protoc parser corpus (`../protobuf-suite/valid.json`), one row per
case, so Go runs the whole corpus too. Its `expected` column is this
parser's output — that this output equals protoc's own golden is asserted
separately, against the goldens, by
`ts/test/protobuf-conformance.test.ts`. Together the two say: TypeScript
matches protoc, and Go matches TypeScript. Regenerate it rather than
hand-editing, and only after the conformance test is green.

## Rules

- Prefer adding a fixture here over a one-off in-language assertion when a
  case is expressible as source → descriptor. That is what keeps the two
  runtimes honest against each other.
- TypeScript is canonical. If the two runtimes disagree, the TS behaviour is
  the expected value — unless Go has exposed a genuine TS defect, in which
  case fix TS first and pin the corrected behaviour here.
- A new fixture must pass in BOTH runtimes: run `go test ./...` (from `go/`)
  and `npm test` (from `ts/`) before considering it done.

## Harness rules (both runtimes)

These are the ways a suite can pass while measuring nothing. Each has bitten
this repo; do not reintroduce them.

- **Never throw out of a `describe()` body.** node's test runner prints a red
  suite for a describe-body throw, counts **zero** failed tests, and **exits
  0** — so a malformed or empty fixture goes green in CI. Put the guard in a
  leaf `it()`. (`ts/test/parity.test.ts` was this shape until 2026-08.)
- **No assertion that cannot fail.** `assert.ok(n >= 0)` is true for every
  possible `n`. If the point is "we found some", ratchet at the count you
  actually measured and say so in the message.
- **No silent skip on a missing corpus.** Everything the conformance runners
  read is committed under `protobuf-suite/`, so an absent file is a failure,
  not a reason to pass quietly.
