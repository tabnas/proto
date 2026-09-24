# @tabnas/proto

<!-- tabnas-badges -->
[![npm](https://tabnas.github.io/status/badges/proto-npm.svg)](https://www.npmjs.com/package/@tabnas/proto)
[![CI](https://github.com/tabnas/proto/actions/workflows/ci.yml/badge.svg)](https://github.com/tabnas/proto/actions/workflows/ci.yml)
[![go](https://tabnas.github.io/status/badges/proto-go.svg)](https://pkg.go.dev/github.com/tabnas/proto/go)
[![tabnas standard](https://tabnas.github.io/status/badges/proto-standard.svg)](https://tabnas.github.io/status/)
<!-- /tabnas-badges -->

Parse Protocol Buffers `.proto` IDL (proto2, proto3, editions 2023/2024)
into [FileDescriptorProto][fdp]-shaped JSON, using the
[Tabnas](https://github.com/tabnas/parser) parser and an
[ABNF](https://github.com/tabnas/abnf) grammar.

Docs, guides, the error reference and the playground: **[tabnas.dev](https://tabnas.dev)**.

The TypeScript implementation lives in [`ts/`](ts). See
[`ts/README.md`](ts/README.md) for usage and API. Ports that track it live
in [`go/`](go) and [`rs/`](rs), the latter documented in
[`rs/README.md`](rs/README.md); all three run the shared fixtures in
[`test/spec`](test/spec).

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

## Conformance

The output is checked against **protoc 36.2's own parser test corpus**,
extracted from upstream `parser_unittest.cc` and vendored under
[`test/protobuf-suite`](test/protobuf-suite): all 78 in-scope `valid` cases
(source + the exact descriptor protoc's parser produces) and all 50
`accept-only` cases pass, in every runtime. The other 11 `valid` cases
declare protoc-internal editions (`UNSTABLE`, `99998_TEST_ONLY`) outside the
proto2 / proto3 / 2023 / 2024 support this package claims, and are excluded,
with the exclusion set asserted to be exactly those. The corpus also carries
an `invalid` lane of 99 cases, which is not a gate in any runtime, for the
reason the next paragraph gives.

The grammar is a permissive *union* of the four versions, so rejecting
version-illegal input is deliberately not part of the contract: recognition
and descriptor shape are. See [`AGENTS.md`](AGENTS.md) for the full
statement, including the two declared output-shape deviations from protoc.

## Layout

```
proto-grammar/        # ABNF grammar: common.abnf + per-version deltas
ts/                   # TypeScript implementation (plugin + descriptor walk)
go/                   # Go port, tracking ts/
rs/                   # Rust port, tracking ts/
test/spec/            # shared .tsv fixtures, run by EVERY runtime
test/divergent.tsv    # where the ports disagree, executed
test/protobuf-suite/  # vendored protoc parser_unittest conformance corpus
```

[fdp]: https://protobuf.dev/reference/protobuf/google.protobuf/#file-descriptor-proto
