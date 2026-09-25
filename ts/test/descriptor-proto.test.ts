/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// protobuf's own descriptor.proto (test/descriptor/README.md): the one
// real-world file that names fields after nearly every keyword and
// declares `enum Edition`. Go and Rust check the same facts.

import { describe, it } from 'node:test'
import assert from 'node:assert'
import fs from 'node:fs'
import path from 'node:path'

const { parse } = require('..')

const SRC = fs.readFileSync(
  path.resolve(__dirname, '..', '..', 'test', 'descriptor', 'descriptor.proto'), 'utf8')

// Parsed inside the leaf tests, never in the describe body: a throw
// there counts zero failed tests and exits 0 (test/AGENTS.md, "Harness
// rules"), so a grammar that stopped reading this file would go green.
// Memoised, so the file is parsed once per run.
let cached: any
function fdp(): any {
  if (undefined === cached) cached = parse(SRC)
  return cached
}

describe('descriptor.proto', () => {
  it('is a proto2 file in package google.protobuf', () => {
    assert.equal(fdp().syntax, 'proto2')
    assert.equal(fdp().package, 'google.protobuf')
  })

  it('declares every message, enum and field', () => {
    assert.deepEqual(fdp().messageType.map((m: any) => m.name), [
      'FileDescriptorSet', 'FileDescriptorProto', 'DescriptorProto',
      'ExtensionRangeOptions', 'FieldDescriptorProto', 'OneofDescriptorProto',
      'EnumDescriptorProto', 'EnumValueDescriptorProto', 'ServiceDescriptorProto',
      'MethodDescriptorProto', 'FileOptions', 'MessageOptions', 'FieldOptions',
      'OneofOptions', 'EnumOptions', 'EnumValueOptions', 'ServiceOptions',
      'MethodOptions', 'UninterpretedOption', 'FeatureSet', 'FeatureSetDefaults',
      'SourceCodeInfo', 'GeneratedCodeInfo',
    ])
    assert.deepEqual(fdp().enumType.map((e: any) => e.name), ['Edition', 'SymbolVisibility'])
    assert.equal(fdp().messageType.reduce((n: number, m: any) => n + m.field.length, 0), 143)
    assert.equal(fdp().messageType.reduce((n: number, m: any) => n + m.nestedType.length, 0), 12)
  })

  it('reads the fields named after keywords', () => {
    const file = fdp().messageType.find((m: any) => m.name === 'FileDescriptorProto')
    assert.deepEqual(file.field.map((f: any) => f.name), [
      'name', 'package', 'dependency', 'public_dependency', 'weak_dependency',
      'option_dependency', 'message_type', 'enum_type', 'service', 'extension',
      'options', 'source_code_info', 'syntax', 'edition',
    ])
    const weak = fdp().messageType.find((m: any) => m.name === 'FieldOptions')
      .field.find((f: any) => f.name === 'weak')
    assert.deepEqual(weak, {
      name: 'weak', number: 10, label: 'LABEL_OPTIONAL', type: 'TYPE_BOOL',
      defaultValue: 'false', options: { deprecated: true },
    })
  })

  it('reads enum Edition, whose name folds to a keyword', () => {
    const edition = fdp().enumType.find((e: any) => e.name === 'Edition')
    assert.equal(edition.value.length, 14)
    assert.deepEqual(edition.value.slice(0, 3).map((v: any) => [v.name, v.number]), [
      ['EDITION_UNKNOWN', 0], ['EDITION_LEGACY', 900], ['EDITION_PROTO2', 998],
    ])
  })
})
