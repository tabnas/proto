# DIVERGENCE

Where the ports of `@tabnas/proto` disagree, and why.

TypeScript (`ts/`) is canonical. Go (`go/`) and Rust (`rs/`) track it, and
the shared fixtures in [`test/spec`](test/spec) plus protoc's own parser
corpus in [`test/protobuf-suite`](test/protobuf-suite) hold all three to
the same answers everywhere else.

**Every entry here is MEASURED and EXECUTED, in every runtime.** The rows
live in [`test/divergent.tsv`](test/divergent.tsv), one cell per runtime,
and all three cells now run: `ts/test/divergent.test.ts` reads the `ts`
column, `go/divergent_test.go` the `go` column and
`rs/tests/divergent_test.rs` the `rust` column, each through its half of
the shared `Register`. It fails when a port stops doing what its cell
says AND when a divergence is repaired, so a row cannot outlive the thing
it records. A divergence a fixture row cannot express is pinned by a named
Rust test instead, and says so.

Until 2026-09-22 only the Rust column ran, and the other two were
measured by hand and written down. A recorded measurement is prose with
numbers in it: it goes stale the moment a port moves, and nothing says
so. All three columns are executed now, which is what ADR-14 asks for.

There is no prose-only claim in this file, by construction.

## 1. A field type named after an `Object.prototype` member

**Rust and Go record the type name; TypeScript loses it.**

`ts/src/build-descriptor.ts` looks a field's type up in `SCALAR_TYPES`,
which is an object literal, with text taken from the source:

```ts
const scalar = SCALAR_TYPES[bare]
if (scalar) return { type: scalar }
```

A bare object literal inherits from `Object.prototype`, so a lookup of
`__proto__` or of any of the eleven function-valued members finds an
inherited value rather than `undefined`. The truthiness test then passes
and the field gets a `type` it should not have, and no `typeName` at all.
`__proto__` yields the prototype object, which `JSON.stringify` writes as
`{}`; the others yield a function, which `JSON.stringify` DROPS, so the
field ends up with neither key.

Measured, on the field object alone:

| input | TypeScript | Go | Rust |
|---|---|---|---|
| `optional __proto__ a = 1;` | `"type":{}` | `"typeName":"__proto__"` | `"typeName":"__proto__"` |
| `optional .__proto__ a = 1;` | `"type":{}` | `"typeName":".__proto__"` | `"typeName":".__proto__"` |
| `optional constructor a = 1;` | neither key | `"typeName":"constructor"` | `"typeName":"constructor"` |
| `optional toString a = 1;` | neither key | `"typeName":"toString"` | `"typeName":"toString"` |

The other eight prototype members (`hasOwnProperty`, `isPrototypeOf`,
`propertyIsEnumerable`, `toLocaleString`, `valueOf`, `__defineGetter__`,
`__defineSetter__`, `__lookupGetter__`, `__lookupSetter__`) behave like
`toString`.

What protoc's parser records here is the `typeName`, as written, which is
what Go and Rust produce. Rust cannot reproduce the TypeScript answer
without modelling `Object.prototype` in the descriptor type, and would not
want to.

**Owner: TypeScript.** The repair is a `Map`, or an own-property check, in
`ts/src/build-descriptor.ts` `fieldTypeName`. When it lands, the register
rows go red and name themselves for deletion.

**Executed:** `test/divergent.tsv`, rows 1 and 2.

## 2. Nesting: every runtime caps the document; Rust also bounds its walk

**All three refuse a document nesting deeper than 100 levels, before the
engine runs, with the same message. Rust alone also refuses such a tree
when a caller hands one to its walk directly.**

Each runtime counts the document's braces, skipping string literals and
the comments the lexer skips, and refuses past `MAX_NESTING_DEPTH`
(`MaxNestingDepth` in Go):

```
proto: document nests 101 levels deep, past the 100 this parser accepts
```

Like every refusal the plugin makes on its own account, it carries no
code. `parse` runs the check in every runtime (`Parse` in Go, and Rust's
`parse_with` too), and each exports it alone, `preflight` in TypeScript
and Rust and `Preflight` in Go, for a caller that drives the engine and
wants the CST. The scan is the same in all three: `ts/src/preflight.ts`
and `go/preflight.go` port `brace_depth` from `rs/src/build_descriptor.rs`.

Until 2026-10-10 the cap was Rust's alone, and TypeScript and Go parsed
any depth. Every runtime needs it, each for its own reason. A CST node's
`src` holds every token beneath it, so the text the engine builds grows
with the square of the nesting depth, and so do time and memory.
Measured on 2026-10-10 on the paths with no check: the engine's own
parse, then the walk.

| nesting depth | TypeScript | Go | Rust, uncapped, 1 MiB stack, debug |
|---|---|---|---|
| 300 | parses | parses | **process abort** |
| 1,000 | parses, 0.3 s | parses, 0.13 s, 33 MB allocated | **process abort** |
| 3,000 | `RangeError` after 0.7 s | parses, 0.7 s, 187 MB allocated | **process abort** |
| 10,000 | `RangeError` after 3.7 s, 553 MB resident | not run | **process abort** |

Measured through alchemy-cli the same day: Go's tree for a 330 KB
document nesting 30,000 levels passed 13 GB, and the process was killed;
TypeScript reached its `RangeError` only after 41 s at 100,000 levels. A
Rust stack that runs out ABORTS the process, and the abort cannot be
caught, logged or recovered from.

The number is Rust's, and measured. On the smallest stack a caller is
likely to have, the 1 MiB a spawned `std::thread` gets by default, a
debug build of the uncapped Rust walk parsed 290 levels and aborted at
300; on the 2 MiB a released binary's spawned thread gets, between 600
and 620. The cap of 100 leaves roughly a threefold margin on the smaller
of those, matches the recursion budget protoc's own parser carries, and
is more than an order of magnitude past anything a hand-written `.proto`
nests.

**What stays Rust's alone: the walk refuses a deep tree.** A caller who
drives the engine and skips the check can still build a CST past the
cap. Rust's `to_descriptor` and `build_file` refuse it (`proto: document
nests deeper than 100 levels`), because the descriptor walk recurses and
so do `tabnas::Value`'s drop and its JSON rendering. TypeScript's
`toDescriptor` and Go's `ToDescriptor` and `BuildFile` walk the tree
whole, as such a caller asked. That refusal does not protect the Rust
caller either, since the tree exists by then and a deep enough
`tabnas::Value` aborts as it drops; the check on the source is the
protection, in every runtime.

**Owner: Rust, permanently,** for the walk's bound: it is a property of
the language's stack discipline, not a defect to repair.

**Executed:** `test/spec/nesting.tsv`, in every runtime: the cap itself,
one level past it, an unclosed pile of messages, braces in an aggregate
option value, which count, and the braces a string literal or a comment
holds, which do not. `rs/tests/untrusted_test.rs`,
`ts/test/preflight.test.ts` and `go/preflight_test.go` add the depth
under the cap, the missing code, and the walk: each runtime's test hands
its walk a tree one level past the cap, which Rust refuses and the other
two walk.

## 3. Divergences this repository records that are NOT Rust's

The register carries seven more rows, where Rust agrees with the canonical
TypeScript and **Go** does not. They are here so the file and the register
say the same thing, and because a reader looking for "where do the ports
disagree" should find all of it in one place.

| input | TypeScript | Go | Rust |
|---|---|---|---|
| `int32 a = 1 [__proto__ = 2];` | no `options` | `"options":{"__proto__":2}` | no `options` |
| `optional int32 a = 1 [x = -0x10];` | `"x":"-0x10"` | `"x":-16` | `"x":"-0x10"` |
| `optional int32 a = 1_0;` | `"number":null` | `"number":10` | `"number":null` |
| proto3 `optional group G = 1 { }` | `proto3Optional`, `_g` oneof | neither | `proto3Optional`, `_g` oneof |
| `optional string s = 1 [default = ""];` | `"defaultValue":""` | no `defaultValue` | `"defaultValue":""` |
| `optional string s = 1 [json_name = ""];` | `"jsonName":""` | no `jsonName` | `"jsonName":""` |
| `reserved 0x10;` | `"reservedRange":[]` | no `reservedRange` | `"reservedRange":[]` |

Why each one is what it is:

1. **A field option named `__proto__` is dropped.** The canonical
   `map[name] = value` writes to a bare object, and assigning to
   `__proto__` there sets the prototype rather than creating a property;
   with a primitive value, as every option value is, it is silently
   discarded. `rs/src/build_descriptor.rs` `read_field_options` drops it
   deliberately, with the reason written at the line. A Go map keeps it.

   An `option __proto__ = ...;` STATEMENT is a different path in all
   three and survives everywhere: the canonical form there is a
   computed-key object literal, which does create an own property.

2. **`Number("-0x10")` is `NaN`.** ECMA-262 allows no sign before a
   NonDecimalIntegerLiteral, so the walk falls through and keeps the
   literal text. Go's `jsNumber` strips the sign before choosing a base
   and answers -16.

3. **`Number("1_0")` is `NaN`.** A numeric separator is a source-literal
   feature and not part of StringToNumber, so the field number is `NaN`
   and the descriptor's JSON writes `null`. Go's `toInt` answers 10.
   (`1_0` is accepted at all only because the shared tabnas lexer is more
   permissive than `.proto` here; `test/protobuf-suite/leniency.json`
   records that.)

4. **A proto3 `optional group` is proto3-optional.** The canonical
   `buildGroup` spreads the whole of `fieldLabel`, which carries the
   flag, and `generateSyntheticOneofs` then adds the `_g` oneof. Go's
   `buildGroup` discards the flag with `lbl, _ := fieldLabel(...)`.

5. **An empty `default` is a value.** The canonical walk keeps
   `default = ""` as an empty `defaultValue`, as protoc's parser keeps an
   empty `default_value`. Go's `DefaultValue` is a plain string tagged
   `omitempty`, so an empty one cannot be told from none and is dropped.

6. **An empty `json_name` is a value**, the same way, through Go's
   `JsonName`.

7. **A `reserved` statement whose ranges all fail to read still makes a
   list.** The canonical walk creates `reservedRange` for any `reserved`
   statement with ranges, and when none of them reads it stays `[]`. Go
   appends nothing to a nil slice, and `omitempty` drops it. (No runtime
   reads `0x10` as a range at all, where protoc reads 16; that defect is
   shared, so it is not a divergence.)

These three were found while building the descriptor tree, by probing
for members Go's descriptor cannot hold; no shared fixture row reaches
them. `DescriptorValue` gives the members Go's descriptor has, so the tree
carries them too.

**Owner: Go**, for all seven.

**Executed:** `test/divergent.tsv`, rows 3 to 9.

## What is NOT a divergence

Recorded here because each was measured and found equal, and a later
reader should not have to measure it again.

- **The member order of the descriptor as a tree.** The canonical
  descriptor is an object whose members come in the order the walk first
  assigns them, and a few follow the statements: a file's `package`,
  `optionDependency` and `options`, a message's `extensionRange`,
  `reservedRange` and `reservedName`, an enum's `reservedRange`,
  `reservedName` and `options`, and the names in every option map. Rust's
  `descriptor_value` and Go's `DescriptorValue` give the same members in
  the same order, from an order the walk records, and
  `rs/tests/value_test.rs` and `go/value_test.go` hold each to the
  canonical JSON byte for byte over every shared fixture row that has a
  descriptor (232), with `test/spec/member-order.tsv` covering every
  statement-ordered member. The structs' own serializations still follow
  their field declarations, and Go's sorts option names; the shared
  runner compares after a JSON round trip, which ignores order, so that
  is no parity claim.

- **The spelling of a number in the descriptor's JSON.** Rust does not use
  its own float formatter: `rs/src/jsnum.rs` `js_number_to_string` is
  ECMA-262 6.1.6.1.20, copied from `csv/rs`, and the descriptor hands its
  text to `serde_json` verbatim. `rs/tests/jsnum_test.rs` grades it
  against node over 60,366 doubles, hash for hash, including the
  fixed-to-exponent thresholds at 1e21 and 1e-7 and the midpoints where
  the specification takes the even digit.
- **The cost of a rule with many elements.** Rust's parse is quadratic in
  the number of fields in one message where TypeScript's is linear: 400
  fields cost 10.1s here against 0.129s there, on the debug profile.
  Every ANSWER is the same, so it is not a divergence; the cause is
  `Rule::accept_child_node` in the engine, and `rs/AGENTS.md` records the
  curve, the diagnosis and why no test pins it.
- **An rpc named `Stream`, or `stream`.** Accepted, in every runtime:
  keywords are case-sensitive in the grammar and every keyword is an
  identifier where protoc admits one. `test/spec/keywords.tsv` pins the
  keyword-as-identifier inputs in all three runtimes, and
  `rs/tests/proto_test.rs` `an_rpc_named_after_a_keyword_parses_in_every_runtime`
  keeps the name and the modifier told apart. (Before the grammar admitted
  keywords, both names were refused in every runtime, and the old pin
  said so.)
- **Everything else.** A differential run over 450 inputs (every shared
  fixture row, the whole of protoc's `valid`, `accept-only` and `invalid`
  lanes, the leniency probes and 90 hand-written edge cases aimed at the
  hand-rolled scanners, the option-name reader, the range and
  reserved-name lists, the number reader and the version options) found
  TypeScript and Rust agreeing on **every** accept-or-reject decision,
  and producing **byte-identical JSON** for 446 of them. The four that
  differ are exactly the cases in section 1.
