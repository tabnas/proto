/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The rewind history proto's parsers keep: the engine's default, 64
// consumed tokens. The compiled grammar has no probe, the one builtin that
// rewinds, so no parse rewinds; the one reader of the history is
// `recordAggregate`, which reads the last token consumed (`ctx.v1`). Every
// document the window test reads parses alike at 64 and at 8,192, the
// bound proto kept through 0.7.0, a longer history only holding a window's
// text back to its oldest token. go/history_test.go and
// rs/tests/history_test.rs hold the Go and Rust ports to the same.

import * as assert from 'node:assert/strict'
import { test } from 'node:test'

import { Tabnas } from '@tabnas/parser'

import compiled from '../src/proto-grammar.json'
import { Proto, make, makeStreaming } from '../dist/proto'

import { documents, longDocument, longErrorDocuments } from './documents'

// A parse of `text` on `tn`: its CST, or its error's code, place and token.
function outcome(tn: any, text: string): string {
  try {
    return 'ok ' + JSON.stringify(tn.parse(text))
  } catch (e: any) {
    const { code, pos, row, col, token } = e.toJSON()
    return 'err ' + JSON.stringify({ code, pos, row, col, token: token?.src })
  }
}

test('the compiled grammar has no probe, and the parsers keep the default history', () => {
  const text = JSON.stringify(compiled)
  assert.ok(!/@probe/.test(text), 'a probe builtin')
  assert.ok(!/"pd_/.test(text), 'a probe key')
  assert.equal((make() as any).options.rewind.history, 64)
  assert.equal((makeStreaming() as any).options.rewind.history, 64)
  // The streaming parser bounds the rule history, which the parser for a
  // tree leaves unbounded (./streaming.test.ts holds their passes alike).
  assert.equal((makeStreaming() as any).options.rule.history, 3)
  assert.equal((make() as any).options.rule.history, null)
})

test('every document parses alike at the default history and at 8,192', () => {
  const all: [string, string][] = documents()
  all.push(['the long document', longDocument()])
  longErrorDocuments().forEach((text, index) => all.push([`long error document ${index}`, text]))
  const short: any = new Tabnas()
  Proto(short)
  const long: any = new Tabnas({ rewind: { history: 8192 } })
  Proto(long)
  let started = Date.now()
  all.forEach(([where, text], index) => {
    assert.equal(outcome(short, text), outcome(long, text), where)
    if (20000 < Date.now() - started) {
      started = Date.now()
      process.stderr.write(`history: ${index + 1} of ${all.length} documents ` +
        `(${Math.round((100 * (index + 1)) / all.length)}%)\n`)
    }
  })
  process.stderr.write(`history: ${all.length} documents alike at 64 and at 8,192\n`)
})
