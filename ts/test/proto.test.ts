/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

import { describe, it } from 'node:test'
import assert from 'node:assert'

const { parse, toDescriptor, Proto } = require('..')
const { Tabnas } = require('@tabnas/parser')

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
    // A named type is unresolved here, so `type` is left unset (as protoc
    // does before its resolution pass) and only `typeName` is recorded.
    assert.equal(scores.type, undefined)
    assert.equal(scores.typeName, 'ScoresEntry')
    const entry = fdp.messageType[0].nestedType.find((m: any) => m.name === 'ScoresEntry')
    assert.equal(entry.options.mapEntry, true)
    assert.deepEqual(entry.field.map((x: any) => [x.name, x.type]),
      [['key', 'TYPE_STRING'], ['value', 'TYPE_INT32']])
  })

  it('records oneof declarations and back-references', () => {
    const m = fdp.messageType[0]
    // `contact` is declared; `_age` is the synthetic oneof protoc adds for
    // the proto3 explicit `optional int32 age`, appended after it.
    assert.deepEqual(m.oneofDecl.map((o: any) => o.name), ['contact', '_age'])
    const email = m.field.find((x: any) => x.name === 'email')
    assert.equal(email.oneofIndex, 0)
    assert.equal(email.type, 'TYPE_STRING')
    const age = m.field.find((x: any) => x.name === 'age')
    assert.equal(age.oneofIndex, 1)
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
    // `default` is a pseudo-option: protoc lifts it to `defaultValue`.
    const name = f.find((x: any) => x.name === 'name')
    assert.equal(name.defaultValue, 'x')
    assert.equal(name.options, undefined)
    const bars = f.find((x: any) => x.name === 'bars')
    assert.deepEqual([bars.label, bars.type, bars.typeName],
      ['LABEL_REPEATED', undefined, 'Bar'])
  })

  it('expands a group into a field plus a nested message', () => {
    const g = fdp.messageType[0].field.find((x: any) => x.name === 'mygroup')
    assert.deepEqual([g.number, g.label, g.type, g.typeName],
      [4, 'LABEL_OPTIONAL', 'TYPE_GROUP', 'MyGroup'])
    const nested = fdp.messageType[0].nestedType.find((m: any) => m.name === 'MyGroup')
    assert.deepEqual(nested.field.map((x: any) => x.name), ['a'])
  })

  it('records extension ranges and top-level extend', () => {
    // `end` is exclusive, as in descriptor.proto: `100 to 199` -> [100, 200).
    assert.deepEqual(fdp.messageType[0].extensionRange, [{ start: 100, end: 200 }])
    assert.equal(fdp.extension[0].name, 'ext')
    assert.equal(fdp.extension[0].number, 100)
    assert.equal(fdp.extension[0].extendee, 'Foo')
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
    // `import option` is its own dependency list, not a plain import.
    assert.deepEqual(fdp.dependency, [])
    assert.deepEqual(fdp.optionDependency, ['custom.proto'])
    assert.equal(fdp.messageType[0].name, 'Pub')
    assert.equal(fdp.messageType[0].visibility, 'VISIBILITY_EXPORT')
    assert.equal(fdp.messageType[1].enumType[0].name, 'E')
    assert.equal(fdp.messageType[1].enumType[0].visibility, 'VISIBILITY_LOCAL')
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

describe('aggregate option values', () => {
  // protoc records the text between the braces, comments turned to the
  // spaces and newlines that keep each token where it was (protoc 36.2's
  // parser gives exactly this string for this source).
  const src =
    'message M {\n  option (f) = {\n    a: 1 // one\n    b { c: "x" /* two */ }\n  };\n}\n'
  const text = '\n    a: 1 \n    b { c: "x"           }\n  '

  // The rules under the entry whose `src` is `entry`: the CST shape of
  // one value inside the braces.
  const entryKids = (n: any, entry: string): any =>
    'messageValueEntry' === n?.rule && entry === n.src
      ? n.kids.map((k: any) => k.rule)
      : (n?.kids || []).map((k: any) => entryKids(k, entry)).find((k: any) => k)

  it('records the text between the braces, as protoc does', () => {
    assert.equal(parse(src).messageType[0].options['(f)'], text)
  })

  it('records the same text through toDescriptor on a reused engine', () => {
    // The walk has only the CST, so the text has to be on the CST: the
    // plugin puts it on the aggregate's `constant` node as `aggregate`,
    // and leaves `src` the tokens run together, as on every node.
    const tn = new Tabnas({ rewind: { history: 8192 } }).use(Proto)
    const cst = tn.parse(src)
    assert.equal(toDescriptor(cst).messageType[0].options['(f)'], text)
    const find = (n: any): any =>
      'constant' === n?.rule ? n : (n?.kids || []).map(find).find((k: any) => k)
    const node = find(cst)
    assert.equal(node.src, '{a:1b{c:"x"}}')
    assert.equal(node.aggregate, text)
    // A value inside the braces is a `constant`, a single string included.
    assert.deepEqual(entryKids(cst, 'c:"x"'), ['constant'])
  })

  it('takes a string value written as adjacent literals', () => {
    const adjacent = 'message M { option (f) = { a: "x" "y" }; }'
    const fdp = parse(adjacent)
    assert.equal(fdp.messageType[0].options['(f)'], ' a: "x" "y" ')
    // Two or more literals are the entry's `strLit` children, one each.
    const tn = new Tabnas({ rewind: { history: 8192 } }).use(Proto)
    assert.deepEqual(entryKids(tn.parse(adjacent), 'a:"x""y"'), ['strLit', 'strLit'])
  })
})

describe('text format inside an aggregate', () => {
  const tn = new Tabnas({ rewind: { history: 8192 } }).use(Proto)
  const kids = (n: any, rule: string, src: string): any =>
    rule === n?.rule && src === n.src
      ? n.kids.map((k: any) => k.rule)
      : (n?.kids || []).map((k: any) => kids(k, rule, src)).find((k: any) => k)

  it('reads a list and a message in angle brackets as nodes of their own', () => {
    const cst = tn.parse('option (f) = { a: [1, "x" "y"] b < c: 1 > };')
    assert.deepEqual(kids(cst, 'messageValueEntry', 'a:[1,"x""y"]'), ['listValue'])
    assert.deepEqual(kids(cst, 'listValue', '[1,"x""y"]'), ['constant', 'constant'])
    assert.deepEqual(kids(cst, 'messageValueEntry', 'b<c:1>'), ['angleValue'])
    assert.deepEqual(kids(cst, 'angleValue', '<c:1>'), ['messageValueEntry'])
  })

  it('reads a keyword, and a bracketed name, as an identifier inside the braces only', () => {
    const src = 'option (f) = { message: optional [ x . y ]: 1 };\nmessage M { optional int32 a = 1; }'
    const cst = tn.parse(src)
    // The bracketed name is one word, its spaces left out as every
    // node's `src` leaves them out.
    assert.deepEqual(kids(cst, 'messageValueEntry', '[x.y]:1'), ['constant'])
    const fdp = toDescriptor(cst)
    assert.equal(fdp.options['(f)'], ' message: optional [ x . y ]: 1 ')
    assert.equal(fdp.messageType[0].name, 'M')
    // Outside an aggregate a keyword is a keyword, as it was in 0.5.0.
    assert.throws(() => parse('option (f) = max;'))
  })

  it('takes a list of messages after a bracketed name without a colon', () => {
    const cst = tn.parse('option (f) = { [x.y] [ { a: 1 } ] true [] b: true [x.z]: 1 };')
    assert.deepEqual(kids(cst, 'messageValueEntry', '[x.y][{a:1}]'), ['listValue'])
    assert.deepEqual(kids(cst, 'messageValueEntry', 'true[]'), ['listValue'])
    // A value with a bracketed name after it is still the entry's value.
    assert.deepEqual(kids(cst, 'messageValueEntry', 'b:true'), ['constant'])
    assert.deepEqual(kids(cst, 'messageValueEntry', '[x.z]:1'), ['constant'])
    // The name is read as protoc reads it. Its tokenizer refuses a
    // decimal point with a digit after it directly behind an identifier,
    // and a number that runs into a letter; text format then joins what
    // is left, so `[x.y 2]` names `x.y2`.
    assert.throws(() => parse('option (f) = { [a.2/x.Y] {} };'))
    assert.throws(() => parse('option (f) = { [1p/x.Y] {} };'))
    assert.equal(parse('option (f) = { [x.y 2]: 1 };').options['(f)'], ' [x.y 2]: 1 ')
  })

  it('knows every word the grammar spells as a literal, bar export and local', () => {
    const { grammarText } = require('../dist/grammar')
    const { KEYWORDS } = require('../dist/aggregate')
    const words = new Set<string>()
    for (const line of grammarText.split('\n')) {
      // A rule line ends at the first `;` outside a quoted literal.
      let body = ''
      let quoted = false
      for (const c of line) {
        if ('"' === c) quoted = !quoted
        else if (';' === c && !quoted) break
        body += c
      }
      for (const m of body.matchAll(/"([A-Za-z_][A-Za-z0-9_]*)"/g)) words.add(m[1].toLowerCase())
    }
    words.delete('export')
    words.delete('local')
    assert.deepEqual([...KEYWORDS].sort(), [...words].sort())
  })
})

describe('string literals', () => {
  it('records one literal as written and adjacent literals as protoc does', () => {
    // protoc decodes both. One literal is kept as written, escapes and
    // all, as 0.5.0 kept it; adjacent literals, which 0.5.0 refused, are
    // recorded as protoc records them: decoded and concatenated.
    const fdp = parse('option (f) = "\\x41"; option (g) = "\\x41" "";\nimport "a\\x41";\nimport "a" "\\x41";')
    assert.equal(fdp.options['(f)'], '\\x41')
    assert.equal(fdp.options['(g)'], 'A')
    assert.deepEqual(fdp.dependency, ['a\\x41', 'aA'])
  })

  it('refuses adjacent literals protoc refuses, and nothing 0.5.0 took', () => {
    // protoc's tokenizer refuses `\e` in any literal. One literal holding
    // it is kept as written, as 0.5.0 kept it, and so is an aggregate,
    // whose text is recorded as written; adjacent literals are refused.
    assert.equal(parse('option (f) = "\\e";').options['(f)'], '\\e')
    assert.equal(parse('option (f) = { a: "\\e" "x" };').options['(f)'], ' a: "\\e" "x" ')
    assert.throws(() => parse('option (f) = "\\e" "x";'), /unexpected/)
    assert.throws(() => parse('option (f) = "x" /* c */ `y`;'), /unexpected/)
    assert.throws(() => parse('import "a" "\\U0001" "F600";'), /unexpected/)
  })
})

describe('telling inside an aggregate from outside', () => {
  const tn = new Tabnas({ rewind: { history: 8192 } }).use(Proto)

  // Best of three, in milliseconds.
  const time = (src: string): number => {
    let best = Infinity
    for (let i = 0; i < 3; i++) {
      const start = performance.now()
      tn.parse(src)
      best = Math.min(best, performance.now() - start)
    }
    return best
  }

  it('asks only the rule at hand, not the rules above it', () => {
    // The answer once came from walking up the rule stack, which every
    // top-level definition and every aggregate entry deepens, so each
    // keyword cost more than the one before it. Now the rule is marked.
    const { inAggregate } = require('../dist/aggregate')
    const lex = { ctx: { t: [] } }
    const rule = (name: string, keep: any) => ({
      name,
      o0: { src: 'x' },
      rawk: () => keep,
      get parent(): any {
        throw new Error('walked up the rule stack')
      },
    })
    assert.equal(inAggregate(lex, rule('messageValueEntry', { protoAggregate: true })), true)
    assert.equal(inAggregate(lex, rule('field', undefined)), false)
    assert.equal(inAggregate({ ctx: { t: [{ src: '{' }] } }, rule('constant', undefined)), true)
  })

  it('reads a keyword inside an aggregate as fast as any other word', () => {
    // On the walk this replaced, 4,000 `message: 1` entries, or `message <`
    // nested 2,000 deep, took longer than the same shape spelt `abcdefg`,
    // and the gap grew with the square of the size. Now the two spellings
    // cost the same. The comparison is between spellings of one shape, so
    // it holds on any machine and whatever the engine's own curve.
    const flat = (word: string) => 'option (f) = {' + ` ${word}: 1`.repeat(4000) + ' };'
    const angle = (word: string) =>
      'option (f) = {' + ` ${word} <`.repeat(2000) + ' >'.repeat(2000) + ' };'
    for (const shape of [flat, angle]) {
      const keyword = time(shape('message'))
      const plain = time(shape('abcdefg'))
      assert.ok(
        keyword < 1.5 * plain + 50,
        `${keyword.toFixed(0)} ms with a keyword, against ${plain.toFixed(0)} ms without`,
      )
    }
  })
})
