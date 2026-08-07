/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// Third-party conformance: protobuf's OWN parser test corpus.
//
//   upstream  https://github.com/protocolbuffers/protobuf  v35.1
//   commit    35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03   (pinned)
//   source    src/google/protobuf/compiler/parser_unittest.cc
//
// The corpus is NOT committed to this repo. `scripts/fetch-protobuf-corpus.sh`
// downloads it at the pinned commit into `test/protobuf-suite/` (gitignored).
//
// THIS TEST MUST NEVER SKIP. If the corpus is absent it FAILS, loudly, with
// instructions — a conformance test that quietly does not run is worse than no
// test at all, because the green tick is a lie.
//
// `go/protobuf_test.go` reads the SAME files with the same contracts.

import { describe, it } from 'node:test'
import assert from 'node:assert'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const { parse } = require('..')

const SUITE = join(__dirname, '..', '..', 'test', 'protobuf-suite')
const FETCH = 'scripts/fetch-protobuf-corpus.sh'

// Case counts observed at the pinned commit. Pinned here so the corpus cannot
// be quietly shrunk (or silently grow stale) without a test going red.
const EXPECT_COUNTS: Record<string, number> = {
  'valid.json': 82,
  'invalid.json': 96,
  'accept-only.json': 50,
  'excluded.json': 6,
}

type Case = {
  name: string
  helper: string
  input: string
  expected?: any
  error?: string
  note?: string
}

function missing(): string {
  return (
    `\n\nThe protobuf conformance corpus is MISSING from ${SUITE}.\n` +
    `This test does not skip. Run:\n\n    ${FETCH}\n\n` +
    `(CI runs it before the tests; see .github/workflows/conformance.yml.)\n`
  )
}

function load(file: string): Case[] {
  const path = join(SUITE, file)
  if (!existsSync(path)) throw new Error(`${file} not found.` + missing())
  const cases = JSON.parse(readFileSync(path, 'utf8')) as Case[]
  assert.equal(
    cases.length,
    EXPECT_COUNTS[file],
    `${file}: ${cases.length} cases, expected ${EXPECT_COUNTS[file]}. ` +
      `The corpus changed — re-pin it deliberately, do not just edit this number ` +
      `to make the suite pass.`,
  )
  return cases
}

// The ONE normalisation, applied identically to BOTH sides: drop a key whose
// value is null/undefined, an empty array, or an empty object. It exists only
// because protojson omits unset repeated fields while @tabnas/proto emits `[]`
// for them. It can never hide a difference in a PRESENT value — if either side
// has a value the other lacks, the compare still fails.
//
// Nothing else is normalised. In particular this does NOT paper over `syntax`
// emitted where protoc leaves it unset, `type` emitted for an unresolved type
// reference, or descriptor fields landing inside `options`. Those are real
// divergences and are supposed to show up red.
function norm(v: any): any {
  if (Array.isArray(v)) return v.map(norm)
  if (v && 'object' === typeof v) {
    const out: any = {}
    for (const k of Object.keys(v).sort()) {
      const nv = norm(v[k])
      if (null === nv || undefined === nv) continue
      if (Array.isArray(nv) && 0 === nv.length) continue
      if (nv && 'object' === typeof nv && !Array.isArray(nv) && 0 === Object.keys(nv).length) continue
      out[k] = nv
    }
    return out
  }
  return v
}

function label(c: Case): string {
  const one = c.input.replace(/\s+/g, ' ').trim()
  return `${c.name}: ${60 < one.length ? one.slice(0, 57) + '...' : one}`
}

// Load eagerly, but NEVER by throwing out of a describe() body: node's test
// runner reports a describe-body throw as a failed SUITE while counting ZERO
// failed TESTS — and then exits 0. That is the exact "green tick that lies"
// this whole effort exists to kill. So a load failure is turned into a real,
// leaf, failing `it()` below, which does set the exit code.
let corpus: { valid: Case[]; invalid: Case[]; acceptOnly: Case[] } | null = null
let loadError: Error | null = null
try {
  corpus = {
    valid: load('valid.json'),
    invalid: load('invalid.json'),
    acceptOnly: load('accept-only.json'),
  }
  load('excluded.json') // pins the exclusion count at exactly 6
} catch (e: any) {
  loadError = e
}

describe('protobuf corpus (protocolbuffers/protobuf v35.1 parser_unittest.cc)', () => {
  it('corpus is present and complete', () => {
    if (loadError) throw loadError
    assert.ok(existsSync(SUITE))
  })

  if (!corpus) return

  // valid: must parse AND produce exactly protoc's parser descriptor.
  describe('valid — parses and equals protoc\'s descriptor', () => {
    for (const c of corpus.valid) {
      it(label(c), () => {
        const got = JSON.parse(JSON.stringify(parse(c.input)))
        assert.deepStrictEqual(norm(got), norm(c.expected))
      })
    }
  })

  // invalid: must be REJECTED. protoc's parser reports an error for these.
  describe('invalid — must be rejected', () => {
    for (const c of corpus.invalid) {
      it(label(c), () => {
        let result: any
        let threw = false
        try {
          result = parse(c.input)
        } catch {
          threw = true
        }
        assert.ok(
          threw,
          `accepted input that protoc rejects with ${JSON.stringify(c.error)}\n` +
            `  input:  ${JSON.stringify(c.input)}\n` +
            `  parsed: ${JSON.stringify(result)}`,
        )
      })
    }
  })

  // accept-only: protoc's PARSER accepts these (upstream asserts the error
  // collector is empty); they fail only later in DescriptorPool validation,
  // which this package does not perform. Upstream publishes no descriptor
  // golden, so this lane can only assert accept/reject. Reported separately —
  // it is never folded into the headline valid figure.
  describe('accept-only — protoc\'s parser accepts (validation is out of scope)', () => {
    for (const c of corpus.acceptOnly) {
      it(label(c), () => {
        assert.doesNotThrow(
          () => parse(c.input),
          `rejected input that protoc's parser accepts (upstream: ${c.helper})\n` +
            `  input: ${JSON.stringify(c.input)}`,
        )
      })
    }
  })
})
