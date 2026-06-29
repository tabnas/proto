/* Copyright (c) 2025 Richard Rodger and other contributors, MIT License */

// Composition test: the ZON grammar plugin layered with the official
// @tabnas/debug plugin. @tabnas/debug is a devDependency, but this still
// resolves it dynamically and SKIPS when it is absent so the suite stays
// runnable outside the package; TABNAS_DEBUG_PATH can point at a sibling
// checkout's built plugin.

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '../dist/zon'

function loadDebug(): any {
  const candidates = [process.env.TABNAS_DEBUG_PATH, '@tabnas/debug'].filter(
    Boolean,
  ) as string[]
  for (const c of candidates) {
    try {
      return require(c).Debug
    } catch {
      /* try next */
    }
  }
  return null
}

const Debug = loadDebug()
const skip = Debug
  ? false
  : '@tabnas/debug not available (set TABNAS_DEBUG_PATH)'

function build(): any {
  const tn = new Tabnas().use(jsonic).use(Zon, {})
  tn.use(Debug, { print: false, trace: false })
  return tn
}

describe('compose: zon + @tabnas/debug', () => {
  test('parses normally with the debug plugin installed', { skip }, () => {
    const tn = build()
    assert.deepStrictEqual(
      JSON.parse(JSON.stringify(tn.parse('.{ .a = 1, .b = .{ 2, 3 } }'))),
      { a: 1, b: [2, 3] },
    )
  })

  test('debug.model() returns the structured zon grammar', { skip }, () => {
    const tn = build()
    const m = tn.debug.model()

    // The structured rule set and entry rule.
    assert.deepStrictEqual(
      m.rules.map((r: any) => r.name).sort(),
      ['elem', 'list', 'map', 'pair', 'val'],
    )
    assert.equal(m.config.start, 'val')
    assert.ok(
      m.plugins.some((p: any) => p.name === 'Zon'),
      'plugins should list Zon',
    )

    // val is a choice whose open alts push both the map and list rules:
    // ZON's `.{ ... }` syntax is disambiguated into struct (map) vs tuple
    // (list) by what follows the opening brace.
    const val = m.rules.find((r: any) => r.name === 'val')
    assert.ok(
      val.open.some((a: any) => a.push === 'map'),
      'val should push map',
    )
    assert.ok(
      val.open.some((a: any) => a.push === 'list'),
      'val should push list',
    )

    // The rule-reference graph captures the recursive collection structure:
    // map -> pair, list -> elem, and pair/elem each close-replace themselves
    // to iterate over additional members.
    const edge = (name: string) => m.graph.find((e: any) => e.name === name)
    assert.deepStrictEqual(edge('val').openPush.slice().sort(), ['list', 'map'])
    assert.deepStrictEqual(edge('map').openPush, ['pair'])
    assert.deepStrictEqual(edge('list').openPush, ['elem'])
    assert.deepStrictEqual(edge('pair').closeReplace, ['pair'])
    assert.deepStrictEqual(edge('elem').closeReplace, ['elem'])

    // The grammar portion is JSON-serialisable and round-trips.
    const grammar = {
      tokens: m.tokens,
      rules: m.rules,
      graph: m.graph,
      config: m.config,
      abnf: m.abnf,
    }
    assert.deepStrictEqual(JSON.parse(JSON.stringify(grammar)).rules, m.rules)
  })
})
