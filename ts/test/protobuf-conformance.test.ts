/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// Conformance against protoc's own parser test corpus.
//
// `test/protobuf-suite/*.json` is a vendored extraction of upstream
// protobuf's `src/google/protobuf/compiler/parser_unittest.cc` (v35.1) —
// see `test/protobuf-suite/AGENTS.md` for provenance and lane meanings.
// This runner drives two lanes:
//
//   valid       — source + the FileDescriptorProto protoc's parser produces.
//                 Compared field for field.
//   accept-only — source protoc's parser accepts (it only asserts no error,
//                 and publishes no descriptor). Must parse without throwing.
//
// The corpus is vendored, so this never skips: if the files are missing the
// suite fails rather than quietly passing.

import { describe, it } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const { parse } = require('..')

const suiteDir = join(__dirname, '..', '..', 'test', 'protobuf-suite')

type ValidCase = { name: string; input: string; expected: any }
type AcceptCase = { name: string; input: string }

function load<T>(file: string): T[] {
  const body = readFileSync(join(suiteDir, file), 'utf8')
  const cases = JSON.parse(body)
  assert.ok(Array.isArray(cases) && 0 < cases.length, file + ': no cases')
  return cases
}

// ---- declared deviations from protoc's descriptor encoding ----------------
//
// @tabnas/proto records options as a plain `{name: value}` map rather than
// protoc's `uninterpretedOption` list (see AGENTS.md, "Output shape"). The
// two carry the same information, so translate protoc's encoding into ours
// and compare — a name or a value we failed to capture still fails.

function optionName(parts: any[]): string {
  return (parts || [])
    .map((p) => (p.isExtension ? '(' + p.namePart + ')' : p.namePart))
    .join('.')
}

function optionValue(u: any): any {
  if ('stringValue' in u) return Buffer.from(u.stringValue, 'base64').toString('utf8')
  if ('positiveIntValue' in u) return Number(u.positiveIntValue)
  if ('negativeIntValue' in u) return Number(u.negativeIntValue)
  if ('doubleValue' in u) {
    // protoc evaluates `inf` / `-inf` / `-nan` to a double; we keep the
    // literal text the source wrote.
    if ('Infinity' === u.doubleValue) return 'inf'
    if ('-Infinity' === u.doubleValue) return '-inf'
    if ('NaN' === u.doubleValue) return '-nan'
    return Number(u.doubleValue)
  }
  if ('aggregateValue' in u) return u.aggregateValue
  const id = u.identifierValue
  if ('true' === id) return true
  if ('false' === id) return false
  return id
}

function bridgeOptions(o: any): any {
  if (!o || !Array.isArray(o.uninterpretedOption)) return o
  const out: any = {}
  for (const [k, v] of Object.entries(o)) if ('uninterpretedOption' !== k) out[k] = v
  for (const u of o.uninterpretedOption) out[optionName(u.name)] = optionValue(u)
  return out
}

function bridge(v: any): any {
  if (Array.isArray(v)) return v.map(bridge)
  if (v && 'object' === typeof v) {
    const o: any = {}
    for (const [k, x] of Object.entries(v)) {
      o[k] = 'options' === k ? bridgeOptions(bridge(x)) : bridge(x)
    }
    return o
  }
  return v
}

// Drop absent fields and empty lists so a golden that omits a defaulted
// field compares equal, and sort keys so property order does not matter.
function norm(v: any): any {
  if (Array.isArray(v)) return v.map(norm)
  if (v && 'object' === typeof v) {
    const o: any = {}
    for (const k of Object.keys(v).sort()) {
      const nv = norm(v[k])
      if (undefined === nv) continue
      if (Array.isArray(nv) && 0 === nv.length) continue
      o[k] = nv
    }
    return o
  }
  return v
}

// `defaultValue` is a string in descriptor.proto. protoc re-renders a
// numeric default through the field's C++ type; we keep the literal as
// written. Compare numerically when both sides are numbers.
function sameDefaults(a: any, b: any): boolean {
  if ('string' !== typeof a || 'string' !== typeof b) return false
  const x = Number(a)
  const y = Number(b)
  return !Number.isNaN(x) && !Number.isNaN(y) && x === y
}

function equal(got: any, want: any): boolean {
  if (JSON.stringify(got) === JSON.stringify(want)) return true
  if (got && want && 'object' === typeof got && 'object' === typeof want) {
    if (Array.isArray(got) !== Array.isArray(want)) return false
    const keys = new Set([...Object.keys(got), ...Object.keys(want)])
    for (const k of keys) {
      if (equal(got[k], want[k])) continue
      if ('defaultValue' === k && sameDefaults(got[k], want[k])) continue
      return false
    }
    return true
  }
  return false
}

// Editions the plugin does not claim to support: protoc carries internal
// `UNSTABLE` and `NNNNN_TEST_ONLY` editions for in-development features
// (new varint types, …). @tabnas/proto documents proto2, proto3 and
// editions 2023/2024, and rejects any other edition string.
const OUT_OF_SCOPE = /edition\s*=\s*["'](?!2023|2024)/

describe('protobuf conformance: parser_unittest corpus', () => {
  const valid = load<ValidCase>('valid.json')
  const inScope = valid.filter((c) => !OUT_OF_SCOPE.test(c.input))
  const skipped = valid.length - inScope.length

  it('excludes only the editions the plugin does not claim', () => {
    // Guard the exclusion list: it must stay tiny and must be exactly the
    // protoc-internal editions, not a dumping ground for failures.
    assert.equal(skipped, 11)
    for (const c of valid) {
      if (!OUT_OF_SCOPE.test(c.input)) continue
      assert.match(c.input, /edition\s*=\s*["'](UNSTABLE|\d+_TEST_ONLY)["']/)
    }
  })

  describe('valid: source -> FileDescriptorProto', () => {
    for (const c of inScope) {
      it(`${c.name}: ${c.input.replace(/\s+/g, ' ').trim().slice(0, 60)}`, () => {
        const got = norm(JSON.parse(JSON.stringify(parse(c.input))))
        const want = norm(bridge(c.expected))
        // protoc omits `syntax` for a file with no declaration (proto2).
        if (undefined === want.syntax && 'proto2' === got.syntax) delete got.syntax
        assert.ok(equal(got, want),
          `${c.name}\n  got  ${JSON.stringify(got)}\n  want ${JSON.stringify(want)}`)
      })
    }
  })

  describe('accept-only: source protoc parses without error', () => {
    for (const c of load<AcceptCase>('accept-only.json')) {
      it(`${c.name}: ${c.input.replace(/\s+/g, ' ').trim().slice(0, 60)}`, () => {
        assert.doesNotThrow(() => parse(c.input))
      })
    }
  })
})

// Lexer-leniency probes: places where the shared tabnas lexer is more
// permissive than the .proto grammar. `accepted` is protoc's answer,
// `tabnas` is ours. Pinning both keeps the deviation surface from growing
// silently — a new divergence turns this red until it is a deliberate,
// recorded decision.
describe('protobuf conformance: lexer leniency', () => {
  const probes: any[] = JSON.parse(
    readFileSync(join(suiteDir, 'leniency.json'), 'utf8')).probes
  assert.ok(Array.isArray(probes) && 0 < probes.length, 'leniency.json: no probes')

  for (const p of probes) {
    it(`${p.name}${p.accepted === p.tabnas ? '' : ' (recorded deviation)'}`, () => {
      let accepted = true
      try { parse(p.input) } catch { accepted = false }
      assert.equal(accepted, p.tabnas, p.tabnasNote || p.why)
    })
  }

  it('deviates from protoc on exactly the recorded probes', () => {
    const diverge = probes.filter((p) => p.accepted !== p.tabnas).map((p) => p.name)
    assert.deepStrictEqual(diverge.sort(), [
      'digit-separator-in-field-number',
      'exponent-field-number',
      'hash-line-comment',
      'underscore-suffixed-number-in-enum',
    ])
  })
})
