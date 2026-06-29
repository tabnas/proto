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

FileDescriptorProto JSON (camelCase, enum values as string names). `map<K,V>`
expands to a repeated message field + a synthesised `…Entry` nested message
with `options.mapEntry = true`. `typeName` is stored as written (no
cross-file resolution). `proto3Optional` is set for proto3 explicit
`optional`. Version recorded as `syntax` or `edition`.

[fdp]: https://protobuf.dev/reference/protobuf/google.protobuf/#file-descriptor-proto
