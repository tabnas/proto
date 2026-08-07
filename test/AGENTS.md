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

## Rules

- Prefer adding a fixture here over a one-off in-language assertion when a
  case is expressible as source → descriptor. That is what keeps the two
  runtimes honest against each other.
- TypeScript is canonical. If the two runtimes disagree, the TS behaviour is
  the expected value — unless Go has exposed a genuine TS defect, in which
  case fix TS first and pin the corrected behaviour here.
- A new fixture must pass in BOTH runtimes: run `go test ./...` (from `go/`)
  and `npm test` (from `ts/`) before considering it done.

  **Exception, deliberate and documented:** `int-literals.tsv` is KNOWN RED in
  both runtimes. Its `expected` values are protoc 35.1's own answers (obtained
  by running `protoc --descriptor_set_out`), not either runtime's current
  output. It was added in the 2026-08 conformance baseline, whose job is to
  measure, not to fix. Do not edit those values to match the implementations.

## Third-party conformance corpus — `protobuf-suite/` (gitignored)

`test/protobuf-suite/` holds protobuf's OWN parser test corpus, extracted from
`compiler/parser_unittest.cc` at a pinned commit, plus recorded `protoc`
verdicts for the leniency probes. It is **never committed** — see
`scripts/fetch-protobuf-corpus.sh` and the baseline table in `../AGENTS.md`.

`leniency-probes.json` (committed) holds OUR probe inputs. The verdicts are
protoc's, recorded by running it, never hand-written.

Both runners (`ts/test/protobuf.test.ts`, `go/protobuf_test.go`,
`ts/test/leniency.test.ts`, `go/leniency_test.go`) **FAIL LOUDLY** when the
corpus is absent. They must never skip: a conformance test that quietly does
not run is worse than no test, because the green tick is a lie.

Note for anyone touching the TS runners: never throw out of a `describe()`
body. node's test runner prints a red suite for that and still counts ZERO
failed tests, so the process **exits 0**. Turn the failure into a leaf `it()`.
