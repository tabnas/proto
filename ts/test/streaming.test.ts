/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The parser a host streams a `.proto` document through, `makeStreaming()`,
// against the parser for a tree, `make()`. Its grammar is the compiled one
// without the CST's builders, so it builds no tree, and its rule passes,
// which the transducer part (alchemy/transduce.alc) reads, must be
// `make()`'s: over every document the window test reads, every pass alike,
// its rule, state, depth, alternate, counters and tokens, and whether the
// rule's node is a value a pass carries, and every parse ending alike, an
// error at the same code, place and token. go/streaming_test.go and
// rs/tests/streaming_test.rs hold the Go and Rust ports to the same.

import * as assert from 'node:assert/strict'
import { test } from 'node:test'

import compiled from '../src/proto-grammar.json'
import { make, makeStreaming } from '../dist/proto'

import { documents, longDocument, longErrorDocuments } from './documents'

// A parser `build` makes, whose passes `parse` logs: every pass of a parse
// of `text`, and how the parse ended.
function logging(build: () => any): (text: string) => string[] {
  const tn: any = build()
  let out: string[] = []
  tn.sub({
    ruleDone: (rule: any, _ctx: any, done: any) => {
      const open = 'o' === done.state
      const tokens = (open ? rule.o.slice(0, rule.oN) : rule.c.slice(0, rule.cN))
        .map((t: any) => t.name + ' ' + JSON.stringify(t.src) + ' ' + t.sI)
      const alt = null == done.alt ? '-' : [done.alt.p, done.alt.r, done.alt.b, done.alt.g].join('/')
      // Whether the pass carries the rule's node as a value, as transduce's
      // parser-event source asks it: a close pass does, where the node is
      // defined and not a container.
      const node = rule.node
      const scalar = !open && undefined !== node && (null === node || 'object' !== typeof node)
      out.push([rule.name, done.state, rule.d, rule.i, alt, true === done.forced, true === done.probe,
        JSON.stringify(rule.n), scalar, tokens.join(', ')].join(' | '))
    },
  })
  return (text: string) => {
    out = []
    try {
      tn.parse(text)
      out.push('ok')
    } catch (e: any) {
      const { code, pos, row, col, token } = e.toJSON()
      out.push('err ' + JSON.stringify({ code, pos, row, col, token: token?.src }))
    }
    return out
  }
}

test('makeStreaming() builds no tree', () => {
  const text = 'syntax = "proto3";\nmessage M { int32 a = 1 [(o) = { x: 1 }]; }\n'
  assert.equal(makeStreaming().parse(text), undefined)
  assert.equal((make() as any).parse(text).rule, 'proto')
  const builders = JSON.stringify(compiled).match(/"@(node|capture|bubble)\$"/g) ?? []
  assert.ok(800 < builders.length, String(builders.length))
})

test("makeStreaming()'s passes are make()'s over every document, error ends included", () => {
  const all: [string, string][] = documents()
  all.push(['the long document', longDocument()])
  longErrorDocuments().forEach((text, index) => all.push([`long error document ${index}`, text]))
  assert.ok(500 < all.length, String(all.length))
  const tree = logging(make)
  const stream = logging(makeStreaming)
  let compared = 0
  let failed = 0
  let started = Date.now()
  all.forEach(([where, text], index) => {
    const want = tree(text)
    const got = stream(text)
    assert.equal(got.length, want.length, where + ': the count of passes')
    for (let i = 0; i < want.length; i++) assert.equal(got[i], want[i], `${where}: pass ${i}`)
    compared += want.length
    if (want[want.length - 1].startsWith('err ')) failed++
    if (20000 < Date.now() - started) {
      started = Date.now()
      process.stderr.write(`streaming passes: ${index + 1} of ${all.length} documents ` +
        `(${Math.round((100 * (index + 1)) / all.length)}%)\n`)
    }
  })
  assert.ok(50 < failed, failed + ' documents fail')
  process.stderr.write(`streaming passes: ${all.length} documents, ${failed} failing, ${compared} passes alike\n`)
})
