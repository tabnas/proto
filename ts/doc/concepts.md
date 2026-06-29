# Concepts

Background on how the ZON plugin is put together, and why. This is
understanding-oriented reading — for steps see the
[tutorial](tutorial.md) and [how-to guide](guide.md), and for exact
signatures and syntax see the [reference](reference.md).

## A grammar plugin on a shared engine

The plugin has no parser of its own. It is a thin layer on a stack of
three pieces:

- the **Tabnas engine** (`@tabnas/parser`) — a rule-based parser over a
  configurable, matcher-based lexer,
- the **relaxed-JSON grammar** (`@tabnas/jsonic`) — the rules and
  helper actions (`@array$`, the `val`/`map`/`list`/`pair`/`elem` rule
  set) that turn tokens into objects and arrays, and
- **this plugin** (`@tabnas/zon`) — the option overrides, custom lex
  matchers, and small grammar overlay that retune that stack to read
  Zig anonymous-struct syntax instead of JSON.

Because the engine is configuration-driven, ZON support is mostly an
options change plus a handful of alternates — not a new parser. The
plugin embeds the canonical grammar text (from the repo-root
`zon-grammar.jsonic`) as a string, parses it with a throwaway jsonic
instance to get a grammar object, attaches its option overrides to that
object, and hands the whole thing to the engine atomically via
`tn.grammar(grammarDef, { rule: { alt: { g: 'zon' } } })`.

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
**adding** what ZON needs, rather than accepting both. That is a
deliberate choice: a `build.zig.zon` file that accidentally used JSON
braces should be a clear error, not a silent success.

## The four mechanisms

The plugin reshapes the stack with four cooperating mechanisms, all
applied together through one `GrammarSpec`:

1. **Custom lex matchers** own the `.`-prefixed and Zig-specific
   tokens. They run ahead of the fixed-token matcher (high `order`
   values) so they reliably claim their input:
   - `.{` peeks ahead and emits `#OB` (struct) when followed by
     `<ws>.ident<ws>=`, or `#OS` (tuple) otherwise.
   - `.identifier` emits `#TX` whose `val` is the identifier with the
     dot stripped, and whose `use.zonEnum` flag marks it for optional
     enum-tag wrapping.
   - `\\`-prefixed lines emit one `#ST` string token with the joined
     content.
   - char literals (`'x'`, `'\n'`, `'\xNN'`, `'\u{...}'`) emit a `#NR`
     number token whose value is a one-char string or the code point,
     per `charAsNumber`.

2. **Token remapping.** `#CL` is rebound from `:` to `=`; the default
   char mappings for `#OB`, `#OS`, and `#CS` are dropped to `null`, so
   a stray `{`, `[`, or `]` produces a syntax error instead of silently
   opening a structure. The default jsonic text matcher is turned off,
   since identifiers only ever appear as `.ident` and are owned by the
   custom matcher.

3. **Key-set restriction.** The `KEY` token set is narrowed to `#TX`
   alone, so only an identifier (not a number or a quoted string) can
   sit on the left of `=`.

4. **Grammar overlay.** A few alternates are prepended to `val`,
   `list`, `elem`, and `pair`. They swap the list terminator from the
   default `#CS` to `#CB`, seed the list node with `@array$`, and
   accept a trailing comma before `}`. This is the only part written in
   grammar text; everything else is options.

The `{ rule: { exclude: 'jsonic,imp' } }` override also removes
jsonic's implicit maps/lists, top-level commas, and path-dive
extensions, and `{ rule: { start: 'val' } }` makes a single value the
entry rule.

## Struct vs tuple disambiguation

ZON uses one opener, `.{`, for both maps and lists. The engine's parser
allows only two tokens of lookahead, which is not enough to tell a
struct from a tuple by grammar alone (you would have to see an
arbitrary distance ahead to find the first `=`).

So the decision is pushed down into the lexer. When the `.{` matcher
fires, it scans past the opening brace, whitespace, and `//` comments,
then checks for `.ident` followed by `=`. If found, it emits `#OB`
(struct); otherwise `#OS` (tuple). The grammar therefore only ever sees
an already-classified open token, and a two-token-lookahead rule set is
enough. This is why `.{}` parses as an **empty list** rather than an
empty map: with nothing inside, there is no `.field =` to mark it as a
struct.

## Enum literals: one token, two roles

A bare `.foo` token (`#TX`) is valid in two positions. Before `=` it is
a key (the field name `foo`); in value position it is an enum literal
(the value `'foo'`). Because `#TX` is a member of both the `KEY` and
`VAL` token sets, the parser picks the right interpretation purely by
context — no grammar branching is needed.

When `enumTag` is set, an enum literal in value position must be
wrapped as `{ [enumTag]: name }`. The relaxed-JSON grammar already owns
the value-close phase via `@val-bc/replace`, and once a phase is
"replaced" the engine suppresses any `/prepend` on it. So the wrapping
runs in the *after-close* phase (`@val-ac`): it checks whether the
closed value came from a token carrying the `zonEnum` flag, and if so
rebuilds the node as the tagged object. Keys are unaffected, because
they are consumed in key position, not as values.

## Why reuse one instance

Building the ZON grammar — parsing the embedded grammar text, applying
the option overlay, wiring the custom matchers — dominates the cost of
a parse; the parse itself, on a typical small ZON value, is cheap by
comparison. The instance is stateless across parses (each parse builds
its own context and only reads instance state), so the right pattern is
to build the engine once and reuse it for every input. The repo's
performance test guards exactly this: reuse stays linear, and the
rebuild-per-parse anti-pattern is many times slower.

## Accepted vs rejected — edge cases

- `.{}` → `[]`. An empty literal is a list, not a map.
- `{ a = 1 }` → **error**. Bare `{` is not a ZON opener; it was
  removed.
- `'A'` → `'A'` by default, `65` with `charAsNumber: true`. The single
  quote is a char literal, not a string delimiter.
- `"a\\b"` → `'a\b'`. Double quotes are the only string delimiter, with
  Zig escapes; an unknown escape is an error.
- `.red` as a value → `'red'`, or `{ tag: 'red' }` with `enumTag`.
- `.red` as a key (`.red = 1`) → key `red`; `enumTag` never applies to
  keys.
- Trailing comma before `}` → accepted in both structs and tuples.
- `//` comment → discarded; `#` and `/* */` are **not** comments in
  ZON.

## Relationship to the Go port

The plugin ships in two implementations — this TypeScript one and a Go
port — built from the same canonical `zon-grammar.jsonic`. The
TypeScript version is the reference. For the Go API shape, value types,
and any accepted differences, see
[../../go/doc/concepts.md](../../go/doc/concepts.md).
