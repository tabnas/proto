# How-to guide

Task-focused recipes. Each block is runnable.

## Read fields, numbers, labels and types

```js
const { parse } = require('@tabnas/proto')
const f = parse('syntax="proto3"; message M { repeated int32 xs = 7; }')
  .messageType[0].field[0]
f.name    // => 'xs'
f.number  // => 7
f.label   // => 'LABEL_REPEATED'
f.type    // => 'TYPE_INT32'
```

## Handle `map` fields

A `map<K,V>` becomes a repeated message field plus a synthesised
`…Entry` nested message (`options.mapEntry = true`), as in `protoc`:

```js
const { parse } = require('@tabnas/proto')
const m = parse('syntax="proto3"; message M { map<string,int32> by = 1; }')
  .messageType[0]
m.field[0].label                 // => 'LABEL_REPEATED'
m.field[0].typeName              // => 'ByEntry'
m.nestedType[0].options.mapEntry // => true
```

## Read a `oneof`

Members are normal fields tagged with `oneofIndex`:

```js
const { parse } = require('@tabnas/proto')
const m = parse('syntax="proto3"; message M { oneof x { string a = 1; int32 b = 2; } }')
  .messageType[0]
m.oneofDecl[0].name           // => 'x'
m.field[0].oneofIndex         // => 0
```

## Read services

```js
const { parse } = require('@tabnas/proto')
const svc = parse('syntax="proto3"; service S { rpc Watch (Req) returns (stream Res); }')
  .service[0]
svc.method[0].name             // => 'Watch'
svc.method[0].serverStreaming  // => true
```

## Field and file options

```js
const { parse } = require('@tabnas/proto')
const fdp = parse('syntax="proto3"; message M { int32 a = 1 [deprecated = true]; }')
fdp.messageType[0].field[0].options.deprecated  // => true
```

## Reuse an engine for many files

```js
const { Tabnas } = require('@tabnas/parser')
const { Proto, toDescriptor } = require('@tabnas/proto')

const j = new Tabnas().use(Proto)
const fdp = toDescriptor(j.parse('syntax="proto3"; message M {}'))
fdp.messageType[0].name  // => 'M'
```
