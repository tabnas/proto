# @tabnas/proto

Parse Protocol Buffers `.proto` IDL (proto2, proto3, editions 2023/2024)
into [FileDescriptorProto][fdp]-shaped JSON, using the
[Tabnas](https://github.com/tabnas/parser) parser and an
[ABNF](https://github.com/tabnas/abnf) grammar.

The TypeScript implementation lives in [`ts/`](ts) — see
[`ts/README.md`](ts/README.md) for usage and API.

```js
const { parse } = require('@tabnas/proto')
const fdp = parse('syntax = "proto3"; message M { int32 a = 1; }')
// fdp.messageType[0].field[0] => { name: 'a', number: 1, label: 'LABEL_OPTIONAL', type: 'TYPE_INT32' }
```

## How it works

The grammar is authored once in ABNF (`proto-grammar/*.abnf`): a shared
`common.abnf` base plus per-version deltas (`proto2`, `proto3`,
`edition-2023`, `edition-2024`) that extend it with ABNF incremental
alternatives (`name =/ alt`). `embed-grammar.js` concatenates them into a
single permissive union grammar embedded in the package. `@tabnas/abnf`
compiles that grammar to a Tabnas `GrammarSpec`; the engine parses a
`.proto` file into a CST, and a small walk assembles the
FileDescriptorProto. Version-specific legality is recorded from the
`syntax` / `edition` declaration.

The grammar is pure structure over the lexer's whole-word tokens (`TX`
identifier, `NR` number, `ST` string, `VL` keyword value); whitespace and
`//` / `/* */` comments are handled by the lexer.

## Layout

```
proto-grammar/        # ABNF grammar: common.abnf + per-version deltas
ts/                   # TypeScript implementation (plugin + descriptor walk)
```

[fdp]: https://protobuf.dev/reference/protobuf/google.protobuf/#file-descriptor-proto
