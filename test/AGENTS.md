# Agents Guide — shared spec fixtures

`spec/*.tsv` holds the cross-runtime conformance fixtures. All three
runtimes auto-discover and run **every** file in this directory, so a
change here affects TypeScript, Go and Rust together — edit with that in
mind.

## Format

Tab-separated, one case per line, with a header row naming the columns.
Blank lines are skipped, and so are comment lines — a line starting with
`#` that contains no tab. (A data row always has at least one tab, so a
`#`-leading source such as a C preprocessor directive still works.)

| Column | Meaning |
|---|---|
| `input` | `.proto` source. Escapes `\n` `\r` `\t` `\\` are decoded. |
| `expected` | The resulting FileDescriptorProto as JSON, or `ERROR` / `ERROR:<substring>` for input that must be rejected. Unlike most of the fleet the text after the colon is a fragment of the MESSAGE, not an error code: the one such row names the plugin's own version check, which is not a parse failure the engine gives a code to. |
| `opts` | Optional JSON `ProtoOptions` — `{"version":"proto3"}`, `{"reconcile":false}` (empty means auto-detect). |

`expected` and `opts` are **not** escape-decoded — they are raw JSON, so
JSON's own escape rules apply. To put a literal backslash in `input`,
write `\\`.

Results are compared after a JSON round-trip, so absent fields and field
order do not affect the comparison.

## Who runs what

- TypeScript: `ts/test/parity.test.ts` — `makeRunner(...).dir(...)`.
- Go: `go/parity_test.go` — `support.Runner{...}.Dir(t, dir)`.
- Rust: `rs/tests/parity_test.rs` — `Runner::new_with_row(...).dir(...)`.

All three compare a result after a JSON round trip, which is what the
table above means by "absent fields and field order do not affect the
comparison": Go passes `jsonFlatten`, Rust `to_value`, TypeScript a
`normalize` that stringifies and reparses. A runner missing that step
compares a live object against a cell, so a `NaN` field number never
equals the `null` the cell records.

All three are a dozen lines holding only what is specific to proto: how
to build the parser for a row's options. Everything else — finding
`test/spec`, reading the file, decoding escapes, the `ERROR:` contract,
the comparison, the `<file>:<line>` in a failure message — comes from
[`@tabnas/support`](https://github.com/tabnas/support) and its Go and Rust
counterparts, so the three loaders cannot drift from one another either.

All three discover files by directory listing: adding a `.tsv` here runs
it in every runtime without touching a runner. An empty fixture, and a
spec directory with no fixtures in it, both **fail** — a runner that
reports green having run nothing is indistinguishable from coverage that
was never there.

## `divergent.tsv` is NOT in `spec/`, deliberately

[`../test/divergent.tsv`](divergent.tsv) is the divergence register: one
row per input where the three ports DISAGREE, with a cell per runtime.
It sits beside `spec/` rather than in it precisely because the runners
above run every file in `spec/` by listing, and every row of the register
is expected to disagree with one of them.

A fixture fails when behaviour REGRESSES. The register fails BOTH ways:
when a port is repaired to agree with the others, the row still claims
they differ, so the suite goes red and names the row to delete. All three
columns are executed: `ts/test/divergent.test.ts` reads the `ts` column,
`go/divergent_test.go` the `go` column and `rs/tests/divergent_test.rs`
the `rust` column, each through its runtime's half of
`tabnas_support::Register`. The prose lives in
[`../DIVERGENCE.md`](../DIVERGENCE.md).

## The files

`adjacent-strings` / `aggregate` / `edition-2023` / `edition-2024` /
`proto2` / `proto3` / `version-detect` / `whitespace` are the hand-written
per-topic fixtures. `aggregate.tsv` pins how an aggregate option value's
text is recorded and which text format forms the grammar takes between
the braces; `adjacent-strings.tsv` pins a string written as adjacent
literals, wherever protoc reads one. The rows of both were checked
against protoc 36.2's own parser, and a new row should be too
(`protobuf-suite/tools/oracle-check.py --spec`). The one exception is the
`aggregate.tsv` block that says so: text protoc's parser records and
text format refuses, which this package refuses as well.
`descriptor-shape.tsv` is a curated, commented tour of the descriptor
details protoc pins down (range bounds, groups, pseudo-options, synthetic
oneofs, visibility, …).

`protobuf-suite.tsv` is **generated**: the in-scope `valid` lane of the
vendored protoc parser corpus (`../protobuf-suite/valid.json`), one row per
case, so every runtime runs the whole corpus as a fixture too. Its
`expected` column is this parser's output — that this output equals
protoc's own golden is asserted separately, against the goldens, by each
runtime's conformance runner. Together the two say: TypeScript matches
protoc, and Go and Rust match TypeScript. Regenerate it rather than
hand-editing, and only after the conformance tests are green: run
`node test/protobuf-suite/tools/suite-tsv.js` from the repository root,
after `npm run build` in `ts/`.

## What the corpus does not reach

The fixtures and the vendored protoc corpus together were green on every
row while five descriptor details were wrong, so the count of rows is
not a measure of coverage. What they missed, and what the rows added in
2026-09 cover, is one shape: a NAME that collides with the grammar's own
text.

- A declared name that occurs inside its own leading keyword (`message
  m`, `oneof o`, `enum n`, `package e`). The whole declaration was
  dropped, with no error.
- An enum value name ending in its own number (`A1 = 1`), or beginning
  with a statement keyword (`optionX = 1`).
- A type name beginning with a modifier (`streaming.Request` after
  `rpc`), or spelt like a scalar behind a leading dot (`.int32`).
- A declaration kind whose options nobody looked for (`oneof`).

Every `.proto` protoc's own corpus writes uses names chosen to read
well, so none of these appear in it. When adding a fixture, prefer a
name that collides with the syntax around it over one that reads
naturally: the second kind is already covered many times over.

## Rules

- Prefer adding a fixture here over a one-off in-language assertion when a
  case is expressible as source → descriptor. That is what keeps the three
  runtimes honest against each other.
- TypeScript is canonical. If the runtimes disagree, the TS behaviour is
  the expected value — unless a port has exposed a genuine TS defect, in
  which case fix TS first and pin the corrected behaviour here.
- A new fixture must pass in ALL THREE runtimes: run `npm test` (from
  `ts/`), `go test ./...` (from `go/`) and `cargo test --all-targets`
  (from `rs/`) before considering it done.

## Harness rules (all runtimes)

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
