/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

import { describe, it } from 'node:test'
import assert from 'node:assert'

const { parse } = require('..')

describe('proto3', () => {
  const fdp = parse(`syntax = "proto3";
package demo;
import "google/protobuf/timestamp.proto";
import public "other.proto";
message Person {
  string name = 1;          // a line comment
  optional int32 age = 2;   /* a block comment */
  repeated string tags = 3;
  map<string, int32> scores = 4;
  oneof contact { string email = 5; string phone = 6; }
  message Address { string city = 1; }
  enum Kind { UNKNOWN = 0; ADMIN = 1; }
}
enum Status { reserved 2, 9 to 11; UNKNOWN = 0; ACTIVE = 1; }
service Dir { rpc Find (Person) returns (stream Person); }
`)

  it('records package, syntax and dependencies', () => {
    assert.equal(fdp.syntax, 'proto3')
    assert.equal(fdp.package, 'demo')
    assert.deepEqual(fdp.dependency, ['google/protobuf/timestamp.proto', 'other.proto'])
    assert.deepEqual(fdp.publicDependency, [1])
  })

  it('maps fields with types, numbers, labels', () => {
    const f = fdp.messageType[0].field
    assert.deepEqual(
      f.slice(0, 3).map((x: any) => [x.name, x.number, x.label, x.type]),
      [
        ['name', 1, 'LABEL_OPTIONAL', 'TYPE_STRING'],
        ['age', 2, 'LABEL_OPTIONAL', 'TYPE_INT32'],
        ['tags', 3, 'LABEL_REPEATED', 'TYPE_STRING'],
      ],
    )
  })

  it('sets proto3Optional on an explicit optional field', () => {
    const age = fdp.messageType[0].field.find((x: any) => x.name === 'age')
    assert.equal(age.proto3Optional, true)
  })

  it('synthesises a map entry message and a repeated message field', () => {
    const scores = fdp.messageType[0].field.find((x: any) => x.name === 'scores')
    assert.equal(scores.label, 'LABEL_REPEATED')
    assert.equal(scores.type, 'TYPE_MESSAGE')
    assert.equal(scores.typeName, 'ScoresEntry')
    const entry = fdp.messageType[0].nestedType.find((m: any) => m.name === 'ScoresEntry')
    assert.equal(entry.options.mapEntry, true)
    assert.deepEqual(entry.field.map((x: any) => [x.name, x.type]),
      [['key', 'TYPE_STRING'], ['value', 'TYPE_INT32']])
  })

  it('records oneof declarations and back-references', () => {
    const m = fdp.messageType[0]
    assert.deepEqual(m.oneofDecl.map((o: any) => o.name), ['contact'])
    const email = m.field.find((x: any) => x.name === 'email')
    assert.equal(email.oneofIndex, 0)
    assert.equal(email.type, 'TYPE_STRING')
  })

  it('captures nested messages and enums', () => {
    const m = fdp.messageType[0]
    assert.ok(m.nestedType.find((x: any) => x.name === 'Address'))
    assert.deepEqual(m.enumType[0].value.map((v: any) => [v.name, v.number]),
      [['UNKNOWN', 0], ['ADMIN', 1]])
  })

  it('captures enum reserved ranges and service streaming', () => {
    assert.deepEqual(fdp.enumType[0].reservedRange, [{ start: 2, end: 2 }, { start: 9, end: 11 }])
    const method = fdp.service[0].method[0]
    assert.equal(method.serverStreaming, true)
    assert.equal(method.clientStreaming, undefined)
    assert.equal(method.inputType, 'Person')
  })
})

describe('proto2', () => {
  const fdp = parse(`syntax = "proto2";
message Foo {
  required int32 id = 1;
  optional string name = 2 [default = "x"];
  repeated Bar bars = 3;
  extensions 100 to 199;
  group MyGroup = 4 { optional int32 a = 1; }
}
extend Foo { optional string ext = 100; }
`)

  it('handles required/optional/repeated labels and field options', () => {
    const f = fdp.messageType[0].field
    const id = f.find((x: any) => x.name === 'id')
    assert.equal(id.label, 'LABEL_REQUIRED')
    const name = f.find((x: any) => x.name === 'name')
    assert.equal(name.options.default, 'x')
    const bars = f.find((x: any) => x.name === 'bars')
    assert.deepEqual([bars.label, bars.type, bars.typeName],
      ['LABEL_REPEATED', 'TYPE_MESSAGE', 'Bar'])
  })

  it('records extension ranges and top-level extend', () => {
    assert.deepEqual(fdp.messageType[0].extensionRange, [{ start: 100, end: 199 }])
    assert.equal(fdp.extension[0].name, 'ext')
    assert.equal(fdp.extension[0].number, 100)
  })
})

describe('edition 2023', () => {
  const fdp = parse(`edition = "2023";
package e;
option features.field_presence = EXPLICIT;
message M { int32 a = 1 [features.field_presence = IMPLICIT]; }
`)
  it('records the edition and file/field options', () => {
    assert.equal(fdp.edition, 'EDITION_2023')
    assert.equal(fdp.options['features.field_presence'], 'EXPLICIT')
    const a = fdp.messageType[0].field[0]
    assert.equal(a.options['features.field_presence'], 'IMPLICIT')
  })
})

describe('edition 2024', () => {
  const fdp = parse(`edition = "2024";
import option "custom.proto";
export message Pub { int32 a = 1; }
message Outer { local enum E { A = 0; } }
`)
  it('parses import option, export/local symbol visibility', () => {
    assert.equal(fdp.edition, 'EDITION_2024')
    assert.deepEqual(fdp.dependency, ['custom.proto'])
    assert.equal(fdp.messageType[0].name, 'Pub')
    assert.equal(fdp.messageType[1].enumType[0].name, 'E')
  })
})

describe('whitespace and comments', () => {
  it('parses a comment/whitespace-heavy file the same as its minified form', () => {
    const pretty = parse(`syntax = "proto3";

// header
message  M  {
  int32   a   =   1 ;   // trailing
  /* block
     comment */
  repeated  string  b  =  2 ;
}
`)
    const mini = parse('syntax="proto3";message M{int32 a=1;repeated string b=2;}')
    assert.deepEqual(pretty.messageType, mini.messageType)
  })
})
