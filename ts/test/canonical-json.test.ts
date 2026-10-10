/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// Every descriptor row of the shared fixtures is the canonical JSON, byte
// for byte.
//
// The shared runner (parity.test.ts) compares a row after a JSON round
// trip, which ignores member order, and so do its Go and Rust halves. The
// ports also give the descriptor as a tree a host walks, in this
// object's member order (`descriptor_value` in Rust, `DescriptorValue` in
// Go), and their own tests (`rs/tests/value_test.rs`,
// `go/value_test.go`) compare that tree's JSON TEXT with each row's
// `expected` cell. That holds them to this runtime only while every cell
// is exactly what `JSON.stringify(parse(input, opts))` writes here, and
// this test is what keeps it so: a cell edited by hand into another order
// fails here first.

import { test } from 'node:test'
import assert from 'node:assert'

import { findSpecDir, loadSpecDir, isErrorExpect } from '@tabnas/support'

const { parse } = require('..')

test('every descriptor row is the canonical JSON.stringify output, byte for byte', () => {
  const failures: string[] = []
  let checked = 0
  for (const file of loadSpecDir(findSpecDir(__dirname))) {
    for (const row of file.rows) {
      const expected = row.named('expected')
      if (isErrorExpect(expected)) continue
      checked++
      const opts = row.named('opts')
      const got = JSON.stringify(
        parse(row.unescNamed('input'), '' === opts.trim() ? undefined : JSON.parse(opts)))
      if (got !== expected) {
        failures.push(`${row.where()}\n  got      ${got}\n  expected ${expected}`)
      }
    }
  }
  assert.deepEqual(failures, [], `${failures.length} of ${checked} rows differ`)
  // Ratcheted at what is on disk today, so a loader that finds fewer rows
  // cannot pass by measuring less.
  assert.equal(checked, 235,
    `test/spec holds ${checked} descriptor rows, not the 235 this test was measured against`)
})


// A statement that places a member keeps it present when it yields
// nothing: `extensions 1_0;` reads no range (`Number('1_0')` is NaN) and
// still assigns `extensionRange`, so the canonical JSON holds an empty
// list. The Go and Rust trees are held to these same bytes
// (go/value_test.go, rs/tests/value_test.rs); a struct whose empty list
// reads as absent cannot hold them, which is why they are tests there and
// not fixture rows.
test('a statement keeps its member when it yields nothing', () => {
  for (const [src, want] of [
    ['syntax = "proto2";\nmessage M { extensions 1_0; }\n',
      '{"dependency":[],"publicDependency":[],"weakDependency":[],"messageType":[{"name":"M","field":[],"nestedType":[],"enumType":[],"oneofDecl":[],"extension":[],"extensionRange":[]}],"enumType":[],"service":[],"extension":[],"syntax":"proto2"}'],
    ['syntax = "proto2";\nmessage M { reserved 1_0; option deprecated = true; extensions 1_0; reserved "a"; }\n',
      '{"dependency":[],"publicDependency":[],"weakDependency":[],"messageType":[{"name":"M","field":[],"nestedType":[],"enumType":[],"oneofDecl":[],"extension":[],"options":{"deprecated":true},"reservedRange":[],"extensionRange":[],"reservedName":["a"]}],"enumType":[],"service":[],"extension":[],"syntax":"proto2"}'],
    ['syntax = "proto2";\nenum E { A = 0; reserved 1_0; option allow_alias = true; }\n',
      '{"dependency":[],"publicDependency":[],"weakDependency":[],"messageType":[],"enumType":[{"name":"E","value":[{"name":"A","number":0}],"reservedRange":[],"options":{"allow_alias":true}}],"service":[],"extension":[],"syntax":"proto2"}'],
    ['syntax = "proto2";\nmessage M { reserved 0x10; }\n',
      '{"dependency":[],"publicDependency":[],"weakDependency":[],"messageType":[{"name":"M","field":[],"nestedType":[],"enumType":[],"oneofDecl":[],"extension":[],"reservedRange":[]}],"enumType":[],"service":[],"extension":[],"syntax":"proto2"}'],
  ]) {
    assert.equal(JSON.stringify(parse(src)), want, src)
  }
})
