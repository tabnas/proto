# How-to guide

Short, task-focused recipes. Each is self-contained and assumes you
have the plugin installed (see the [tutorial](tutorial.md) for the
basics). For the full API, every option, and the complete syntax,
follow the links into the [reference](reference.md).

Every recipe starts from the same three imports:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'
```

## Use it as a plugin

`Zon` is a plugin, not a standalone parser. Layer it onto a Tabnas
engine that already has the jsonic grammar, then call `.parse()`:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

j.parse('.{ .a = 1, .b = 2 }') // => { a: 1, b: 2 }
```

The instance is reusable — build it once and call `.parse()` as many
times as you like. (Building the grammar is the expensive part; do not
reconstruct the instance per parse.)

## Parse a realistic build.zig.zon

A ZON manifest mixes named struct fields with tuple-style `paths`
lists and allows trailing commas and `//` line comments:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

const manifest = j.parse(`.{
    .name = "example",
    .version = "0.0.1",
    .minimum_zig_version = "0.14.0",
    .dependencies = .{
        .foo = .{
            .url = "https://example.com/foo.tar.gz",
            .hash = "1220deadbeef",
        },
    },
    .paths = .{
        "build.zig",
        "src",
    },
}`)

manifest // => { name: 'example', version: '0.0.1', minimum_zig_version: '0.14.0', dependencies: { foo: { url: 'https://example.com/foo.tar.gz', hash: '1220deadbeef' } }, paths: ['build.zig', 'src'] }
```

## Parse numbers in every ZON base

Numbers accept decimal, hex, octal, binary, floats, and `_` digit
separators:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

j.parse('0x2a')      // => 42
j.parse('0o52')      // => 42
j.parse('0b101010')  // => 42
j.parse('1_000_000') // => 1000000
j.parse('3.14')      // => 3.14
```

## Parse character literals as code points

By default Zig char literals (`'A'`, `'\n'`, `'\u{1F600}'`) parse as
one-character strings. Set `charAsNumber: true` to receive numeric
code points instead:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon, { charAsNumber: true })

j.parse("'A'")          // => 65
j.parse("'\\n'")        // => 10
j.parse("'\\u{1F600}'") // => 128512
```

## Tag enum literals to tell them apart from strings

Without options, an enum-literal value like `.red` becomes the plain
string `'red'` — indistinguishable from `"red"` in the parsed tree.
Set `enumTag` to wrap each enum value in a one-key object so you can
tell which was which:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon, { enumTag: '$enum' })

j.parse('.{ .kind = .red, .label = "red" }') // => { kind: { $enum: 'red' }, label: 'red' }
```

The tag name is yours to choose — use whatever key your consumers
expect.

## Read multi-line Zig strings

Consecutive lines prefixed with `\\` become a single string, joined
with `\n` (the `\\` prefix is stripped from each line):

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

const doc = j.parse(`.{
  .description =
    \\\\first line
    \\\\second line
  ,
}`)

doc // => { description: 'first line\nsecond line' }
```

## Handle a parse error

ZON deliberately rejects non-ZON input — a bare `{` opener, for
instance. A failed parse throws the engine's parse error; catch it and
read its fields:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon)

let threw = false
try {
  j.parse('{ a = 1 }') // not ZON: bare { is rejected
} catch (err) {
  threw = true
  // err.code, err.row, err.col, err.message are available here.
}
threw // => true
```

## Re-enable strict JSON while the plugin is loaded

Every grammar alternate the plugin adds carries the group tag `zon`.
To switch those alts off — restoring the plain jsonic grammar while
the plugin stays registered — exclude that tag:

```typescript
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '@tabnas/zon'

const j = new Tabnas().use(jsonic).use(Zon).options({
  rule: { exclude: 'zon' },
})
```

This is rarely useful — you would normally just not load the plugin —
but it is the supported way to peel the ZON layer back off.
