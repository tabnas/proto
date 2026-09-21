#!/usr/bin/env node

// The oracle for `rs/src/jsnum.rs` `js_number_to_string`, which has to
// spell a double exactly as `JSON.stringify` does.
//
// It builds the SAME value list `rs/tests/jsnum_test.rs` builds, formats
// each one the way the canonical runtime would, and prints the count and
// two FNV-1a hashes: one over the raw bit patterns, which says the two
// generators agree on the VALUES, and one over the formatted text, which
// is what the Rust test pins.
//
// Run:
//
//   node rs/scripts/js-number-oracle.mjs
//
// and compare with what `cargo test --test jsnum_test` prints for the
// same three lines. Both are pinned in the test, so a disagreement is a
// red build rather than a thing to notice.
//
// Keep the generator here in step with the one in the test. The test
// pins the COUNT for that reason: a generator that changed on one side
// only fails before any hash is compared.

const values = []

// Small integers: the `k <= n <= 21` branch, all of it.
for (let i = 0; i < 10000; i++) values.push(i)
// Sixteenths, tenths, hundredths and thousandths: where an exact decimal
// midpoint sits between two shortest digit strings and the specification
// takes the even one.
for (let i = 1; i <= 10000; i++) values.push(i / 16)
for (let i = 1; i <= 10000; i++) values.push(i / 10)
for (let i = 1; i <= 10000; i++) values.push(i / 100)
for (let i = 1; i <= 5000; i++) values.push(i / 1000)
// Negatives, including the sign path.
for (let i = 1; i <= 5000; i++) values.push(-i / 8)
// Every decade across the two thresholds where the spelling changes
// form, 1e21 and 1e-7.
for (const m of [1, 2, 3, 5, 7, 9]) {
  for (let k = -30; k <= 30; k++) values.push(Number(`${m}e${k}`))
}
// Raw bit patterns, so the digit generator meets doubles no decimal
// literal would reach. A 64-bit LCG, the same constants as the test.
const MASK = (1n << 64n) - 1n
const view = new DataView(new ArrayBuffer(8))
let state = 0x2545f4914f6cdd1dn
let taken = 0
while (taken < 10000) {
  state = (state * 6364136223846793005n + 1442695040888963407n) & MASK
  view.setBigUint64(0, state)
  const candidate = view.getFloat64(0)
  if (Number.isFinite(candidate)) {
    values.push(candidate)
    taken++
  }
}

const OFFSET = 0xcbf29ce484222325n
const PRIME = 0x100000001b3n

function fnv1a(bytes) {
  let hash = OFFSET
  for (const byte of bytes) {
    hash = ((hash ^ BigInt(byte)) * PRIME) & MASK
  }
  return hash
}

// The bit patterns, big-endian, so the two generators can be compared
// before their formatting is.
const bits = []
for (const value of values) {
  view.setFloat64(0, value)
  for (let i = 0; i < 8; i++) bits.push(view.getUint8(i))
}

const text = values.map((value) => String(value)).join('\n')

console.log('count', values.length)
console.log('bits', fnv1a(bits).toString(16))
console.log('text', fnv1a(Buffer.from(text, 'utf8')).toString(16))
