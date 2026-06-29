# Tutorial — your first ZON parse

This walks you from nothing to a working parse, then through one
option and one error. Follow it in order; each step builds on the
last. When you finish you will have installed the plugin, parsed a
struct and a tuple, switched on an option, and handled a parse error.

For a recipe-style index of individual tasks, see the
[how-to guide](guide.md). For exhaustive signatures and the full
syntax, see the [reference](reference.md). For how it all works, see
[concepts](concepts.md).

## 1. Install

`@tabnas/zon` is a grammar plugin: it has no parser of its own. It runs
on the Tabnas engine, with the relaxed-JSON grammar from
`@tabnas/jsonic` underneath. Install all three:

```bash
npm install @tabnas/parser @tabnas/jsonic @tabnas/zon
```

`@tabnas/parser` (>= 2) and `@tabnas/jsonic` (>= 2) are peer
dependencies.

## 2. Build a parser

Create a Tabnas engine, layer the jsonic grammar onto it, then layer
the ZON plugin on top. The result is a reusable parser instance:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

j.parse('.{ .name = "Alice", .age = 30 }') // => { name: 'Alice', age: 30 }
```

You wrote Zig anonymous-struct syntax — `.{ ... }` to open, `.field`
for each key, `=` to assign — and got back a plain object. That is the
point: the plugin teaches the engine to read ZON.

## 3. Parse a tuple

The same `.{ ... }` opener also makes tuples. When the brace is *not*
immediately followed by `.field =`, the values inside become an array:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

j.parse('.{ 1, 2, 3 }')      // => [1, 2, 3]
j.parse('.{ "a", "b" }')     // => ['a', 'b']
```

The plugin decides struct-vs-tuple by peeking past the opening brace,
so you never have to mark which one you mean — just write it.

## 4. Nest and mix

Structs and tuples nest freely, and a struct can hold both:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

j.parse('.{ .xs = .{ 1, 2, 3 }, .y = .{ .z = true } }') // => { xs: [1, 2, 3], y: { z: true } }
```

This is the shape of a real `build.zig.zon` manifest: named fields,
some holding nested structs, some holding tuple-style path lists.

## 5. Turn on an option

The plugin is configured through its second `use()` argument. For
example, a Zig char literal like `'A'` is a one-character string by
default; set `charAsNumber: true` to get its code point instead:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon, { charAsNumber: true })

j.parse("'A'") // => 65
```

There are only two options, `charAsNumber` and `enumTag`; the
[reference](reference.md#options) lists both with their defaults.

## 6. Catch an error

ZON is not a superset of JSON. A bare `{` is not a valid opener — the
plugin removes it on purpose — so parsing one throws:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

let threw = false
try {
  j.parse('{ a = 1 }') // not ZON: bare { is rejected
} catch (e) {
  threw = true
}
threw // => true
```

The thrown error is the engine's standard parse error, with a code,
a source location, and a formatted message you can show a user.

## Where to go next

- [How-to guide](guide.md) — focused recipes for individual tasks.
- [Reference](reference.md) — the public API, every option, the full
  ZON syntax accepted.
- [Concepts](concepts.md) — how the plugin reshapes the engine, and
  why.
