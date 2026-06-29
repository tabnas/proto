/* Copyright (c) 2025 Richard Rodger and other contributors, MIT License */

// Performance regression guard. Mirrors go/perf_test.go
// (TestParseReusesInstance).
//
// zon has NO convenience parse() entry point on the TS side: it is a plugin
// users install themselves (new Tabnas().use(jsonic).use(Zon)). So there is
// nothing for the module to cache — the regression we can guard is the
// *usage*: build ONE instance and reuse it for many parses, never rebuilding
// the (expensive) engine + grammar per parse. Building the grammar dominates a
// parse.
//
// The check is machine-INDEPENDENT: it compares reuse against a single parse
// and against the rebuild-per-parse anti-pattern on the SAME machine in the
// SAME run, so a slow CI box cannot make it flaky (everything scales
// together). There is deliberately NO absolute wall-clock budget.

import { test, describe } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '../dist/zon'

const SRC = '.{ .a = 1, .b = "x", .c = .{ 1, 2, 3 } }' // tiny representative ZON value
const N = 2000

describe('perf', () => {
  test('reusing one instance stays linear and beats rebuild-per-parse', () => {
    // Build the reusable instance once (the expensive step).
    const j = new Tabnas().use(jsonic).use(Zon)

    // Warm the reuse path so the comparison is steady-state, and sanity-check
    // the parse result en route.
    for (let i = 0; i < 100; i++) {
      assert.deepEqual(j.parse(SRC), { a: 1, b: 'x', c: [1, 2, 3] })
    }

    // Time one isolated (already-warmed) parse on the reused instance.
    let t0 = process.hrtime.bigint()
    j.parse(SRC)
    const single = Number(process.hrtime.bigint() - t0)

    // Time N parses reusing the ONE instance.
    t0 = process.hrtime.bigint()
    for (let i = 0; i < N; i++) {
      j.parse(SRC)
    }
    const reuse = Number(process.hrtime.bigint() - t0)

    // Time N parses that REBUILD a fresh instance every call — the
    // anti-pattern this guards against.
    t0 = process.hrtime.bigint()
    for (let i = 0; i < N; i++) {
      const rj = new Tabnas().use(jsonic).use(Zon)
      rj.parse(SRC)
    }
    const rebuild = Number(process.hrtime.bigint() - t0)

    const avgReuse = reuse / N

    // 1) Reuse must stay (near) linear: amortized per-parse time over N reused
    //    parses should be within a small factor of a single warmed parse.
    //    Allow 4x for scheduling / timer noise on a tiny input.
    if (single > 0) {
      assert.ok(
        avgReuse <= 4 * single,
        `reuse is not staying linear: ${N} reused parses took ${(reuse / 1e6).toFixed(2)}ms ` +
          `(avg ${(avgReuse / 1e3).toFixed(2)}us/parse) vs ${(single / 1e3).toFixed(2)}us for a ` +
          `single parse (ratio ${(avgReuse / single).toFixed(1)}x, limit 4x)`,
      )
    }

    // 2) Reuse must be dramatically faster than rebuilding per parse. Building
    //    the grammar dominates, so rebuild-per-parse is many times slower than
    //    reuse; requiring >4x both documents the win and would FAIL if a
    //    future change made representative usage rebuild on every parse.
    assert.ok(
      rebuild >= 4 * reuse,
      `rebuild-per-parse is not dominated by reuse as expected: ` +
        `rebuild=${(rebuild / 1e6).toFixed(2)}ms reuse=${(reuse / 1e6).toFixed(2)}ms ` +
        `(ratio ${(rebuild / reuse).toFixed(1)}x, expected >4x). Building the grammar ` +
        `should dominate — reuse a single instance.`,
    )

    console.log(
      `[perf] single=${(single / 1e3).toFixed(2)}us  ` +
        `reuse(N=${N})=${(reuse / 1e6).toFixed(2)}ms avg=${(avgReuse / 1e3).toFixed(2)}us  ` +
        `rebuild(N=${N})=${(rebuild / 1e6).toFixed(2)}ms  ` +
        `reuse/single=${(avgReuse / Math.max(single, 1)).toFixed(2)}x  ` +
        `rebuild/reuse=${(rebuild / reuse).toFixed(1)}x`,
    )
  })
})
