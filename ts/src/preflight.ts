/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The nesting preflight: a `.proto` document nesting deeper than
// MAX_NESTING_DEPTH is refused before the engine builds a tree that deep.
//
// The tree costs more than its nesting suggests. A CST node's `src` holds
// every token beneath it, so a document nested `d` levels deep builds
// source text that grows with d squared, and so do the parse's time and
// memory: the walk here overflows its stack from about 3,000 levels, after
// seconds and hundreds of megabytes; Go's parse grows past any memory a
// machine has; and a Rust stack that runs out aborts the process.
// DIVERGENCE.md section 2 has the measurements. So every runtime refuses
// the same documents, with the same message, at the same cap: the Rust
// port's, which sits more than an order of magnitude past anything a
// hand-written `.proto` nests and matches the recursion budget protoc's
// own parser carries.
//
// Port of `preflight` and `brace_depth` in the Rust crate (`rs/src/lib.rs`,
// `rs/src/build_descriptor.rs`); `go/preflight.go` is the Go one.

// How deep a document may nest, in braces. `parse` refuses a deeper one.
export const MAX_NESTING_DEPTH = 100

// Refuse a `.proto` source that nests deeper than MAX_NESTING_DEPTH,
// before anything builds a tree that deep. `parse` runs it; a caller who
// drives the engine directly (`new Tabnas().use(Proto)` and `tn.parse`)
// runs it on the source first. Throws an Error whose message names the
// depth; like every refusal the plugin makes itself, it carries no code.
export function preflight(src: string): void {
  const depth = braceDepth(src)
  if (depth > MAX_NESTING_DEPTH) {
    throw new Error(
      `proto: document nests ${depth} levels deep, past the ` +
      `${MAX_NESTING_DEPTH} this parser accepts`,
    )
  }
}

// The nesting depth of a `.proto` source, counted in braces, skipping the
// string literals and the comments the tabnas lexer skips.
//
// Over-counting is safe and under-counting is not, so an unterminated
// string or comment counts every brace inside it: the engine rejects that
// source anyway. Only ASCII characters decide anything here, so scanning
// UTF-16 code units counts exactly what the Rust and Go ports count
// scanning UTF-8 bytes.
function braceDepth(src: string): number {
  const length = src.length
  let at = 0
  let depth = 0
  let deepest = 0
  while (at < length) {
    const ch = src.charCodeAt(at)
    if (0x7b === ch) { // {
      depth++
      if (depth > deepest) deepest = depth
      at++
    } else if (0x7d === ch) { // }
      if (0 < depth) depth--
      at++
    } else if (0x22 === ch || 0x27 === ch) { // " or '
      at++
      while (at < length && src.charCodeAt(at) !== ch) {
        at += 0x5c === src.charCodeAt(at) ? 2 : 1 // a backslash escapes one
      }
      at++
    } else if (0x23 === ch) {
      // The shared tabnas lexer reads `#` as a line comment; `.proto`
      // does not, which the leniency corpus records.
      while (at < length && 0x0a !== src.charCodeAt(at)) at++
    } else if (0x2f === ch && 0x2f === src.charCodeAt(at + 1)) { // //
      while (at < length && 0x0a !== src.charCodeAt(at)) at++
    } else if (0x2f === ch && 0x2a === src.charCodeAt(at + 1)) { // /*
      at += 2
      while (at < length &&
        !(0x2a === src.charCodeAt(at) && 0x2f === src.charCodeAt(at + 1))) {
        at++
      }
      at += 2
    } else {
      at++
    }
  }
  return deepest
}
