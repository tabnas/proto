/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

import { describe, it } from 'node:test'
import assert from 'node:assert'

const { parse } = require('..')

describe('version detection', () => {
  it('detects proto2 / proto3 from the syntax declaration', () => {
    assert.equal(parse('syntax = "proto2";').syntax, 'proto2')
    assert.equal(parse('syntax = "proto3";').syntax, 'proto3')
  })

  it('detects edition 2023 / 2024 and records the edition', () => {
    const e23 = parse('edition = "2023";')
    assert.equal(e23.edition, 'EDITION_2023')
    assert.equal(e23.syntax, undefined)
    const e24 = parse('edition = "2024";')
    assert.equal(e24.edition, 'EDITION_2024')
  })

  it('uses the explicit option when there is no declaration', () => {
    assert.equal(parse('message M {}', { version: 'proto3' }).syntax, 'proto3')
    assert.equal(parse('message M {}', { version: '2024' }).edition, 'EDITION_2024')
  })

  it('defaults to proto2 when neither declaration nor option is given', () => {
    assert.equal(parse('message M {}').syntax, 'proto2')
  })

  it('throws on a mismatch between option and declaration (reconcile=true)', () => {
    assert.throws(
      () => parse('syntax = "proto3";', { version: 'proto2' }),
      /version mismatch/,
    )
  })

  it('lets the declaration win when reconcile=false', () => {
    const fdp = parse('syntax = "proto3";', { version: 'proto2', reconcile: false })
    assert.equal(fdp.syntax, 'proto3')
  })
})
