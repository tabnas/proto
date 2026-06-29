# @tabnas/proto

Parse Protocol Buffers `.proto` IDL — **proto2, proto3, and editions 2023
/ 2024** — into [FileDescriptorProto][fdp]-shaped JSON, using the
[Tabnas](https://github.com/tabnas/parser) parser driven by an
[ABNF](https://github.com/tabnas/abnf) grammar.

```js
const { parse } = require('@tabnas/proto')

const fdp = parse(`
  syntax = "proto3";
  package example;
  message Person {
    string name = 1;
    repeated string emails = 2;
  }
`)

fdp.syntax                              // => 'proto3'
fdp.package                             // => 'example'
fdp.messageType[0].name                 // => 'Person'
fdp.messageType[0].field[1].label       // => 'LABEL_REPEATED'
fdp.messageType[0].field[0].type        // => 'TYPE_STRING'
```

## Versions

The version is auto-detected from the file's `syntax` / `edition`
declaration, and/or set explicitly with the `version` option:

```js
const { parse } = require('@tabnas/proto')

parse('edition = "2023";').edition             // => 'EDITION_2023'
parse('message M {}', { version: 'proto3' }).syntax  // => 'proto3'
```

When both an explicit `version` and a declaration are present they must
agree (`reconcile: true`, the default) or `parse` throws; set
`reconcile: false` to let the declaration win.

## API

- `parse(src, options?) => FileDescriptorProto` — parse a `.proto` string.
- `Proto` — the Tabnas plugin; `new Tabnas().use(Proto)` installs the
  grammar so `tn.parse(src)` returns the raw `{rule, src, kids}` CST.
- `toDescriptor(cst, options?)` — turn a parsed CST into a
  FileDescriptorProto.

Options: `{ version?: 'proto2'|'proto3'|'2023'|'2024' | null,
reconcile?: boolean }`.

## What it produces

A `FileDescriptorProto`-shaped object (the `descriptor.proto` JSON shape):
`package`, `dependency` (+ `publicDependency` / `weakDependency`),
`messageType` (recursive `DescriptorProto` with `field`, `nestedType`,
`enumType`, `oneofDecl`, `extensionRange`, `reservedRange`), `enumType`,
`service`, `extension`, `options`, and `syntax` / `edition`. `map<K,V>`
fields are expanded to a repeated message field plus a synthesised
`…Entry` nested message with `options.mapEntry = true`, exactly as `protoc`
does. Type names are stored as written; cross-file resolution is a separate
concern.

See [doc/tutorial.md](doc/tutorial.md), [doc/guide.md](doc/guide.md),
[doc/reference.md](doc/reference.md), and [doc/concepts.md](doc/concepts.md).

[fdp]: https://protobuf.dev/reference/protobuf/google.protobuf/#file-descriptor-proto
