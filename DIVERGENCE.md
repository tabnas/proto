# DIVERGENCE

Where the ports of `@tabnas/proto` disagree, and why.

TypeScript (`ts/`) is canonical. Go (`go/`) and Rust (`rs/`) track it, and
the shared fixtures in [`test/spec`](test/spec) plus protoc's own parser
corpus in [`test/protobuf-suite`](test/protobuf-suite) hold all three to
the same answers everywhere else.

**Every entry here is MEASURED and EXECUTED.** The measurements were taken
on 2026-09-21 against this checkout: TypeScript through Node 22 type
stripping over `ts/src`, Go through `go test`, Rust through
`cargo test`. The rows live in [`test/divergent.tsv`](test/divergent.tsv)
and run from `rs/tests/divergent_test.rs` through
`tabnas_support::Register`, which fails when a port stops doing what a row
says AND when a divergence is repaired, so a row cannot outlive the thing
it records. A divergence a fixture row cannot express is pinned by a named
Rust test instead, and says so.

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

## 2. Nesting is capped in Rust

**Rust refuses a document nesting deeper than 100 messages; TypeScript and
Go accept it.**

The descriptor walk recurses, `tabnas::Value`'s default `Drop` recurses,
and `Value::to_json` recurses. A JavaScript stack overflow is a catchable
`RangeError` and a Go goroutine stack grows; a Rust stack that runs out
ABORTS the process, and the abort cannot be caught, logged or recovered
from. A port that can build an unbounded tree therefore has to bound it.

`rs/src/build_descriptor.rs` `MAX_NESTING_DEPTH` is 100. `parse` counts
braces outside strings and comments and refuses past that BEFORE the
engine builds a tree that deep; `build_file` refuses a CST it is handed
directly.

Measured, on the smallest stack a caller is likely to have, the 1 MiB a
spawned `std::thread` gets by default, with a debug build:

| nesting depth | Rust, uncapped | Rust, capped | TypeScript |
|---|---|---|---|
| 100 | parses | parses | parses |
| 290 | parses | refused | parses |
| 300 | **process abort** | refused | parses |
| 1000 | **process abort** | refused | parses |
| 5000 | **process abort** | refused | `RangeError`, catchable |

The last row is the difference stated exactly: TypeScript runs out of
stack too, and says so in an exception the caller can handle. Rust cannot,
which is why the refusal has to come first.

On the 2 MiB a spawned thread gets from a released binary the abort moves
to between 600 and 620. The cap of 100 leaves roughly a threefold margin
on the smaller of those, matches the recursion budget protoc's own parser
carries, and is more than an order of magnitude past anything a
hand-written `.proto` nests. It also bounds the parse TIME, which grows
with the square of the nesting depth because each level's `src` repeats
every deeper level's.

**Owner: Rust, permanently.** This is a property of the language's stack
discipline, not a defect to repair.

**Executed:** `rs/tests/untrusted_test.rs`, which parses AT the cap and one
level under it as well as past it. A fixture row cannot express it: the
input is larger than a cell, and the canonical runtime's answer is a
descriptor 100 levels deep.

## 3. Divergences this repository records that are NOT Rust's

The register carries four more rows, where Rust agrees with the canonical
TypeScript and **Go** does not. They are here so the file and the register
say the same thing, and because a reader looking for "where do the ports
disagree" should find all of it in one place.

| input | TypeScript | Go | Rust |
|---|---|---|---|
| `int32 a = 1 [__proto__ = 2];` | no `options` | `"options":{"__proto__":2}` | no `options` |
| `optional int32 a = 1 [x = -0x10];` | `"x":"-0x10"` | `"x":-16` | `"x":"-0x10"` |
| `optional int32 a = 1_0;` | `"number":null` | `"number":10` | `"number":null` |
| proto3 `optional group G = 1 { }` | `proto3Optional`, `_g` oneof | neither | `proto3Optional`, `_g` oneof |

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

**Owner: Go**, for all four.

**Executed:** `test/divergent.tsv`, rows 3 to 6.

## What is NOT a divergence

Recorded here because each was measured and found equal, and a later
reader should not have to measure it again.

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
- **An rpc named `Stream`.** Refused, in TypeScript and in Rust alike: the
  shared lexer matches a word keyword without regard to case, so the name
  collides with the `stream` modifier. `rs/tests/proto_test.rs`
  `an_rpc_named_after_a_keyword_is_refused_in_every_runtime` pins it.
- **Everything else.** A differential run over 450 inputs (every shared
  fixture row, the whole of protoc's `valid`, `accept-only` and `invalid`
  lanes, the leniency probes and 90 hand-written edge cases aimed at the
  hand-rolled scanners, the option-name reader, the range and
  reserved-name lists, the number reader and the version options) found
  TypeScript and Rust agreeing on **every** accept-or-reject decision,
  and producing **byte-identical JSON** for 446 of them. The four that
  differ are exactly the cases in section 1.
