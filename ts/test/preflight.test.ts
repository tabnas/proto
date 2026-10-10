/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The nesting cap, the TypeScript half of `rs/tests/untrusted_test.rs`'s
// nesting tests; `go/preflight_test.go` is the Go one. `parse` refuses a
// document nesting deeper than MAX_NESTING_DEPTH before the engine builds
// a tree that deep, with the message the other two runtimes give, and the
// shared fixture `test/spec/nesting.tsv` holds all three to the same
// documents.

import { describe, it } from 'node:test'
import assert from 'node:assert'
import Fs from 'node:fs'
import Path from 'node:path'

const { parse, preflight, toDescriptor, Proto, MAX_NESTING_DEPTH } = require('..')
const { Tabnas } = require('@tabnas/parser')

// `message M { message M { ... } }`, nested `depth` levels.
const nested = (depth: number) =>
  'syntax = "proto2";\n' + 'message M {'.repeat(depth) + '}'.repeat(depth)

const levels = (fdp: any) => {
  let depth = 0
  for (let m = fdp.messageType[0]; m; m = m.nestedType[0]) depth++
  return depth
}

describe('nesting cap', () => {
  it('is the cap every runtime carries', () => {
    assert.equal(MAX_NESTING_DEPTH, 100)
  })

  it('accepts a document nesting exactly to the cap, and one under it, whole', () => {
    // A cap nobody tests at is a number, not a bound.
    assert.equal(levels(parse(nested(MAX_NESTING_DEPTH))), MAX_NESTING_DEPTH)
    assert.equal(levels(parse(nested(MAX_NESTING_DEPTH - 1))), MAX_NESTING_DEPTH - 1)
  })

  it('refuses a document past the cap, naming the depth', () => {
    for (const depth of [MAX_NESTING_DEPTH + 1, 10 * MAX_NESTING_DEPTH]) {
      assert.throws(() => parse(nested(depth)), {
        message: `proto: document nests ${depth} levels deep, past the 100 this parser accepts`,
      })
    }
  })

  it('refuses with no code, as every refusal the plugin makes itself', () => {
    let error: any
    try { parse(nested(MAX_NESTING_DEPTH + 1)) } catch (e) { error = e }
    assert.ok(error instanceof Error)
    assert.equal((error as any).code, undefined)
  })

  it('refuses an unclosed pile before the engine sees it', () => {
    const start = Date.now()
    assert.throws(() => parse('syntax = "proto2";\n' + 'message M {'.repeat(100000)),
      /nests 100000 levels deep/)
    // The engine never runs: 100,000 levels cost the unguarded walk tens
    // of seconds before it overflowed its stack.
    assert.ok(Date.now() - start < 5000, `took ${Date.now() - start} ms`)
  })

  it('does not count a brace inside a string or a comment', () => {
    const noise = '{'.repeat(4 * MAX_NESTING_DEPTH)
    for (const src of [
      `syntax = "proto2";\noption a = "${noise}";\n`,
      `syntax = "proto2";\noption a = '${noise}';\n`,
      `syntax = "proto2";\noption a = "\\"${noise}";\n`,
      `syntax = "proto2";\n// ${noise}\nmessage M {}\n`,
      `syntax = "proto2";\n/* ${noise} */\nmessage M {}\n`,
      `syntax = "proto2";\n# ${noise}\nmessage M {}\n`,
    ]) {
      assert.doesNotThrow(() => preflight(src), src.slice(0, 40))
      assert.doesNotThrow(() => parse(src), src.slice(0, 40))
    }
  })

  it('counts every brace, an aggregate option value\'s included', () => {
    const src = 'syntax = "proto2";\noption (x) = {' + 'a {'.repeat(100) + '}'.repeat(101) + ';'
    assert.throws(() => preflight(src), /nests 101 levels deep/)
  })

  it('is exported on its own, for a caller that drives the engine', () => {
    preflight(nested(MAX_NESTING_DEPTH - 1))
    preflight(nested(MAX_NESTING_DEPTH))
    assert.throws(() => preflight(nested(MAX_NESTING_DEPTH + 1)), /nests 101 levels deep/)

    // The engine's own parse is not guarded: the caller runs the check.
    const tn = new Tabnas({ rewind: { history: 8192 } }).use(Proto)
    const src = nested(MAX_NESTING_DEPTH)
    preflight(src)
    assert.equal(levels(toDescriptor(tn.parse(src))), MAX_NESTING_DEPTH)

    // And the walk has no bound of its own: a tree past the cap, from a
    // caller who skipped the check, is walked whole. The Rust walk refuses
    // one, which DIVERGENCE.md section 2 records.
    const deep = nested(MAX_NESTING_DEPTH + 1)
    assert.equal(levels(toDescriptor(tn.parse(deep))), MAX_NESTING_DEPTH + 1)
  })
})


// The scan skips strings and comments where the lexer finds them, no
// sooner and no later: a brace it skips that the lexer counts would let a
// document past the cap reach the engine.
describe('nesting cap, where the lexer finds strings and comments', () => {
  // A document one level past the cap after `head`.
  const past = (head: string) =>
    head + 'message M {'.repeat(MAX_NESTING_DEPTH + 1) + '}'.repeat(MAX_NESTING_DEPTH + 1)

  const refusedAt = (src: string, depth: number) => {
    const message = new RegExp(`nests ${depth} levels deep`)
    assert.throws(() => preflight(src), message, src.slice(0, 60))
    assert.throws(() => parse(src), message, src.slice(0, 60))
  }

  it('ends a line comment at a carriage return, as the lexer ends a line', () => {
    for (const comment of ['// c', '# c']) {
      refusedAt(past('syntax = "proto2";\r' + comment + '\r'), MAX_NESTING_DEPTH + 1)
    }
  })

  it('does not count a brace in a backtick string, across lines too', () => {
    const noise = '{'.repeat(4 * MAX_NESTING_DEPTH)
    for (const src of [
      'syntax = "proto2";\noption a = `' + noise + '`;\n',
      'syntax = "proto2";\noption a = `\\`' + noise + '`;\n',
      'syntax = "proto2";\noption a = `' + noise + '\n' + noise + '`;\n',
    ]) {
      assert.doesNotThrow(() => preflight(src), src.slice(0, 40))
      assert.doesNotThrow(() => parse(src), src.slice(0, 40))
    }
  })

  it('opens a string only where a token starts', () => {
    // Inside a word the lexer reads a quote as part of the word, so
    // `message a"b` names a message `a"b` and the braces after it count.
    for (const quote of ['"', "'", '`']) {
      refusedAt(past('syntax = "proto2";\nmessage a' + quote + 'b {') + '}', MAX_NESTING_DEPTH + 2)
    }
    // Straight after a keyword it opens a string, whose braces close
    // nothing.
    refusedAt('syntax = "proto2";\n' + 'message M {'.repeat(90) + 'reserved"' + '}'.repeat(90) +
      '";' + 'message M {'.repeat(11) + '}'.repeat(101), 101)
    const noise = '{'.repeat(4 * MAX_NESTING_DEPTH)
    const fdp = parse('syntax = "proto2";\nmessage M { reserved"' + noise + '"; }\n')
    assert.deepEqual(fdp.messageType[0].reservedName, [noise])
  })

  it('knows the grammar\'s keywords and fixed tokens', () => {
    // After each, a quote opens a string. Sixty levels open, a string of
    // sixty closers, and sixty levels more: 120 levels when the closers
    // are a string's, 60 when they count.
    const grammar = JSON.parse(Fs.readFileSync(
      Path.join(__dirname, '..', 'src', 'proto-grammar.json'), 'utf8'))
    const depth = (lead: string) => {
      const src = '{'.repeat(60) + lead + '"' + '}'.repeat(60) + '"' + '{'.repeat(60)
      try { preflight(src) } catch (e: any) { return Number(/nests (\d+)/.exec(e.message)![1]) }
      return 0
    }
    const keywords = Object.entries(grammar.options.match.token as Record<string, string>)
    assert.ok(0 < keywords.length)
    for (const [name, re] of keywords) {
      const word = /^@~\/\^([A-Za-z]+)\(\?!\[A-Za-z0-9_\]\)\/$/.exec(re)
      assert.ok(word, `match token ${name} is not a keyword the scan can read: ${re}`)
      assert.equal(depth(' ' + word![1]), 120, `after the keyword ${word![1]}`)
      assert.equal(depth(' ' + word![1] + 'x'), 0, `after the word ${word![1]}x`)
    }
    for (const [name, token] of Object.entries(grammar.options.fixed.token as Record<string, string>)) {
      if ('{' === token || '}' === token) continue
      assert.equal(depth('a' + token), 120, `after the fixed token ${name} (${token})`)
    }
  })
})
