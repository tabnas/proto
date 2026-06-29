# Concepts (Go)

Background on how the Go ZON plugin is put together, and why — plus a
section on how it differs from the TypeScript version. This is
understanding-oriented reading; for steps see the
[tutorial](tutorial.md) and [how-to guide](guide.md), and for exact
signatures and syntax see the [reference](reference.md).

## A grammar plugin on a shared engine

The plugin has no parser of its own. It is a thin layer on a stack of
two pieces:

- the **jsonic engine** (`github.com/tabnas/jsonic/go`) — a rule-based
  parser over a configurable, matcher-based lexer, carrying the
  relaxed-JSON grammar and its helper actions (`@array$`, the
  `val`/`map`/`list`/`pair`/`elem` rules), and
- **this plugin** (`github.com/tabnas/zon/go`) — the option overrides,
  custom lex matchers, and small grammar overlay that retune that stack
  to read Zig anonymous-struct syntax instead of JSON.

Because the engine is configuration-driven, ZON support is mostly an
options change plus a handful of alternates — not a new parser. The
plugin embeds the canonical grammar text (from the repo-root
`zon-grammar.jsonic`, kept in sync with the TypeScript source by the
build), parses it with a throwaway jsonic instance into a
`*tabnasjsonic.GrammarSpec`, attaches its `*tabnasjsonic.Options` overrides to that
spec, and applies the whole thing atomically via `j.Grammar(gs,
&tabnasjsonic.GrammarSetting{Rule: ...G: "zon"})`.

## ZON is not a superset of JSON

JSON and ZON share scalars but differ in structure:

| | JSON / jsonic | ZON |
|---|---|---|
| Open a map | `{` | `.{` (followed by `.field =`) |
| Open a list | `[` | `.{` (otherwise) |
| Close | `}` / `]` | `}` |
| Key/value separator | `:` | `=` |
| Keys | strings | `.identifier` |
| Strings | `"` `'` `` ` `` | `"` only |
| Comments | `#` `//` `/* */` | `//` only |

The plugin makes those swaps by **disabling** what JSON allows and
**adding** what ZON needs, rather than accepting both — so a
`build.zig.zon` file that accidentally used JSON braces is a clear
error, not a silent success.

## The four mechanisms

The plugin reshapes the stack with four cooperating mechanisms, all
applied together through one `GrammarSpec`:

1. **Custom lex matchers** own the `.`-prefixed and Zig-specific
   tokens, registered under `Options.Lex.Match` with high `Order`
   values so they run ahead of the fixed-token matcher:
   - `.{` peeks ahead and emits `#OB` (struct) when followed by
     `<ws>.ident<ws>=`, or `#OS` (tuple) otherwise.
   - `.identifier` emits `#TX` whose `Val` is the identifier with the
     dot stripped, and whose `Use["zonEnum"]` flag marks it for
     optional enum-tag wrapping.
   - `\\`-prefixed lines emit one `#ST` string token with the joined
     content.
   - char literals emit a `#NR` number token whose value is a one-char
     string or the code point (as `float64`), per `CharAsNumber`.

2. **Token remapping.** `#CL` is rebound from `:` to `=`; the default
   char mappings for `#OB`, `#OS`, and `#CS` are dropped to `nil`, so a
   stray `{`, `[`, or `]` is a syntax error. The default text matcher
   is turned off.

3. **Key-set restriction.** The `KEY` token set is narrowed to `#TX`
   alone, so only an identifier can sit on the left of `=`.

4. **Grammar overlay.** A few alternates are prepended to `val`,
   `list`, `elem`, and `pair`. They swap the list terminator from the
   default `#CS` to `#CB`, seed the list node with `@array$`, and
   accept a trailing comma before `}`.

The `Rule.Exclude = "jsonic,imp"` override removes jsonic's implicit
maps/lists, top-level commas, and path-dive extensions, and
`Rule.Start = "val"` makes a single value the entry rule.

## Struct vs tuple disambiguation

ZON uses one opener, `.{`, for both maps and lists. The parser allows
only two tokens of lookahead — not enough to tell a struct from a tuple
by grammar alone. So the decision is pushed into the lexer: when the
`.{` matcher fires (`peekIsMapOpen`), it scans past the brace,
whitespace, and `//` comments and checks for `.ident` followed by `=`.
If found, it emits `#OB` (struct); otherwise `#OS` (tuple). The grammar
only ever sees an already-classified open token. This is why `.{}`
parses as an **empty list**: with nothing inside, there is no
`.field =` to mark it as a struct.

## Enum literals: one token, two roles

A bare `.foo` token (`#TX`) is valid in two positions: before `=` it is
a key (field name `foo`); in value position it is an enum literal
(value `"foo"`). Because `#TX` belongs to both the `KEY` and `VAL`
token sets, the parser picks the right reading purely by context.

When `EnumTag` is set, an enum literal in value position is wrapped as
`map[string]any{EnumTag: name}`. jsonic's grammar already owns the
value-close phase via `@val-bc/replace`, and once a phase is "replaced"
the engine suppresses any `/prepend` on it. So the wrapping runs in the
*after-close* phase (`@val-ac`): a `StateAction` checks whether the
closed value came from a token carrying the `zonEnum` flag, and if so
rebuilds `r.Node` as the tagged map. Keys are unaffected.

## Why reuse one instance

Building the ZON grammar dominates the cost of a parse; the parse
itself is cheap. The default no-options `Parse` path therefore caches a
single instance behind a `sync.Once`, reusing it across calls (safe for
concurrent use, since a parse builds its own context and only reads
instance state). Option-taking calls build a dedicated instance, since
their configuration differs per call — use `MakeJsonic` once and reuse
it for a hot loop with fixed options. The repo's `perf_test.go` guards
the reuse win.

## Differences from the TS version

The TypeScript implementation is the reference; the Go module is a
faithful port built from the same `zon-grammar.jsonic`. The differences
do **not** change a successful parse's *structure* — they concern the
host language's API shape, value types, and a couple of error codes.

### API shape

| Area | TypeScript | Go |
|---|---|---|
| Convenience entry | none — install the plugin yourself | `tabnaszon.Parse(src, opts...)` and `tabnaszon.MakeJsonic(opts...)` |
| Build a parser | `new Tabnas().use(jsonic).use(Zon, opts)` | `tabnaszon.MakeJsonic(opts)` or `j.UseDefaults(tabnaszon.Zon, tabnaszon.Defaults, m)` |
| Options | one object `{ charAsNumber, enumTag }` | `ZonOptions{ CharAsNumber *bool, EnumTag string }`, or a `map[string]any` |
| "Omit vs set" | option present or absent | `*bool` nil vs set; `EnumTag == ""` means unset |
| Parse failure | **throws** | returns `error`; never panics on parse errors |

The Go side adds the `Parse` / `MakeJsonic` convenience helpers because
Go has no fluent `.use()` chain; the TypeScript side has no such
helpers (you build the engine yourself with `.use(jsonic).use(Zon)`).

### Value types

TypeScript returns untyped `any` JavaScript values; Go returns `any`
with predictable concrete types:

| Value | TypeScript | Go |
|---|---|---|
| Struct | object (null-prototype) | `map[string]any` |
| Tuple / empty | array | `[]any` |
| Number (all bases, float, char-as-number) | `number` | `float64` |
| String / enum / char-as-string | `string` | `string` |
| Boolean | `boolean` | `bool` |
| Null | `null` | `nil` |
| Tagged enum | `{ [tag]: name }` | `map[string]any{tag: name}` |

The most visible consequence: ZON integers like `42` come back as the
JavaScript number `42` in TypeScript and as `float64(42)` in Go — Go
has no separate integer type in the result tree.

### Error codes

A successful parse is identical across runtimes, but (inheriting
jsonic's documented divergences) a few *failing* inputs map to
different error **codes** between the two — for example a raw control
character inside a double-quoted string reports `unprintable` in
TypeScript and `unterminated_string` in Go. Both report the failure at
the same row/column; only the `Code` differs. If you branch on the
error code, account for this. See the jsonic Go
[differences reference](../../../jsonic/go/doc/differences.md) for the
full list.

## Accepted vs rejected — edge cases

- `.{}` → `[]any{}`. An empty literal is a list, not a map.
- `{ a = 1 }` → **error** (returned, not panicked). Bare `{` is not a
  ZON opener.
- `'A'` → `"A"` by default, `float64(65)` with `CharAsNumber` set.
- `"a\\b"` → `"a\b"`. Double quotes only, with Zig escapes; unknown
  escapes are an error.
- `.red` as a value → `"red"`, or `map[string]any{tag: "red"}` with
  `EnumTag`.
- `.red` as a key (`.red = 1`) → key `red`; `EnumTag` never applies to
  keys.
- Trailing comma before `}` → accepted in both structs and tuples.
- `//` comment → discarded; `#` and `/* */` are not comments in ZON.
