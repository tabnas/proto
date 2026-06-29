# Concepts

How `@tabnas/proto` is built, and why.

## Grammar, not hand-rolled parser

Protocol Buffers' `.proto` language is published only as informal EBNF on
[protobuf.dev](https://protobuf.dev/), with one spec page per version and
no single reusable parser. This package authors the language **once in
ABNF** and lets [`@tabnas/abnf`](https://github.com/tabnas/abnf) compile it
to a Tabnas `GrammarSpec`. The engine parses a `.proto` file into a generic
`{rule, src, kids}` CST; a small TypeScript walk turns that into a
FileDescriptorProto.

## Simple lexer, structure in the grammar

The Tabnas lexer already tokenises whole words and **ignores** whitespace
and `//` / `/* */` comments between tokens. So the grammar carries no
whitespace rules and no char-by-char lexical definitions — it is pure
structure over the lexer's built-in tokens, referenced by name:

- `TX` — identifier (`ident = TX`)
- `NR` — number (`fieldNumber = NR`)
- `ST` — string (`strLit = ST`)
- `VL` — `true` / `false` / `null`

Keywords are whole-word matched (`@tabnas/abnf`'s `wordKeywords` option) so
`option` never grabs the `option` prefix of an identifier `optional`.

## One permissive union grammar

`common.abnf` defines the shared core. Per-version delta files extend it
with ABNF incremental alternatives (`name =/ alt`):

- `proto2.abnf` — `group`
- `edition-2023.abnf` — `edition = "…";`
- `edition-2024.abnf` — `import option`, `export` / `local` visibility

(labels, `extend`, `extensions` are shared in `common.abnf`). The five
files are concatenated into a single permissive grammar that accepts every
version's syntax. Which constructs are *legal* for the resolved version is
a concern of the walk and of `protoc`, not of recognition — keeping the
grammar small and the spec pages easy to mirror.

## The walk and abnf inlining

The descriptor walk keys off CST rule names. One wrinkle: abnf inlines a
sub-rule referenced at the very start of an alternative (Paull's
left-recursion elimination), so the specific statement rule (`message`,
`field`, …) is folded into its enclosing dispatch node (`topLevelDef`,
`messageElement`). The walk recovers the statement kind from the keyword
that precedes the node's first child, and reads inlined values (a leading
type, the first `reserved` range) from the node's `src`. Whole-word
tokenisation is what makes that `src` reading unambiguous.

## Versions

The resolved version (from the `syntax` / `edition` declaration, reconciled
with the `version` option) is recorded as `syntax` (`proto2` / `proto3`) or
`edition` (`EDITION_2023` / `EDITION_2024`), and drives version-sensitive
descriptor details such as `proto3Optional`.

## Out of scope (for now)

A Go port (mirroring the `@tabnas/zon` / `@tabnas/abnf` dual-runtime
layout), edition *feature* resolution (e.g. `features.field_presence`
driving presence defaults — features are recorded verbatim in `options`),
cross-file type resolution, and the protobuf text/wire formats.
