# Agents Guide — proto

## What this project is

`@tabnas/proto` parses Protocol Buffers `.proto` IDL — **proto2, proto3,
and editions 2023/2024** — into [FileDescriptorProto][fdp]-shaped JSON. It
drives the [Tabnas](https://github.com/tabnas/parser) engine with an
[ABNF](https://github.com/tabnas/abnf) grammar rather than a hand-written
parser. TypeScript is the implementation; a Go port is a planned follow-up.

Pipeline: `proto-grammar/*.abnf` → (`@tabnas/abnf` compiles) → Tabnas
`GrammarSpec` → engine parses `.proto` to a `{rule, src, kids}` CST →
`src/build-descriptor.ts` walks the CST into a `FileDescriptorProto`.

## Layout

```
proto-grammar/
  common.abnf          # shared base (the union superset)
  proto2.abnf          # =/ deltas: group
  proto3.abnf          # (no structural additions)
  edition-2023.abnf    # =/ deltas: edition declaration
  edition-2024.abnf    # =/ deltas: import option, export/local visibility
ts/
  embed-grammar.js     # concatenates the 5 .abnf files -> src/grammar.ts
  src/grammar.ts       # GENERATED — do not edit
  src/proto.ts         # plugin + parse()/toDescriptor() entry points
  src/build-descriptor.ts  # CST -> FileDescriptorProto walk
  src/descriptor.ts    # output types + scalar-type table
  src/detect-version.ts    # syntax/edition detection + reconciliation
  test/                # node:test (version-detect, proto, doc-examples)
```

## Grammar conventions (important)

The grammar is **pure structure over the lexer's whole-word tokens**;
whitespace and `//` / `/* */` comments are ignored by the lexer, so the
grammar never mentions them. Lexical atoms are referenced by name:
`TX` (identifier), `NR` (number), `ST` (string), `VL` (true/false/null) —
features added to `@tabnas/abnf` for this project. Wrap a token in a named
rule (`ident = TX`) so it surfaces as a CST node for the walk.

The grammar is compiled with `{ tag: 'proto', start: 'proto',
wordKeywords: true }`. `wordKeywords` makes literal keywords match as whole
words (so `option` doesn't grab the `option` prefix of `optional`). It is
**required** — without it the grammar mis-tokenises.

`common.abnf` is a permissive **union** that accepts every version's
syntax. Per-version legality (proto3 has no `required`, `group` is
proto2-only, …) is the walk's / protoc's concern, not recognition's. After
editing any `.abnf` file run `npm run embed` (the build does this).

## The walk and abnf inlining (the main gotcha)

`@tabnas/abnf` inlines a sub-rule referenced at the very start of an
alternative (Paull's left-recursion elimination). So the specific statement
rule (`message`, `field`, `range`, …) is folded into its enclosing dispatch
node (`topLevelDef`, `messageElement`, `ranges`). The walk therefore:

- discriminates a statement by `kw(node)` — the keyword(s) in `src` before
  the node's first child (e.g. `message`, `map<`, `oneof`, `reserved`);
  `kw === ''` with a leading `fieldType`/`label` means a field;
- reads inlined values from `src` when a needed sub-rule was inlined (the
  leading field type, the first `reserved` range, option names) — safe
  because tokens are whole words, so `src` boundaries are unambiguous;
- unwraps the edition-2024 `export`/`local` visibility wrapper, where the
  `message`/`enumDef` stays a *child* node instead of inlining.

When you add a construct, dump the CST first (parse with the bare grammar
and print `{rule, src, kids}`) to see how it inlined, then map it.

## Conformance — the TRUE 2026-08 baseline

### The claim being judged

`README.md` / `ts/README.md` make an **unqualified** claim: parse `.proto`
IDL — proto2, proto3, editions 2023/2024 — into FileDescriptorProto-shaped
JSON, `map<K,V>` expanded "exactly as `protoc` does". The single declared
limitation is "type names are stored as written; cross-file resolution is a
separate concern". So the package is judged against protoc's **parser**
output, pre-`DescriptorPool` resolution — which is exactly what the corpus
below measures.

### The corpus (fetched, never committed)

`scripts/fetch-protobuf-corpus.sh` downloads, at a **pinned commit**:

| | |
|---|---|
| Upstream | https://github.com/protocolbuffers/protobuf |
| Tag / commit | `v35.1` / `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03` |
| Source | `src/google/protobuf/compiler/parser_unittest.cc` (sha256 pinned) |
| Also | the `protoc` v35.1 linux-x86_64 release (sha256 pinned) |

It lands in `test/protobuf-suite/`, which is **gitignored**. The corpus is
third-party and is never vendored (project rule). Only the fetch script,
`scripts/corpus/*` (our two extraction tools) and the pinned SHAs are
committed.

`parser_unittest.cc` was chosen over protobuf's `conformance/` directory
(which tests wire/JSON encoding of compiled messages, not the IDL) and over
`protoc --descriptor_set_out` goldens (post-resolution, so they would
measure a claim this package does not make). Descriptor goldens are
upstream's own text-format values transcoded to protojson by the canonical
Go protobuf runtime — never hand-written.

Lanes, and why the split is what it is (taken from the helper bodies, not
guessed — both `ExpectHasWarnings` and `ExpectHasValidationErrors` assert
`ASSERT_EQ("", error_collector_.text_)` *after* `Parse()`, i.e. the parser
accepted):

| Lane | Upstream helper | Contract |
|---|---|---|
| valid (82) | `ExpectParsesTo` | parses **and** deep-equals the golden |
| invalid (96) | `ExpectHasErrors`, `ExpectHasEarlyExitErrors` | **rejected** |
| accept-only (50) | `ExpectHasWarnings`, `ExpectHasValidationErrors` | **accepted** (no golden published upstream) |
| excluded (6) | — | not extractable; mechanical reason recorded per case |
| leniency (13) | — | our probe inputs, **protoc's** recorded verdicts |

### The dial, measured 2026-08 — both runtimes are loudly RED, on purpose

| Lane | TypeScript | Go |
|---|---|---|
| valid — parses AND equals protoc's descriptor | **4 / 82** | **4 / 82** |
| invalid — rejected | **57 / 96** | **58 / 96** |
| accept-only — accepted | 50 / 50 | 50 / 50 |
| leniency — matches protoc's verdict | **8 / 13** | **9 / 13** |

Whole suite: TS 178 pass / 124 fail (exit 1); Go 176 pass / 129 fail (exit 1).

Dominant valid-lane failure causes, most to least common:

1. `syntax: "proto2"` emitted for a file with no declaration; protoc leaves
   `syntax` unset. Also `syntax: "editions"` missing on edition files.
2. `type: "TYPE_MESSAGE"` emitted for an unresolved type reference; protoc's
   parser leaves `type` unset and fills it during resolution.
3. `jsonName`, `defaultValue` and `extendee` land inside `options` instead of
   their own FieldDescriptorProto fields.
4. Custom options are a flat map; protoc emits `options.uninterpretedOption`.
5. `group` is not implemented at descriptor level at all — see the KNOWN GAP
   tests in `ts/test/proto.test.ts` and `go/proto_test.go`.
6. Map entry names are `Primitive_Type_MapEntry`; protoc's are
   `PrimitiveTypeMapEntry`.

The invalid lane is short because the grammar is a permissive union **by
design**: per-version legality (`required` in editions, `group` in editions,
label required in proto2, field/enum numbers in range, one `package` per
file, `extend` of a primitive, …) is not enforced anywhere. Those are the
39 / 38 wrongly-accepted cases.

### Leniency

This plugin does **not** layer on `jsonic`: the documented setup is
`new Tabnas().use(Proto)` and nothing else, so the json5-style leak (where
`'{a:1'` errors with the plugin alone but is ACCEPTED through the documented
stack) does not apply — both classify `{a:1}` and `{a:1` as errors, pinned by
`TestLeniencyBarePluginVsDocumentedStack`.

The same failure **class** lives one level down, in the shared `@tabnas/abnf`
lexer (jsonic's lexer): `#` line comments, `1_0` digit separators and `1e2`
field numbers all tokenise and become legal proto where protoc 35.1 rejects
them. `ts/test/leniency.test.ts` / `go/leniency_test.go` pin those against
**recorded protoc verdicts**, and they are RED.

### TS/Go divergence found

| Input | TypeScript | Go |
|---|---|---|
| `enum E { HEX_MAX = 0x7FFFFFFF; }` | `2147483647` (protoc agrees) | `0` |
| `optional int32 a = 0x10;` | `16` (protoc agrees) | `0` |
| `int32 a = 1_0;` | `number: null` | `number: 10` |
| `[default = 0x10000000000000000]` | `18446744073709552000` (float64 loss) | `"0x1000…"` (raw string) |
| `[default=-0x80000000000000001]` | accepted | **rejected** (the 57 vs 58) |
| empty file `""` | **TypeError out of `toDescriptor`** | accepted (protoc accepts) |
| `optional group G = 1 {}` | `typeName: ""` | `typeName` omitted |

Both runtimes read `010` as 10; protoc reads it as octal 8. Pinned in
`test/spec/int-literals.tsv` (protoc-verified), which is KNOWN RED.

**Do not make any of these numbers look better by weakening a test.**

## Build / test

```sh
./scripts/fetch-protobuf-corpus.sh   # or: make fetch-corpus  (npm pretest runs it)
cd ts && npm i && npm run build && npm test
```

Dependencies: `@tabnas/abnf` (must be the local/in-flight version with the
`TX`/`NR`/`ST`/`VL` token terminals and `wordKeywords`) and
`@tabnas/parser`. In this dev layout `@tabnas/abnf` resolves via
`file:../../abnf/ts`; `@tabnas/parser` from the registry. Node ≥ 24 in CI
(warns but runs on 22).

## Output shape

FileDescriptorProto JSON (camelCase, enum values as string names). `map<K,V>`
expands to a repeated message field + a synthesised `…Entry` nested message
with `options.mapEntry = true`. `typeName` is stored as written (no
cross-file resolution). `proto3Optional` is set for proto3 explicit
`optional`. Version recorded as `syntax` or `edition`.

[fdp]: https://protobuf.dev/reference/protobuf/google.protobuf/#file-descriptor-proto
