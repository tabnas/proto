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
- `package?`, `optionDependency[]?` (edition 2024 `import option`)
- `messageType[]` — `DescriptorProto`: `name`, `field[]`, `nestedType[]`,
  `enumType[]`, `oneofDecl[]`, `extension[]`, `extensionRange[]`,
  `reservedRange[]`, `reservedName[]`, `visibility?`, `options?`
- `enumType[]` — `EnumDescriptorProto`: `name`, `value[]`, `reservedRange[]`,
  `reservedName[]`, `visibility?`, `options?`
- `service[]` — `ServiceDescriptorProto`: `name`, `method[]`, `options?`
- `extension[]`, `options?`, and `syntax?` / `edition?`

`syntax` is `'proto2'` / `'proto3'` for a syntax file and `'editions'` for
an edition file — an edition file carries both `syntax` and `edition`, as
`protoc` emits them.

### `FieldDescriptorProto`

`name`, `number`, `label` (`LABEL_OPTIONAL` / `LABEL_REQUIRED` /
`LABEL_REPEATED`), `type?` (`TYPE_*`), `typeName?` (for message/enum/group
types, stored as written), `extendee?`, `jsonName?`, `defaultValue?`,
`proto3Optional?`, `oneofIndex?`, `options?`.

Scalar types map to `TYPE_DOUBLE … TYPE_SINT64`. Any other type is a
message-or-enum reference that cannot be told apart without symbol
resolution, so — exactly as `protoc`'s parser does before its resolution
pass — `type` is left **unset** and only `typeName` is recorded, as
written. Cross-file / scope resolution is a separate pass.

`json_name` and `default` are pseudo-options: they are lifted out of
`options` into `jsonName` and `defaultValue` (a string, the literal as
written). An `extend` member records the message it extends in `extendee`.

### Ranges

`extensionRange` / message `reservedRange` are half-open — `end` is
**exclusive**, so `extensions 100 to 199` is `{ start: 100, end: 200 }`.
Enum `reservedRange` is closed: `end` is **inclusive**. `to max` is
`536870912` (exclusive) for message ranges, `2147483647` in a
`message_set_wire_format` message, and `2147483647` for enum ranges. This
is `protoc`'s own asymmetry.

### `group` (proto2)

`optional group TheGroup = 1 { … }` expands to a `TYPE_GROUP` field named
`thegroup` (lower-cased, as `protoc` does) with `typeName: 'TheGroup'`,
plus a nested message `TheGroup` carrying the body.

### `map<K,V>`

A map field becomes a `LABEL_REPEATED` field whose `typeName` is a
synthesised nested `<Name>Entry` message with `options.mapEntry = true` and
`key` (1) / `value` (2) fields. The entry name is the field name
CamelCased with underscores removed (`map_field` -> `MapFieldEntry`), and
any `features.*` options on the map field are copied onto the entry's key
and value fields.

### proto3 explicit `optional`

`optional` in proto3 sets `proto3Optional` and, as in `protoc`, synthesises
a single-field oneof named `_<field>` appended after the declared oneofs
(`X`-prefixed until the name is unique); the field's `oneofIndex` points at
it.

### Options

Options are a plain `{ name: value }` map keyed by the option name exactly
as written (`ctype`, `(foo)`, `features.field_presence`,
`foo.(.bar.baz).qux`) — not `protoc`'s `uninterpretedOption` list. The
information is the same; the shape is friendlier to read. Values are
JavaScript strings / numbers / booleans, with identifiers (`CORD`, `inf`,
`-nan`) kept verbatim.

## Errors

`parse` throws on malformed input (a Tabnas parse error), on an unknown
`syntax`/`edition` value, and on a version mismatch when `reconcile` is
true.
