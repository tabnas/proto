# Reference (Go)

The complete public surface of the Go `zon` module: exports, the parse
entry points, the two options, and the exact ZON syntax accepted. For
a guided introduction see the [tutorial](tutorial.md); for task recipes
see the [how-to guide](guide.md); for how it works (and how it differs
from TypeScript) see [concepts](concepts.md).

## Module

```bash
go get github.com/tabnas/zon/go@latest
```

```go
import tabnaszon "github.com/tabnas/zon/go"
```

| | |
|---|---|
| Module | `github.com/tabnas/zon/go` |
| Package | `tabnaszon` |
| Engine | `github.com/tabnas/jsonic/go` (pulled in transitively) |
| `Version` | exported `const` string of the module version |

## Public API

### `func Parse(src string, opts ...ZonOptions) (any, error)`

Parses a ZON string and returns the resulting value. Convenience
wrapper around `MakeJsonic(opts...).Parse(src)`.

With **no** options it reuses a single lazily-created instance, so
repeated calls do not rebuild the engine + grammar. The shared instance
is safe for concurrent use (each parse builds its own context and only
reads instance state). With options, a dedicated instance is built per
call, since the configuration differs per call.

```go
result, err := tabnaszon.Parse(`.{ .a = 1 }`)
// result: map[string]any{"a": float64(1)}
```

### `func MakeJsonic(opts ...ZonOptions) *tabnasjsonic.Jsonic`

Returns a reusable `*tabnasjsonic.Jsonic` instance configured for ZON
parsing. Use this when parsing many strings with the same options:
build once, call `.Parse()` per input.

```go
j := tabnaszon.MakeJsonic()
result, err := j.Parse(`.{ 1, 2, 3 }`)
// result: []any{float64(1), float64(2), float64(3)}
```

A plugin-registration failure (a programming error with static inputs)
panics rather than misbehaving silently.

### `func Zon(j *tabnasjsonic.Jsonic, options map[string]any) error`

The raw plugin function. Usually invoked indirectly through
`j.UseDefaults(tabnaszon.Zon, tabnaszon.Defaults, opts...)` or via `Parse` /
`MakeJsonic`. It is idempotent: a re-invocation guard
(`zon-init` decoration) makes re-application during `SetOptions` a
no-op.

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnaszon.Zon, tabnaszon.Defaults)
result, err := j.Parse(`.{ .a = 1 }`)
```

### `var Defaults map[string]any`

The default option map, paired with `Zon` for `UseDefaults`:

```go
var Defaults = map[string]any{
    "charAsNumber": false,
    "enumTag":      "",
}
```

### `type ZonOptions struct`

A typed wrapper over the option map. Fields use pointer / empty-value
conventions so callers can express "omit" vs "set":

```go
type ZonOptions struct {
    // When non-nil and true, parses Zig char literals ('x') as numeric
    // code points. When nil or false (default), they are one-char strings.
    CharAsNumber *bool

    // When non-empty, wraps enum literals (.foo used as value) in
    // map[string]any{<EnumTag>: name} instead of producing bare strings.
    EnumTag string
}
```

## Options

### `CharAsNumber`

- **Type:** `*bool`
- **Default:** `false` (nil)
- **Effect:** Controls how Zig character literals (`'x'`, `'\n'`,
  `'\x41'`, `'\u{1F600}'`) are parsed.
  - nil / `false` — the literal becomes a one-character `string`. `'A'`
    → `"A"`.
  - `true` — the literal becomes its numeric Unicode code point as a
    `float64`. `'A'` → `float64(65)`, `'\n'` → `float64(10)`,
    `'\u{1F600}'` → `float64(0x1F600)`.

```go
charAsNum := true
tabnaszon.Parse(`'A'`, tabnaszon.ZonOptions{CharAsNumber: &charAsNum}) // float64(65)
```

### `EnumTag`

- **Type:** `string`
- **Default:** `""`
- **Effect:** Controls how enum-literal *values* (a bare `.foo` used in
  value position) are represented.
  - `""` — the enum literal becomes the bare identifier `string`. `.red`
    → `"red"`.
  - a non-empty string `T` — the enum literal is wrapped in a one-key
    map `map[string]any{T: name}`, so it can be told apart from a plain
    string. With `EnumTag: "$enum"`, `.red` →
    `map[string]any{"$enum": "red"}`.

`EnumTag` affects enum literals only as values. A `.field` used as a
key (before `=`) is always the plain field name.

```go
tabnaszon.Parse(`.{ .kind = .red }`, tabnaszon.ZonOptions{EnumTag: "$enum"})
// map[string]any{"kind": map[string]any{"$enum": "red"}}
```

## Value types

`Parse` returns `any`; the concrete Go types are predictable:

| ZON value | Go type |
|---|---|
| Struct literal | `map[string]any` |
| Tuple / empty literal | `[]any` |
| String, enum literal, char-as-string | `string` |
| Number (any base, float, char-as-number) | `float64` |
| Boolean | `bool` |
| Null | `nil` |
| Tagged enum (`EnumTag` set) | `map[string]any{tag: name}` |

## ZON syntax

ZON is **not** a superset of JSON. It uses Zig anonymous-struct
syntax. The plugin disables the bare `{`, `[`, `]` openers and rebinds
the key/value separator to `=`.

### Structs (maps)

Open with `.{`, contain `.field = value` pairs separated by commas,
close with `}`. Field names are identifiers
(`[A-Za-z_][A-Za-z0-9_]*`); the leading dot is stripped from the key.

```
.{ .a = 1, .b = 2 }       => map[string]any{"a": 1, "b": 2}
.{ .a = .{ .b = 1 } }     => map[string]any{"a": map[string]any{"b": 1}}
```

### Tuples (lists)

Also open with `.{`, but contain bare values (no `.field =`), separated
by commas, and close with `}`. Produces a `[]any`.

```
.{ 1, 2, 3 }              => []any{1, 2, 3}
.{ .{ 1, 2 }, .{ 3, 4 } } => []any{[]any{1, 2}, []any{3, 4}}
```

The struct-vs-tuple decision is made at lex time by peeking past `.{`:
if the next significant token is `.identifier` followed by `=`, it is a
struct; otherwise a tuple.

### Empty literal

An empty `.{}` parses as an **empty list** (`[]any{}`).

```
.{}                       => []any{}
```

### Trailing commas

Allowed before `}` in both structs and tuples.

```
.{ .a = 1, }              => map[string]any{"a": 1}
.{ 1, 2, 3, }             => []any{1, 2, 3}
```

### Scalars

| Construct | Example | Result |
|---|---|---|
| Integer | `42` | `float64(42)` |
| Float | `3.14` | `float64(3.14)` |
| Hex | `0x2a` | `float64(42)` |
| Octal | `0o52` | `float64(42)` |
| Binary | `0b101010` | `float64(42)` |
| Digit separator | `1_000_000` | `float64(1000000)` |
| Boolean | `true`, `false` | `true`, `false` |
| Null | `null` | `nil` |
| String | `"hello"` | `"hello"` |
| Enum literal | `.red` | `"red"` (or tagged map) |
| Char literal | `'A'` | `"A"` (or `float64(65)`) |

### Strings

Double-quoted strings only (single quotes are char literals).
Zig-flavoured escapes: `\n`, `\r`, `\t`, `\\`, `\"`, `\'`. Unknown
escapes are an error.

```
"a\nb"                    => "a\nb"
"a\\b"                    => "a\b"
```

### Multi-line strings

Consecutive lines beginning with `\\` form one string. Each line
contributes its text after the `\\`; lines join with `\n`.

```
\\hello
\\world                   => "hello\nworld"
```

### Character literals

Single-quoted Zig char literals: a single character, an escape (`'\n'`,
`'\r'`, `'\t'`, `'\\'`, `'\''`, `'\"'`, `'\0'`), a hex escape `'\xNN'`,
or a Unicode escape `'\u{...}'`. Default result is a one-character
string; with `CharAsNumber` set, the numeric code point as `float64`.

### Comments

`//` line comments only; discarded. (Hash `#` and block `/* */`
comments are disabled by the plugin.)

```
.{
  // a comment
  .name = "x", // trailing comment
}                         => map[string]any{"name": "x"}
```

## Tokens

| Token | Source | Meaning |
|---|---|---|
| `#OB` | `.{` | start of a struct (map) |
| `#OS` | `.{` | start of a tuple (list) |
| `#CB` | `}` | close of struct or tuple |
| `#CL` | `=` | key/value separator |
| `#TX` | `.ident` | field name (key) or enum literal (value) |
| `VAL` | — | number, string, `true`/`false`/`null`, or `.enum` |

`{`, `[`, `]` are not tokens — a bare `{` is a syntax error.

## Grammar group tag

Every grammar alternate the plugin adds carries the group tag `zon`.
Callers can switch the ZON alts off (restoring plain jsonic) via
`Options{Rule: &RuleOptions{Exclude: "zon"}}`:

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnaszon.Zon, tabnaszon.Defaults)
j.SetOptions(tabnasjsonic.Options{Rule: &tabnasjsonic.RuleOptions{Exclude: "zon"}})
```

## Errors

`Parse` and `Jsonic.Parse` return an `error` rather than panicking. The
error is jsonic's parse error, reporting an error code and the source
location (row, column, position). Inputs that are valid jsonic but not
valid ZON (such as a bare `{` opener) are errors. See the
[differences section](concepts.md#differences-from-the-ts-version) for
the few error-code divergences from TypeScript.
