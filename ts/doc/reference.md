# Reference

## Exports

```js ignore
const { parse, toDescriptor, Proto } = require('@tabnas/proto')
```

### `parse(src, options?) => FileDescriptorProto`

Parse a `.proto` source string. Builds a fresh engine per call.

### `Proto` (Tabnas plugin)

`new Tabnas().use(Proto)` installs the union grammar; `tn.parse(src)` then
returns the raw `{rule, src, kids}` CST. A node's `src` holds its tokens
run together and no whitespace. The `constant` node of an aggregate option
value also carries `aggregate`, the text the descriptor records for that
value, described under Options below. `Proto.defaults` is
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
- `messageType[]`. `DescriptorProto`: `name`, `field[]`, `nestedType[]`,
  `enumType[]`, `oneofDecl[]`, `extension[]`, `extensionRange[]`,
  `reservedRange[]`, `reservedName[]`, `visibility?`, `options?`
- `enumType[]`. `EnumDescriptorProto`: `name`, `value[]`, `reservedRange[]`,
  `reservedName[]`, `visibility?`, `options?`
- `service[]`. `ServiceDescriptorProto`: `name`, `method[]`, `options?`
- `extension[]`, `options?`, and `syntax?` / `edition?`

`syntax` is `'proto2'` / `'proto3'` for a syntax file and `'editions'` for
an edition file; an edition file carries both `syntax` and `edition`, as
`protoc` emits them.

### `FieldDescriptorProto`

`name`, `number`, `label` (`LABEL_OPTIONAL` / `LABEL_REQUIRED` /
`LABEL_REPEATED`), `type?` (`TYPE_*`), `typeName?` (for message/enum/group
types, stored as written), `extendee?`, `jsonName?`, `defaultValue?`,
`proto3Optional?`, `oneofIndex?`, `options?`.

Scalar types map to `TYPE_DOUBLE … TYPE_SINT64`. Any other type is a
message-or-enum reference that cannot be told apart without symbol
resolution, so (exactly as `protoc`'s parser does before its resolution
pass) `type` is left **unset** and only `typeName` is recorded, as
written. Cross-file / scope resolution is a separate pass.

`json_name` and `default` are pseudo-options: they are lifted out of
`options` into `jsonName` and `defaultValue` (a string, the literal as
written, or adjacent literals read as described under Options below). An
`extend` member records the message it extends in `extendee`.

### Ranges

`extensionRange` / message `reservedRange` are half-open: `end` is
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
`foo.(.bar.baz).qux`), not `protoc`'s `uninterpretedOption` list. The
information is the same; the shape is friendlier to read. Values are
JavaScript strings / numbers / booleans, with identifiers (`CORD`, `inf`,
`-nan`) kept verbatim.

A string written as one literal keeps its escapes as written: `"\x41"` is
`\x41`. A string written as adjacent literals, `"a" "b"`, is the one
string `protoc` records: each literal decoded and the results joined, so
`"\x41" "b"` is `Ab`. Where the decoded bytes fall outside UTF-8, the
Unicode replacement character stands in for each ill-formed sequence, and
a `bytes` field's default comes back escaped again, as `protoc` escapes
it. The same holds wherever `protoc` reads a string: `syntax`, `edition`,
`import`, `default`, `json_name` and a reserved name. `parse` refuses the
adjacent literals that `protoc` refuses as well: a backtick string, or an
escape outside the set `protoc` knows, such as `\e`. A single literal
holding such an escape is still accepted, and kept as written.

An aggregate value, `option (f) = { a: 1 };`, is a string: the text
between the braces, as `protoc` records it in `aggregate_value`. The text
keeps its whitespace and newlines. Each comment becomes the newlines and
spaces that keep the next token on its line and column, and a comment
directly ahead of the closing brace leaves nothing. A column is what
`protoc` counts as one: a byte of UTF-8, with a tab moving to the next
multiple of 8.

Inside the braces you may write what text format takes there:

- A string value as adjacent literals. `a: "x" "y"`
- A repeated field's values as a list. `a: [1, 2]`
- A message in angle brackets. `a < b: 1 >`
- A field named by an extension or an Any type URL. `[x.y]: 1`,
  `[type.googleapis.com/x.Y] { a: 1 }`
- A field name or enum value spelt like a keyword. `message: optional`

`parse` checks text format only in part. It reads a bracketed name as
`protoc` does: it refuses a name that `protoc` fails to split into
tokens, such as `[a.2/x.Y]` or `[1p.example/x.Y]`, and joins the pieces
of the rest, so `[x.y 2]` names `x.y2`. It also refuses some text that `protoc`'s
parser, which only matches the braces, would record, such as a trailing
comma in a list, and some text format, such as `.5`. It accepts some text
that text format refuses, such as `a: foo/x`.

```js
const { parse } = require('@tabnas/proto')

const fdp = parse(`message M {
  option (f) = {
    a: 1 // one
    b: 2
  };
}`)
fdp.messageType[0].options['(f)'] // => '\n    a: 1 \n    b: 2\n  '
```

## Errors

`parse` throws on malformed input (a Tabnas parse error), on an unknown
`syntax`/`edition` value, and on a version mismatch when `reconcile` is
true.
