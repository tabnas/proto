# Reference

## Exports

```js ignore
const { parse, toDescriptor, Proto } = require('@tabnas/proto')
```

### `parse(src, options?) => FileDescriptorProto`

Parse a `.proto` source string. Builds a fresh engine per call.

### `Proto` (Tabnas plugin)

`new Tabnas().use(Proto)` installs the union grammar; `tn.parse(src)` then
returns the raw `{rule, src, kids}` CST. `Proto.defaults` is
`{ version: null, reconcile: true }`. The plugin installs `@tabnas/abnf`
automatically if it is not already present.

### `toDescriptor(cst, options?) => FileDescriptorProto`

Turn a CST (from `tn.parse`) into a FileDescriptorProto.

## Options

| Field | Type | Default | Meaning |
|---|---|---|---|
| `version` | `'proto2'｜'proto3'｜'2023'｜'2024'｜null` | `null` | Explicit version; `null` auto-detects from the file. |
| `reconcile` | `boolean` | `true` | Error when `version` disagrees with the file's declaration; `false` lets the declaration win. |

With no declaration and no `version`, the default is `proto2` (matching
`protoc`).

## Output shape

`FileDescriptorProto` mirrors `descriptor.proto`'s JSON form (camelCase
fields; enum values as their string names):

- `package?`, `dependency[]`, `publicDependency[]`, `weakDependency[]`
- `messageType[]` — `DescriptorProto`: `name`, `field[]`, `nestedType[]`,
  `enumType[]`, `oneofDecl[]`, `extension[]`, `extensionRange[]`,
  `reservedRange[]`, `reservedName[]`, `options?`
- `enumType[]` — `EnumDescriptorProto`: `name`, `value[]`, `reservedRange[]`,
  `reservedName[]`, `options?`
- `service[]` — `ServiceDescriptorProto`: `name`, `method[]`
- `extension[]`, `options?`, and `syntax?` / `edition?`

### `FieldDescriptorProto`

`name`, `number`, `label` (`LABEL_OPTIONAL` / `LABEL_REQUIRED` /
`LABEL_REPEATED`), `type` (`TYPE_*`), `typeName?` (for message/enum types,
stored as written), `proto3Optional?`, `oneofIndex?`, `options?`.

Scalar types map to `TYPE_DOUBLE … TYPE_SINT64`; any other type is treated
as a message/enum reference: `type` is `TYPE_MESSAGE` and `typeName` holds
the name as written. Cross-file / scope resolution is a separate pass.

### `map<K,V>`

A map field becomes a `LABEL_REPEATED` `TYPE_MESSAGE` field whose
`typeName` is a synthesised nested `<Name>Entry` message with
`options.mapEntry = true` and `key` (1) / `value` (2) fields.

## Errors

`parse` throws on malformed input (a Tabnas parse error), on an unknown
`syntax`/`edition` value, and on a version mismatch when `reconcile` is
true.
