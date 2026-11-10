/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole, by each of the two parsers, `make()`'s, whose value is
// the CST, and `makeStreaming()`'s, which builds no tree. Both declare
// `lex.window`: the nesting cap is counted as the lexer gives the parser its
// tokens, and `make()`'s parser keeps an aggregate's text, which a parse of
// a whole text reads from the source, from the tokens between its braces
// (../src/read.ts). The keywords, match tokens, need nothing of their own:
// the engine cuts a word that met the window's end again over a wider
// window (tabnas/parser 0.12.17).
//
// Every row of the shared fixtures (test/spec, test/passes, test/tree),
// every input of the protoc corpus (test/protobuf-suite) and
// test/descriptor/descriptor.proto is parsed whole and from a reader that
// hands its bytes over in pieces of 1, 3 and 64 bytes, under the engine's
// window and under windows whose margin and block are a few units, so that
// every token meets the window's edge; a long document, whose long tokens of
// every kind run past the engine's window, in pieces of 4 KiB too. The value
// (the CST, an aggregate's text included, or the streaming parser's none),
// the tokens the lexer gives, the rule passes, and an error's code,
// position, row, column and token must be the same. An error's excerpt is not compared: a parse from a reader shows
// a long line only as far back as the window reaches (tabnas/parser's
// DIVERGENCE.md).
//
// go/window_test.go and rs/tests/window_test.rs hold the Go and Rust ports
// to the same.

import * as assert from 'node:assert/strict'
import { test } from 'node:test'

import { make, makeStreaming, parseTree, MAX_NESTING_DEPTH } from '../dist/proto'

import { documents, longDocument, longErrorDocuments, pieces } from './documents'

// A read of a source: the pieces a reader hands its bytes over in, and the
// window it is read through, the engine's own (64 KiB) where `margin` and
// `block` are absent, or one whose margin and block are a few units, which
// the hidden `parseReaderWithSizes` sets, its reads as small as the pieces.
type Reading = { size: number; margin?: number; block?: number }
const WINDOWS = [{}, { margin: 1, block: 1 }, { margin: 3, block: 2 }, { margin: 64, block: 64 }]
const READS: Reading[] = [1, 3, 64].flatMap((size) => WINDOWS.map((window) => ({ size, ...window })))

// A document longer than 16 KiB, of which the corpus holds one, in pieces
// of 64 and 4096 bytes, through the engine's window and one of 64 and 64
// units.
const BIG_READS: Reading[] = [{ size: 64 }, { size: 4096 }, { size: 64, margin: 64, block: 64 }]

// A long document in pieces of 1, 3, 64 and 4096 bytes through the
// engine's window, whose edges its long tokens straddle, and through a
// window of 64 and 64 units.
const LONG_READS: Reading[] = [
  { size: 1 },
  { size: 3 },
  { size: 64 },
  { size: 4096 },
  { size: 64, margin: 64, block: 64 },
]

type Log = { tokens: string[]; rules: string[] }

// The two parsers, each by the name of its constructor.
const PARSERS: Array<[string, () => any]> = [['make()', make], ['makeStreaming()', makeStreaming]]

// A parser `build` makes, with a log of the tokens the lexer gives and the
// rule passes.
function logged(log: Log, build: () => any): any {
  const tn: any = build()
  tn.sub({
    lex: (tkn: any) => {
      log.tokens.push(tkn.name + ' ' + JSON.stringify(tkn.src) + ' ' +
        JSON.stringify(tkn.val) + ' ' + tkn.sI + ' ' + tkn.rI + ':' + tkn.cI)
    },
    ruleDone: (rule: any, _ctx: any, done: any) => {
      log.rules.push(rule.name + ' ' + done.state + ' ' + rule.d)
    },
  })
  return tn
}

// What a parse showed: its value, or its error without the excerpt; and
// the tokens and rule passes `log` gathered, which is empty again after.
function seen(log: Log, parse: () => unknown) {
  let outcome: string
  try {
    const value = parse()
    outcome = 'ok ' + (undefined === value ? 'undefined' : JSON.stringify(value))
  } catch (e: any) {
    if (null == e || 'function' !== typeof e.toJSON) {
      outcome = 'throw ' + String(e && e.message)
    } else {
      const { code, pos, row, col, token } = e.toJSON()
      outcome = 'err ' + JSON.stringify({ code, pos, row, col, token })
    }
  }
  const tokens = log.tokens
  const rules = log.rules
  log.tokens = []
  log.rules = []
  return { outcome, tokens, rules }
}

// `text` parsed whole and from a reader in each of `reads`; the count of
// reader parses compared.
function differential(tn: any, log: Log, where: string, text: string, reads: Reading[]): number {
  const bytes = new Uint8Array(Buffer.from(text, 'utf8'))
  const decoded = new TextDecoder('utf-8', { ignoreBOM: true }).decode(bytes)
  const whole = seen(log, () => tn.parse(decoded))
  for (const { size, margin, block } of reads) {
    const read = seen(log, () => undefined === margin
      ? tn.parseReader(pieces(bytes, size))
      : tn.parseReaderWithSizes(pieces(bytes, size), { margin, block, chunk: size }))
    const at = where + ' in pieces of ' + size +
      (undefined === margin ? '' : ', margin ' + margin + ', block ' + block)
    assert.equal(read.outcome, whole.outcome, at)
    assert.deepEqual(read.tokens, whole.tokens, at + ': the tokens')
    assert.deepEqual(read.rules, whole.rules, at + ': the rule passes')
  }
  return reads.length
}

for (const [name, build] of PARSERS) {
  test(`${name} reads a reader a window at a time`, () => {
    assert.equal(build().options.lex.window, true)
  })

  test(`every fixture and corpus document parses the same from a reader: ${name}`, () => {
    const log: Log = { tokens: [], rules: [] }
    const tn = logged(log, build)
    const all = documents()
    assert.ok(500 < all.length, String(all.length))
    const tree: any = make()
    let compared = 0
    let aggregates = 0
    for (const [where, text] of all) {
      compared += differential(tn, log, where, text, 16 * 1024 < text.length ? BIG_READS : READS)
      try {
        if (JSON.stringify(tree.parse(text)).includes('"aggregate"')) aggregates++
      } catch {}
    }
    assert.ok(50 <= aggregates, aggregates + ' documents hold an aggregate')
    process.stderr.write(`window documents, ${name}: ${all.length}, ${aggregates} with an aggregate, ` +
      `${compared} reader parses\n`)
  })

  test(`a long document parses the same from a reader: ${name}`, () => {
    const log: Log = { tokens: [], rules: [] }
    const tn = logged(log, build)
    const text = longDocument()
    assert.ok(512 * 1024 < text.length, String(text.length))
    assert.equal((parseTree(text).body as unknown[]).length, 508)
    assert.equal(differential(tn, log, 'the long document', text, LONG_READS), LONG_READS.length)
  })

  test(`a long document fails the same from a reader: ${name}`, () => {
    // A token that runs past the engine's window and is then refused, or
    // never ends, and nesting past the cap after a long run: the error is
    // the whole text's, code, place and token.
    const log: Log = { tokens: [], rules: [] }
    const tn = logged(log, build)
    longErrorDocuments().forEach((text, index) => {
      assert.throws(() => build().parse(text), `long error document ${index} parses`)
      differential(tn, log, `long error document ${index}`, text, LONG_READS)
    })
  })

  test(`the parser refuses nesting past the cap as it reads: ${name}`, () => {
    // The brace that opens the level past the cap is refused, with the code
    // proto_too_deep at that brace, whole and from a reader, and the cap
    // itself parses.
    const nest = (depth: number, open: string) =>
      'syntax = "proto2";\n' + open.repeat(depth) + '}'.repeat(depth)
    const tn: any = build()
    tn.parse(nest(MAX_NESTING_DEPTH, 'message M {'))
    const cases: [string, number][] = [
      [nest(MAX_NESTING_DEPTH + 1, 'message M {'), 19 + 11 * MAX_NESTING_DEPTH + 10],
      [nest(MAX_NESTING_DEPTH + 1, 'message M{'), 19 + 10 * MAX_NESTING_DEPTH + 9],
      ['option (x) = ' + '{a '.repeat(MAX_NESTING_DEPTH + 1), 13 + 3 * MAX_NESTING_DEPTH],
      ['message M {'.repeat(100000), 11 * MAX_NESTING_DEPTH + 10],
    ]
    for (const [text, pos] of cases) {
      for (const fromReader of [false, true]) {
        let error: any
        try {
          fromReader
            ? tn.parseReader(pieces(new Uint8Array(Buffer.from(text)), 64))
            : tn.parse(text)
        } catch (e) {
          error = e
        }
        const at = text.slice(0, 40) + (fromReader ? ' from a reader' : '')
        assert.ok(null != error, at)
        const { code, pos: where, token } = error.toJSON()
        assert.deepEqual({ code, pos: where, src: token.src }, { code: 'proto_too_deep', pos, src: '{' }, at)
        assert.match(error.message, /proto: document nests deeper than the 100 levels this parser accepts/)
      }
    }
  })
}

for (const [name, build] of PARSERS) test(`a long document parses from a reader in time linear in its length: ${name}`, () => {
  // Ten times the messages, read through the engine's window in pieces of
  // 4 KiB, take about ten times as long, never the square of it.
  const tn: any = build()
  const document = (messages: number) =>
    'syntax = "proto3";\n' + Array.from({ length: messages },
      (_, i) => `message M${i} { string a = 1; int32 b = 2 [(o) = {x: ${i}}]; } // c\n`).join('')
  const timed = (text: string) => {
    const bytes = new Uint8Array(Buffer.from(text, 'utf8'))
    const started = process.hrtime.bigint()
    tn.parseReader(pieces(bytes, 4096))
    return Number(process.hrtime.bigint() - started) / 1e9
  }
  const small = document(300)
  const large = document(3000)
  timed(small)
  const one = timed(small)
  const ten = timed(large)
  process.stderr.write(`window linear time, ${name}: 300 messages ${one.toFixed(3)}s, ` +
    `3000 messages ${ten.toFixed(3)}s, ${(ten / one).toFixed(1)} times\n`)
  assert.ok(ten < 30 * one, `${one}s and ${ten}s`)
})
