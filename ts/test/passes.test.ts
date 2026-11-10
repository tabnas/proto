/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The rule passes the transducer part (alchemy/transduce.alc) reads a
// statement by. The compiled grammar names each statement's alternative
// after the rule that dispatches it (`topLevelDef$alt4` is a message), in
// the order proto-grammar/*.abnf writes the alternatives, and the part
// knows a statement by that name; `test/passes/*.tsv` holds each source to
// the names its passes open, so a grammar that renumbers them fails here,
// in every runtime (`go/passes_test.go`, `rs/tests/passes_test.rs`), and
// not in a host's stream. Both parsers are held to it: `make()`'s and
// `makeStreaming()`'s, whose passes are the same (./streaming.test.ts). The
// rest of the part, every event of it, is held to the tree's walk where it
// runs, in a host that carries alchemy.
//
// Repetition is replacement (AGENTS.md): the items of a list, the
// declarations of a file, the fields of a message, the values of an enum,
// the fields of an aggregate and the options in brackets, add no rule
// depth however many there are.

import * as assert from 'node:assert/strict'
import * as path from 'node:path'
import { test } from 'node:test'

import { findSpecDir, makeRunner } from '@tabnas/support'

import { make, makeStreaming } from '../dist/proto'

const STATEMENT =
  /^((syntaxOrEdition|topLevelDef|messageElement|enumElement|oneofElement|serviceElement)\$alt\d+|field|optionStmt|fieldOption|messageValueEntry)$/

// The names of the statements' rules, in the order their open passes come,
// and `constant$alt5` where a constant opens an aggregate, on a parser
// `build` makes.
function statements(src: string, build: () => any): string[] {
  const tn = build()
  const names: string[] = []
  tn.sub({
    ruleDone: (rule: any, _ctx: any, done: any) => {
      if (done.probe || 'o' !== done.state) return
      if (STATEMENT.test(rule.name)) names.push(rule.name)
      if ('constant' === rule.name && 'constant$alt5' === done.alt?.p) names.push('constant$alt5')
    },
  })
  tn.parse(src)
  return names
}

makeRunner({
  parse: (input) => {
    const names = statements(input, make)
    assert.deepEqual(statements(input, makeStreaming), names, 'makeStreaming()')
    return JSON.stringify(names)
  },
  parseExpected: (expected) => expected,
}).dir(path.join(findSpecDir(__dirname), '..', 'passes'))

// The deepest rule a parse of `src` reaches.
function depth(src: string): number {
  const tn = make()
  let deepest = 0
  tn.sub({
    ruleDone: (rule: any, _ctx: any, done: any) => {
      if (!done.probe && rule.d > deepest) deepest = rule.d
    },
  })
  tn.parse(src)
  return deepest
}

const N = 10000
const repeated: Array<[string, (n: number) => string]> = [
  ['declarations of a file', (n) => 'message M {}\n'.repeat(n)],
  ['fields of a message', (n) => 'message M {\n' + Array.from({ length: n }, (_, i) => `  int32 f${i} = ${i + 1};\n`).join('') + '}\n'],
  ['values of an enum', (n) => 'enum E {\n' + Array.from({ length: n }, (_, i) => `  V${i} = ${i};\n`).join('') + '}\n'],
  ['fields of an aggregate', (n) => 'option (a) = {' + ' b: 1'.repeat(n) + ' };\n'],
  ['options in brackets', (n) => 'message M { int32 f = 1 [' + Array.from({ length: n }, (_, i) => `(o${i}) = 1`).join(', ') + ']; }\n'],
  ['ranges of a reserved statement', (n) => 'message M { reserved ' + Array.from({ length: n }, (_, i) => `${i + 1}`).join(', ') + '; }\n'],
]

for (const [what, doc] of repeated) {
  // Two items, since a list's first item can sit apart from its repeated
  // ones (`x *( "," x )`): the loop's own item is the second.
  test(`the ${what} add no rule depth: ${N} reach what two do`, () => {
    assert.equal(depth(doc(N)), depth(doc(2)), what)
  })
}
