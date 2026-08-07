/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// Leniency lane — does this plugin accept things .proto does not allow?
//
// The suspicion across all these plugins is that jsonic's base leniency leaks
// through. For @tabnas/proto the DOCUMENTED setup is `new Tabnas().use(Proto)`
// and nothing else — there is no `.use(jsonic)` in the stack, so the json5-style
// leak (where '{a:1' errors with the plugin alone but is ACCEPTED through the
// documented stack) does not apply here; the `json-object` and
// `unterminated-json-object` probes below pin that.
//
// The same failure CLASS lives one level down, in the shared @tabnas/abnf lexer
// that this grammar is compiled against: it is jsonic's lexer, so `#` line
// comments, backtick strings and `1_0` digit separators tokenise happily and
// become legal proto.
//
// Probe inputs: test/leniency-probes.json (ours, committed).
// Probe VERDICTS: recorded by running the real protoc v35.1 —
// scripts/fetch-protobuf-corpus.sh writes test/protobuf-suite/leniency.json.
// Never hand-written, never asserted from memory.
//
// THIS TEST MUST NEVER SKIP. Missing verdicts = loud failure.

import { describe, it } from 'node:test'
import assert from 'node:assert'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const { parse } = require('..')

const SUITE = join(__dirname, '..', '..', 'test', 'protobuf-suite')
const FILE = join(SUITE, 'leniency.json')
const FETCH = 'scripts/fetch-protobuf-corpus.sh'

// Pinned so probes cannot be quietly dropped.
const EXPECT_PROBES = 13

type Probe = {
  name: string
  input: string
  why: string
  accepted: boolean
  protoc: string
}

function load(): { protocVersion: string; probes: Probe[] } {
  if (!existsSync(FILE)) {
    throw new Error(
      `\n\nprotoc leniency verdicts are MISSING from ${FILE}.\n` +
        `This test does not skip. Run:\n\n    ${FETCH}\n\n`,
    )
  }
  const data = JSON.parse(readFileSync(FILE, 'utf8'))
  assert.equal(
    data.probes.length,
    EXPECT_PROBES,
    `expected ${EXPECT_PROBES} probes, got ${data.probes.length}. ` +
      `Do not shrink the probe set to get green.`,
  )
  return data
}

// Load eagerly, but NEVER by throwing out of a describe() body: node's test
// runner reports a describe-body throw as a failed SUITE while counting ZERO
// failed TESTS — and then exits 0. A load failure is therefore turned into a
// real, leaf, failing `it()`, which does set the exit code.
let verdicts: { protocVersion: string; probes: Probe[] } | null = null
let loadError: Error | null = null
try {
  verdicts = load()
} catch (e: any) {
  loadError = e
}

describe('leniency vs real protoc', () => {
  it('protoc verdicts are present and complete', () => {
    if (loadError) throw loadError
  })

  if (!verdicts) return
  const { protocVersion, probes } = verdicts

  for (const p of probes) {
    it(`${p.name} — protoc ${p.accepted ? 'ACCEPTS' : 'REJECTS'} (${protocVersion})`, () => {
      let threw = false
      let result: any
      let err = ''
      try {
        result = parse(p.input)
      } catch (e: any) {
        threw = true
        err = String(e && e.message).split('\n')[0]
      }

      if (p.accepted) {
        assert.ok(
          !threw,
          `rejected input that protoc accepts: ${err}\n` +
            `  input: ${JSON.stringify(p.input)}\n  why:   ${p.why}`,
        )
      } else {
        assert.ok(
          threw,
          `LENIENT: accepted input that protoc rejects.\n` +
            `  input:  ${JSON.stringify(p.input)}\n` +
            `  protoc: ${p.protoc}\n` +
            `  why:    ${p.why}\n` +
            `  parsed: ${JSON.stringify(result)}`,
        )
      }
    })
  }
})

// The json5 leak, tested directly rather than by assertion: build the plugin
// alone and confirm it classifies these inputs the same way the documented
// entry point (`parse`, which is `new Tabnas().use(Proto)`) does. If a future
// change adds jsonic to the stack, this is where the divergence shows up.
describe('leniency: plugin alone vs the documented stack', () => {
  const { Tabnas } = require('@tabnas/parser')
  const { Proto } = require('..')

  // The exact json5 leak inputs, plus a control. `#c\n` is deliberately NOT
  // here: it is recognised by the grammar but crashes the descriptor walk, so
  // it measures the walk rather than the stack. That defect is pinned by the
  // `empty-file` probe in the protoc lane above.
  const inputs = [
    '{a:1}',
    '{a:1',
    'syntax = "proto3"; message M { int32 a = 1; }',
  ]

  for (const src of inputs) {
    it(`same verdict for ${JSON.stringify(src)}`, () => {
      const bare = (() => {
        try {
          const tn = new Tabnas({ rewind: { history: 8192 } })
          Proto(tn)
          tn.parse(src)
          return 'accept'
        } catch {
          return 'reject'
        }
      })()
      const documented = (() => {
        try {
          parse(src)
          return 'accept'
        } catch {
          return 'reject'
        }
      })()
      assert.equal(
        bare,
        documented,
        `plugin alone says ${bare} but the documented stack says ${documented} — ` +
          `that is the json5 leniency leak appearing in proto.`,
      )
    })
  }
})
