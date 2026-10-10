# Reference

## Exports

```js ignore
const { parse, toDescriptor, Proto, preflight, MAX_NESTING_DEPTH } = require('@tabnas/proto')
```

### `parse(src, options?) => FileDescriptorProto`

Parse a `.proto` source string. Builds a fresh engine per call, after
`preflight` has checked the source's nesting.

### `Proto` (Tabnas plugin)

`new Tabnas().use(Proto)` installs the union grammar; `tn.parse(src)` then
returns the raw `{rule, src, kids}` CST. A node's `src` holds its tokens
run together and no whitespace. The `constant` node of an aggregate option
value also carries `aggregate`, the text the descriptor records for that
value, described under Options below. `Proto.defaults` is
`{ version: null, reconcile: true }`. The grammar arrives compiled
(`src/proto-grammar.json`, built by `@tabnas/abnf` when the package is
made), so the plugin installs no ABNF compiler and needs none: the
engine is the only peer dependency.

### `toDescriptor(cst, options?) => FileDescriptorProto`

Turn a CST (from `tn.parse`) into a FileDescriptorProto.

### `preflight(src)` and `MAX_NESTING_DEPTH`

Refuse a source that nests deeper than `MAX_NESTING_DEPTH`, which is 100.
The depth is a count of braces outside string literals and the comments
the lexer skips, found where the lexer finds them: a line comment ends at
a carriage return as well as a line feed, a backtick string is a string,
and a quote inside a word is part of the word, so the braces after it
count. `preflight` throws an `Error` that names it. Like every
refusal the plugin makes itself, the error carries no code. `parse` runs
the check first. A caller who drives the engine directly runs it on the
source before `tn.parse`, since the engine's own parse builds the whole
tree first, and the tree's cost grows with the square of its depth. The
Go port's `Preflight` and the Rust port's `preflight` refuse the same
documents with the same message.

```js
const { parse, MAX_NESTING_DEPTH } = require('@tabnas/proto')

const deep = 'message M {'.repeat(101) + '}'.repeat(101)
let refusal = ''
try { parse(deep) } catch (err) { refusal = err.message }
refusal // => 'proto: document nests 101 levels deep, past the 100 this parser accepts'
MAX_NESTING_DEPTH // => 100
```

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

### Member order

The object lists its members in the order the walk first sets them. The
Go and Rust ports give the same order in the trees their `ParseValue`
and `parse_value` build, which a host walking the value relies on.
Most members have a fixed place. The file's lists come first, then
`edition` and `syntax`. A field reads `name`, `number`, `label`,
`proto3Optional`, then `type` and `typeName`, then `jsonName`,
`defaultValue`, `options`, `extendee` and last `oneofIndex`. A statement
places the rest, so they follow the source: a file's `package`,
`optionDependency` and `options`, a message's `extensionRange`,
`reservedRange` and `reservedName`, which come after its `options`
because the walk reads options first, and an enum's `reservedRange`,
`reservedName` and `options`. An option map lists its names in the order
the source first sets them, and a repeated name keeps its first place and
takes the last value.

```js
const { parse } = require('@tabnas/proto')

const tail = (src) => Object.keys(parse(src)).slice(7)
tail('option java_package = "x";\npackage p;') // => ['syntax', 'options', 'package']
tail('package p;\noption java_package = "x";') // => ['syntax', 'package', 'options']
```

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
written). An `extend` member records the message it extends in `extendee`.

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

An aggregate value, `option (f) = { a: 1 };`, is a string: the text
between the braces, as `protoc` records it in `aggregate_value`. The text
keeps its whitespace and newlines. Each comment becomes the newlines and
spaces that keep the next token on its line and column, and a comment
directly ahead of the closing brace leaves nothing. A column is what
`protoc` counts as one: a byte of UTF-8, with a tab moving to the next
multiple of 8. Inside the braces you may write a string value as adjacent
literals, `a: "x" "y"`, as text format allows.

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
`syntax`/`edition` value, on a version mismatch when `reconcile` is
true, and on a document nesting deeper than `MAX_NESTING_DEPTH`. The
last three are the plugin's own refusals and carry no error code.
