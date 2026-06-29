# @tabnas/zon

A grammar plugin that teaches the [Tabnas](https://github.com/tabnas/parser)
parser to read [Zig Object Notation (ZON)](https://ziglang.org/documentation/master/#ZON) —
the anonymous-struct data format used for `build.zig.zon` manifests.
Available for both TypeScript and Go, built on the same grammar.

ZON looks like this:

```zon
.{
    .name = "example",
    .version = "0.0.1",
    .dependencies = .{
        .foo = .{ .url = "https://example.com/foo.tar.gz", .hash = "1220deadbeef" },
    },
    .paths = .{ "build.zig", "src" },
}
```

## Install

```bash
# TypeScript / JavaScript
npm install @tabnas/parser @tabnas/jsonic @tabnas/zon

# Go
go get github.com/tabnas/zon/go@latest
```

## One tiny example

**TypeScript** — the plugin layers onto a Tabnas engine:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

j.parse('.{ .name = "Alice", .age = 30 }') // => { name: 'Alice', age: 30 }
j.parse('.{ 1, 2, 3 }')                     // => [1, 2, 3]
```

**Go** — `tabnaszon.Parse` is the one-call entry point:

```go
import tabnaszon "github.com/tabnas/zon/go"

result, _ := tabnaszon.Parse(`.{ .name = "Alice", .age = 30 }`)
// map[string]any{"name": "Alice", "age": float64(30)}
```

## Documentation

Full documentation follows the [Diátaxis](https://diataxis.fr)
framework — one file per quadrant, per language:

| | TypeScript | Go |
|---|---|---|
| **Tutorial** (learning) | [ts/doc/tutorial.md](ts/doc/tutorial.md) | [go/doc/tutorial.md](go/doc/tutorial.md) |
| **How-to guide** (tasks) | [ts/doc/guide.md](ts/doc/guide.md) | [go/doc/guide.md](go/doc/guide.md) |
| **Reference** (API + options + syntax) | [ts/doc/reference.md](ts/doc/reference.md) | [go/doc/reference.md](go/doc/reference.md) |
| **Concepts** (explanation) | [ts/doc/concepts.md](ts/doc/concepts.md) | [go/doc/concepts.md](go/doc/concepts.md) |

Per-language hubs: [`ts/README.md`](ts/README.md),
[`go/README.md`](go/README.md).

## Grammar diagram

The grammar is defined once in the top-level
[`zon-grammar.jsonic`](zon-grammar.jsonic) and embedded into both
implementations — TypeScript ([`ts/src/zon.ts`](ts/src/zon.ts)) and Go
([`go/zon.go`](go/zon.go)) — by [`ts/embed-grammar.js`](ts/embed-grammar.js)
during the TypeScript build. Edit the grammar there, not in the
generated sources.

As a railroad/syntax diagram, generated from the live grammar with
[`@tabnas/railroad`](https://github.com/tabnas/railroad):

![zon grammar railroad diagram](ts/doc/grammar.svg)

ASCII version: [`ts/doc/grammar.txt`](ts/doc/grammar.txt).

## License

MIT. Copyright (c) Richard Rodger.
