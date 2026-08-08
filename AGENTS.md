# Agents Guide — proto

## What this project is

`@tabnas/proto` parses Protocol Buffers `.proto` IDL — **proto2, proto3,
and editions 2023/2024** — into [FileDescriptorProto][fdp]-shaped JSON. It
drives the [Tabnas](https://github.com/tabnas/parser) engine with an
[ABNF](https://github.com/tabnas/abnf) grammar rather than a hand-written
parser. TypeScript (`ts/`) is canonical; Go (`go/`) is a port that tracks
it, and the two are held together by the shared `test/spec/*.tsv` fixtures.

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
  test/                # node:test (proto, version-detect, doc-examples,
                       #   parity over test/spec, protobuf-conformance,
                       #   version: exported VERSION == package.json)
go/
  grammar_gen.go       # Go counterpart of embed-grammar.js (go generate)
  grammar.go           # GENERATED — do not edit
  build_descriptor.go  # port of ts/src/build-descriptor.ts
  descriptor.go        # port of ts/src/descriptor.ts
  detect_version.go    # port of ts/src/detect-version.ts
  parity_test.go       # runs the same test/spec/*.tsv fixtures
  version_test.go      # VERSION const == ts/package.json "version"
test/
  spec/*.tsv           # shared cross-runtime fixtures (see test/AGENTS.md)
  protobuf-suite/      # vendored protoc parser_unittest corpus
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

## Build / test

```sh
cd ts && npm i && npm run build && npm test
```

Dependencies: `@tabnas/abnf` (must be the local/in-flight version with the
`TX`/`NR`/`ST`/`VL` token terminals and `wordKeywords`) and
`@tabnas/parser`. In this dev layout `@tabnas/abnf` resolves via
`file:../../abnf/ts`; `@tabnas/parser` from the registry. Node ≥ 24 in CI
(warns but runs on 22).

## Output shape

FileDescriptorProto JSON (camelCase, enum values as string names). The walk
reproduces the descriptor `protoc`'s **parser** produces, i.e. before its
name-resolution pass. Specifically:

- A named field type cannot be told apart from an enum without resolution,
  so `type` is left **unset** and only `typeName` is recorded, as written.
  Only scalars (and `group`, which is syntactically known) get a `type`.
- `map<K,V>` expands to a repeated field + a synthesised `…Entry` nested
  message with `options.mapEntry = true`; the entry name is the field name
  CamelCased with `_` removed (`map_field` -> `MapFieldEntry`), and
  `features.*` options are copied onto the entry's key/value fields.
- `group` expands to a `TYPE_GROUP` field with the **lower-cased** name plus
  a nested message keeping the declared name.
- proto3 explicit `optional` sets `proto3Optional` and synthesises a
  `_<field>` oneof appended after the declared ones (`X`-prefixed until
  unique); the field's `oneofIndex` points at it.
- `json_name` and `default` are pseudo-options, lifted to `jsonName` /
  `defaultValue`. An `extend` member records `extendee`.
- `extensionRange` and message `reservedRange` are half-open (`end`
  exclusive); enum `reservedRange` is closed (`end` inclusive). `to max` is
  536870912 / 2147483647 (`message_set_wire_format`) / 2147483647 (enum).
- edition 2024: `import option` fills `optionDependency`; `export` / `local`
  set `visibility`.
- A syntax file records `syntax`; an **edition** file records both
  `syntax: "editions"` and `edition: "EDITION_20NN"`, as `protoc` does.

### Declared deviations from protoc

Two, both deliberate and both bridged (not waived) by the conformance
runner, so a real capture failure still turns it red:

1. **Options are a plain `{ name: value }` map**, keyed by the option name
   exactly as written (`ctype`, `(foo)`, `foo.(.bar.baz).qux`), rather than
   `protoc`'s `uninterpretedOption` list. Same information, friendlier shape.
2. **`defaultValue` keeps the literal as written.** `protoc` re-renders a
   numeric default through the field's C++ type
   (`18446744073709551616` -> `1.8446744073709552e+19`); we do not.

Everything else that diverges from `protoc` is a bug.

## Conformance

The bar: **protoc 35.1's own parser test corpus**, extracted from upstream
`src/google/protobuf/compiler/parser_unittest.cc` and vendored under
`test/protobuf-suite/` (see its AGENTS.md). `ts/test/protobuf-conformance.test.ts`
runs it — nothing is skipped, and the corpus is in-repo so it needs no
network:

- `valid` (82 cases): source + the descriptor protoc's parser produces.
  **71/71 in-scope pass.** The 11 excluded declare protoc-internal editions
  (`UNSTABLE`, `NNNNN_TEST_ONLY`) outside the proto2/proto3/2023/2024
  support this package claims; the runner asserts the exclusion set is
  exactly those.
- `accept-only` (50 cases): source protoc's parser accepts without
  publishing a descriptor. **50/50 parse.**
- `invalid` (96 cases): source protoc **rejects**. This lane is
  deliberately NOT a pass/fail gate — `common.abnf` is a permissive union
  and per-version legality is the walk's / protoc's concern (see above), so
  the parser accepts roughly half of them. Rejection is not part of the
  contract; recognition and descriptor shape are.
- `leniency`: probes where the shared tabnas lexer is more permissive than
  `.proto` (a `#` comment, `1_0` digit separators, `1e2` where an intLit is
  required). Recorded, and pinned by the conformance runner so the
  deviation surface cannot silently grow.

[fdp]: https://protobuf.dev/reference/protobuf/google.protobuf/#file-descriptor-proto
