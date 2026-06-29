# zon (Go)

A jsonic grammar plugin that parses
[Zig Object Notation (ZON)](https://ziglang.org/documentation/master/#ZON)
into Go values. ZON is the anonymous-struct data format used for Zig
`build.zig.zon` manifests.

## Install

```bash
go get github.com/tabnas/zon/go@latest
```

```go
import tabnaszon "github.com/tabnas/zon/go"
```

## One example

`tabnaszon.Parse` is the one-call entry point — pass source, get a value and
an `error`:

```go
result, err := tabnaszon.Parse(`.{ .name = "Alice", .age = 30 }`)
// result: map[string]any{"name": "Alice", "age": float64(30)}

result, err = tabnaszon.Parse(`.{ 1, 2, 3 }`)
// result: []any{float64(1), float64(2), float64(3)}
```

Numbers come back as `float64`. The no-options `Parse` path reuses a
cached instance internally and is safe for concurrent use; for hot
loops with options, build one instance with `tabnaszon.MakeJsonic` and reuse
it.

## Documentation

Full documentation follows the [Diátaxis](https://diataxis.fr)
framework:

- [Tutorial](doc/tutorial.md) — a guided first parse, start to finish.
- [How-to guide](doc/guide.md) — short recipes for individual tasks.
- [Reference](doc/reference.md) — the public API, every option, and the
  complete ZON syntax accepted.
- [Concepts](doc/concepts.md) — how the plugin reshapes the engine, and
  how the Go version differs from TypeScript.

For the canonical TypeScript implementation, see
[`../ts/README.md`](../ts/README.md).

## Grammar

The grammar is defined once in the top-level
[`zon-grammar.jsonic`](../zon-grammar.jsonic) and embedded into this Go
source ([`zon.go`](zon.go)) and the TypeScript source during the build.
Edit the grammar there, not in the generated source.

A railroad/syntax diagram of the grammar is in
[`../ts/doc/grammar.svg`](../ts/doc/grammar.svg) (ASCII version:
[`../ts/doc/grammar.txt`](../ts/doc/grammar.txt)).

## License

Copyright (c) 2025 Richard Rodger and other contributors, MIT License.
