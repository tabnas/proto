# Tutorial — your first ZON parse (Go)

This walks you from nothing to a working parse, then through one option
and one error. Follow it in order; each step builds on the last. When
you finish you will have installed the module, parsed a struct and a
tuple, switched on an option, and handled a parse error.

For a recipe-style index of individual tasks, see the
[how-to guide](guide.md). For exhaustive signatures and the full
syntax, see the [reference](reference.md). For how it all works — and
how the Go version differs from TypeScript — see
[concepts](concepts.md).

## 1. Install

`zon` is a jsonic plugin. The convenience helpers pull in the jsonic
engine for you, so a single `go get` is enough:

```bash
go get github.com/tabnas/zon/go@latest
```

```go
import tabnaszon "github.com/tabnas/zon/go"
```

## 2. Parse a struct

`tabnaszon.Parse` is the one-call entry point. Give it ZON source and it
returns the parsed value as `any` plus an `error`:

```go
result, err := tabnaszon.Parse(`.{ .name = "Alice", .age = 30 }`)
// result: map[string]any{"name": "Alice", "age": float64(30)}
// err:    nil
```

You wrote Zig anonymous-struct syntax — `.{ ... }` to open, `.field`
for each key, `=` to assign — and got back a `map[string]any`. Note
that numbers come back as `float64`; that is the only numeric type ZON
produces.

## 3. Parse a tuple

The same `.{ ... }` opener also makes tuples. When the brace is *not*
immediately followed by `.field =`, the values inside become a slice:

```go
result, err := tabnaszon.Parse(`.{ 1, 2, 3 }`)
// result: []any{float64(1), float64(2), float64(3)}

result, err = tabnaszon.Parse(`.{ "a", "b" }`)
// result: []any{"a", "b"}
```

The plugin decides struct-vs-tuple by peeking past the opening brace,
so you never mark which one you mean — just write it.

## 4. Nest and mix

Structs and tuples nest freely, and a struct can hold both:

```go
result, err := tabnaszon.Parse(`.{ .xs = .{ 1, 2, 3 }, .y = .{ .z = true } }`)
// result: map[string]any{
//   "xs": []any{float64(1), float64(2), float64(3)},
//   "y":  map[string]any{"z": true},
// }
```

This is the shape of a real `build.zig.zon` manifest: named fields,
some holding nested structs, some holding tuple-style path lists.

## 5. Turn on an option

Options are passed as a `tabnaszon.ZonOptions` value after the source. For
example, a Zig char literal like `'A'` is a one-character string by
default; set `CharAsNumber` to get its code point (a `float64`)
instead:

```go
charAsNum := true
result, err := tabnaszon.Parse(`'A'`, tabnaszon.ZonOptions{CharAsNumber: &charAsNum})
// result: float64(65)
```

`CharAsNumber` is a `*bool` so you can express "leave it at the
default" (nil) versus "set it". There are only two options,
`CharAsNumber` and `EnumTag`; the [reference](reference.md#options)
lists both.

## 6. Handle an error

ZON is not a superset of JSON. A bare `{` is not a valid opener — the
plugin removes it on purpose — so parsing one returns an error rather
than panicking:

```go
result, err := tabnaszon.Parse(`{ a = 1 }`) // not ZON: bare { is rejected
// result: nil
// err:    non-nil parse error
if err != nil {
    // handle the syntax error
}
```

Go never panics on a parse error; always check the returned `error`.

## Where to go next

- [How-to guide](guide.md) — focused recipes for individual tasks.
- [Reference](reference.md) — the public API, every option, the full
  ZON syntax accepted.
- [Concepts](concepts.md) — how the plugin reshapes the engine, and
  how the Go version differs from TypeScript.
