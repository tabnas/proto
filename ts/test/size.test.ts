/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The size of the compiled grammar is a contract (tabnas/bnf#71,
// docs/design/alt-explosion.md section 9.5): admitting every keyword as
// an identifier used to multiply the dispatch tables into the millions.
// Per-decision lookahead and the `ident` token class hold it to a few
// hundred alternates. Go and Rust pin the same bounds.

import { describe, it } from 'node:test'
import assert from 'node:assert'

const { Proto } = require('..')
const { Tabnas } = require('@tabnas/parser')

describe('size', () => {
  it('compiles to a few hundred alternates and a handful per decision', () => {
    const tn = new Tabnas({ rewind: { history: 8192 } })
    tn.use(Proto)
    const rules = Object.entries(tn.rule()) as Array<[string, any]>
    let total = 0
    let biggest: [string, number] = ['', 0]
    for (const [name, rs] of rules) {
      const n = (rs.def.open || []).length
      total += n
      if (n > biggest[1]) biggest = [name, n]
    }
    assert.ok(rules.length <= 500, `${rules.length} rules`)
    assert.ok(total <= 1000, `${total} open alternates`)
    assert.ok(biggest[1] <= 60, `${biggest[0]} has ${biggest[1]} open alternates`)
    // The identifier class is one engine token set.
    assert.ok(Array.isArray(tn.internal().config.tokenSet.ident), 'ident token set')
  })
})
