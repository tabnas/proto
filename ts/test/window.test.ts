/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole. `make()` declares `lex.window`: the nesting cap is
// counted as the lexer gives the parser its tokens, and an aggregate's text,
// which a parse of a whole text reads from the source, is kept from the
// tokens between its braces (../src/read.ts). The keywords, match tokens,
// need nothing of their own: the engine cuts a word that met the window's
// end again over a wider window (tabnas/parser 0.12.17).
//
// Every row of the shared fixtures (test/spec, test/passes, test/tree),
// every input of the protoc corpus (test/protobuf-suite) and
// test/descriptor/descriptor.proto is parsed whole and from a reader that
// hands its bytes over in pieces of 1, 3 and 64 bytes, under the engine's
// window and under windows whose margin and block are a few units, so that
// every token meets the window's edge; a long document, whose long tokens of
// every kind run past the engine's window, in pieces of 4 KiB too. The value
// (the CST, an aggregate's text included), the tokens the lexer gives, the
// rule passes, and an error's code, position, row, column and token must be
// the same. An error's excerpt is not compared: a parse from a reader shows
// a long line only as far back as the window reaches (tabnas/parser's
// DIVERGENCE.md).
//
// go/window_test.go and rs/tests/window_test.rs hold the Go and Rust ports
// to the same.

import * as assert from 'node:assert/strict'
import * as fs from 'node:fs'
import * as path from 'node:path'
import { test } from 'node:test'

import { findSpecDir } from '@tabnas/support'

import { make, parseTree, MAX_NESTING_DEPTH } from '../dist/proto'

const ROOT = path.join(__dirname, '..', '..')

// A reader that hands its bytes over at most `size` at a time.
function pieces(bytes: Uint8Array, size: number) {
  let at = 0
  return {
    read(buf: Uint8Array): number {
      const n = Math.min(size, buf.length, bytes.length - at)
      buf.set(bytes.subarray(at, at + n), 0)
      at += n
      return n
    },
  }
}

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

// `make()`'s parser with a log of the tokens the lexer gives and the rule
// passes.
function logged(log: Log): any {
  const tn: any = make()
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

// The input of every row of every fixture file of `dir`, its `\n`, `\r`,
// `\t` and `\\` the characters they name.
function rows(dir: string, out: [string, string][]): void {
  for (const file of fs.readdirSync(dir).filter((f) => f.endsWith('.tsv')).sort()) {
    fs.readFileSync(path.join(dir, file), 'utf8').split('\n').forEach((line, i) => {
      if (0 === i || '' === line || (line.startsWith('#') && !line.includes('\t'))) return
      const input = line.split('\t')[0]
        .replace(/\\([nrt\\])/g, (_, c) => ({ n: '\n', r: '\r', t: '\t', '\\': '\\' } as any)[c])
      out.push([file + ':' + (i + 1), input])
    })
  }
}

// The input of every row of the shared fixtures, every input of the protoc
// corpus and its leniency probes, and descriptor.proto.
function documents(): [string, string][] {
  const out: [string, string][] = []
  const spec = findSpecDir(__dirname)
  rows(spec, out)
  rows(path.join(ROOT, 'test', 'passes'), out)
  rows(path.join(ROOT, 'test', 'tree'), out)
  const suite = path.join(ROOT, 'test', 'protobuf-suite')
  for (const file of ['valid', 'invalid', 'accept-only']) {
    const cases = JSON.parse(fs.readFileSync(path.join(suite, file + '.json'), 'utf8'))
    cases.forEach((c: any, i: number) => out.push([file + ':' + i, c.input]))
  }
  const leniency = JSON.parse(fs.readFileSync(path.join(suite, 'leniency.json'), 'utf8'))
  leniency.probes.forEach((p: any, i: number) => {
    if ('string' === typeof p.input) out.push(['leniency:' + i, p.input])
  })
  const descriptor = path.join(ROOT, 'test', 'descriptor', 'descriptor.proto')
  out.push(['descriptor.proto', fs.readFileSync(descriptor, 'utf8')])
  return out
}

// A `.proto` file of every kind of token, the long ones longer than the
// engine's 64 KiB window, with characters past ASCII in its comments and
// strings and aggregate option values of every shape, so that tokens of
// every kind straddle the window's edges as a reader hands the text over.
// The same document as `long_document` in rs/tests/window_test.rs and
// `longDocument` in go/window_test.go.
function longDocument(): string {
  const parts: string[] = []
  parts.push('// ' + 'a comment past the window, é \u{1D11E}; '.repeat(2000) + '\r\n')
  parts.push('/* ' + 'a block comment past the window\r\n\t'.repeat(2000) + '*/\r\n')
  parts.push('syntax = "proto2";\r\npackage a.b.c;\r\n')
  parts.push('option long = "' + 'a string past the window ü '.repeat(3000) + '";\r\n')
  parts.push("option tick = '" + 'single quotes past the window '.repeat(3000) + "';\r\n")
  parts.push('option (named) = ' + 'n'.repeat(70000) + ';\r\n')
  parts.push('option (big) = {\r\n' + '  a: 1 /* c\t*/ b { c: "d" } # e\r\n\t'.repeat(2000) + '};\r\n')
  parts.push('message ' + 'N'.repeat(70000) + ' { optional int32 f = 1; }\r\n')
  for (let i = 0; i < 250; i++) {
    const hex = i.toString(16).toUpperCase()
    parts.push(`message M${i} { // régle ${i}\r\n`)
    parts.push(`  optional int32 a${i} = ${i + 1} [default = -0x${hex}, (o) = { x: ${i} y: "z" }];\r\n`)
    parts.push(`  repeated string optionals = ${i + 2};\r\n  required group G${i} = ${i + 3} { optional bool b = 1; }\r\n`)
    parts.push(`  enum E${i} { option allow_alias = true; V${i} = 0; W${i} = 1 [deprecated = true]; }\r\n`)
    parts.push(`  oneof o${i} { int64 one = ${i + 4}; }\r\n  map<string, int32> m${i} = ${i + 5};\r\n`)
    parts.push(`  extensions 100 to max;\r\n  reserved 50 to 60;\r\n  reserved "res${i}";\r\n}\r\n`)
    parts.push(`service S${i} { rpc R${i}(stream M${i}) returns (M${i}) { option (x) = {a: 1}; } }\r\n`)
  }
  parts.push('extend M0 { optional int32 ext = 1000; }\r\n')
  return parts.join('')
}

test('make() reads a reader a window at a time', () => {
  assert.equal((make() as any).options.lex.window, true)
})

test('every fixture and corpus document parses the same from a reader', () => {
  const log: Log = { tokens: [], rules: [] }
  const tn = logged(log)
  const all = documents()
  assert.ok(500 < all.length, String(all.length))
  let compared = 0
  let aggregates = 0
  for (const [where, text] of all) {
    compared += differential(tn, log, where, text, 16 * 1024 < text.length ? BIG_READS : READS)
    try {
      if (JSON.stringify(tn.parse(text)).includes('"aggregate"')) aggregates++
    } catch {}
    log.tokens = []
    log.rules = []
  }
  assert.ok(50 <= aggregates, aggregates + ' documents hold an aggregate')
  process.stderr.write(`window documents: ${all.length}, ${aggregates} with an aggregate, ` +
    `${compared} reader parses\n`)
})

test('a long document parses the same from a reader', () => {
  const log: Log = { tokens: [], rules: [] }
  const tn = logged(log)
  const text = longDocument()
  assert.ok(512 * 1024 < text.length, String(text.length))
  assert.equal((parseTree(text).body as unknown[]).length, 508)
  assert.equal(differential(tn, log, 'the long document', text, LONG_READS), LONG_READS.length)
})

test('a long document fails the same from a reader', () => {
  // A token that runs past the engine's window and is then refused, or
  // never ends, and nesting past the cap after a long run: the error is
  // the whole text's, code, place and token.
  const log: Log = { tokens: [], rules: [] }
  const tn = logged(log)
  const pad = '// ' + 'padding ü '.repeat(9000) + '\r\nsyntax = "proto3";\r\n'
  const cap = MAX_NESTING_DEPTH + 1
  const bad = [
    pad + 'option a = "' + 'unclosed string '.repeat(6000) + '\r\n',
    pad + '/* ' + 'an unclosed comment '.repeat(4000) + '\r\n',
    pad + 'message M { ' + 'int32 a = 1; '.repeat(6000) + '\r\n',
    pad + 'option b = {' + ' c: 1'.repeat(15000) + ' ) };\r\n',
    pad + 'message ' + 'm'.repeat(70000) + ' } \r\n',
    pad + 'option (x) = ' + '{a '.repeat(cap) + '}'.repeat(cap) + ';\r\n',
    pad + 'message M{'.repeat(cap) + '}'.repeat(cap),
    pad + 'message M {'.repeat(100000),
  ]
  bad.forEach((text, index) => {
    assert.throws(() => make().parse(text), `long error document ${index} parses`)
    differential(tn, log, `long error document ${index}`, text, LONG_READS)
  })
})

test('the parser refuses nesting past the cap as it reads', () => {
  // The brace that opens the level past the cap is refused, with the code
  // proto_too_deep at that brace, whole and from a reader, and the cap
  // itself parses.
  const nest = (depth: number, open: string) =>
    'syntax = "proto2";\n' + open.repeat(depth) + '}'.repeat(depth)
  const tn: any = make()
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

test('a long document parses from a reader in time linear in its length', () => {
  // Ten times the messages, read through the engine's window in pieces of
  // 4 KiB, take about ten times as long, never the square of it.
  const tn: any = make()
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
  process.stderr.write(`window linear time: 300 messages ${one.toFixed(3)}s, ` +
    `3000 messages ${ten.toFixed(3)}s, ${(ten / one).toFixed(1)} times\n`)
  assert.ok(ten < 30 * one, `${one}s and ${ten}s`)
})
