# Tutorial — your first `.proto` parse

This walks you from nothing to a parsed FileDescriptorProto.

## 1. Install

```sh
npm install @tabnas/proto @tabnas/parser @tabnas/abnf
```

## 2. Parse a file

```js
const { parse } = require('@tabnas/proto')

const fdp = parse(`
  syntax = "proto3";
  package shop;

  message Order {
    string id = 1;
    repeated Item items = 2;
  }

  message Item {
    string sku = 1;
    int32 qty = 2;
  }
`)

fdp.syntax                          // => 'proto3'
fdp.package                         // => 'shop'
fdp.messageType.length              // => 2
fdp.messageType[0].name             // => 'Order'
fdp.messageType[0].field[1].label   // => 'LABEL_REPEATED'
fdp.messageType[0].field[1].typeName // => 'Item'
```

## 3. Different versions

The version is read from the `syntax` / `edition` line:

```js
const { parse } = require('@tabnas/proto')

parse('syntax = "proto2";').syntax     // => 'proto2'
parse('edition = "2024";').edition     // => 'EDITION_2024'
```

If a file has no declaration, pass `version`:

```js
const { parse } = require('@tabnas/proto')
parse('message M {}', { version: 'proto3' }).syntax  // => 'proto3'
```

## Next steps

- [guide.md](guide.md) — task recipes (maps, oneofs, services, options).
- [reference.md](reference.md) — the full API and output shape.
- [concepts.md](concepts.md) — how the grammar and walk work.
