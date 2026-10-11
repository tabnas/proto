/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The render, run: a descriptor written as a .proto file by the render the
// package carries (`translate().render`), through the `alchemy` command
// (tabnas/alchemy-cli), which this package does not depend on, then read
// back with this package's reader. Set TABNAS_ALCHEMY to the command's
// path to run these tests; without it they skip.
//
// The contract is the round trip: the text reads back as the descriptor
// it was written from. What no text a .proto file can hold carries is
// refused with TARGET_VALUE_UNREPRESENTABLE, never written as a file the
// reader refuses.

import * as assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { spawnSync } from 'node:child_process'
import { after, test } from 'node:test'

const { parse, translate } = require('..')

const ALCHEMY = process.env.TABNAS_ALCHEMY
const skip = ALCHEMY ? false : 'TABNAS_ALCHEMY does not name the alchemy command'

let dir: string | null = null
after(() => {
  if (dir) rmSync(dir, { recursive: true, force: true })
})

type Out = { text?: string, code?: string, message?: string }

// The render over a tree: `{ text }` when it writes one, and the failure
// the command reports (`{ code, message }`) when it does not.
function render(tree: unknown): Out {
  if (null == dir) {
    dir = mkdtempSync(path.join(tmpdir(), 'proto-render-'))
    writeFileSync(path.join(dir, 'render.alc'),
      translate().render.source + '\ndef export [input] (proto-render input)\n')
  }
  const input = path.join(dir, 'tree.json')
  writeFileSync(input, JSON.stringify(tree))
  const run = spawnSync(ALCHEMY as string, ['run', path.join(dir, 'render.alc'), input],
    { encoding: 'utf8', timeout: 120000 })
  if (0 === run.status) return { text: run.stdout }
  try {
    return JSON.parse(run.stderr)
  } catch (e) {
    return { code: 'NOT_JSON', message: `${run.stderr} ${run.error ?? ''}` }
  }
}

// A file read, written and read again: the same descriptor, member for
// member, and where `text` is given, that text.
function roundTrip(src: string, text?: string) {
  const tree = parse(src)
  const out = render(tree)
  assert.equal(typeof out.text, 'string', JSON.stringify(out))
  if (undefined !== text) assert.equal(out.text, text)
  assert.equal(JSON.stringify(parse(out.text as string)), JSON.stringify(tree))
}

function refuses(tree: unknown, what: RegExp) {
  const out = render(tree)
  assert.equal(out.code, 'TARGET_VALUE_UNREPRESENTABLE', JSON.stringify(out))
  assert.match(out.message as string, what)
}

test('the render writes a file that reads back as its descriptor', { skip }, () => {
  roundTrip('syntax = "proto2";\noption a = "x";\nmessage M {\n  optional string s = 1 [default = "y"];\n}\n',
    'syntax = "proto2";\noption a = "x";\nmessage M {\n  optional string s = 1 [default = "y"];\n}\n')
})

// A backtick string is kept with its backticks, as written, and the lexer
// reads one across lines; no quotes hold a line break, so one holding a
// line break is written as itself. As an aggregate, an option's had read
// back as an error, and quoted, a default's, an import's and a json_name's.
test('the render writes a backtick string holding a line break as itself', { skip }, () => {
  roundTrip('syntax = "proto2";\noption a = `x\ny`;\n', 'syntax = "proto2";\noption a = `x\ny`;\n')
  roundTrip('syntax = "proto2";\nimport `x\ny`;\n')
  roundTrip('syntax = "proto2";\nmessage M {\n  optional string s = 1 [default = `x\ny`, json_name = `j\nk`, (o) = `p\rq`];\n}\n')
  roundTrip('syntax = "proto2";\nenum E {\n  A = 0 [(o) = `x\ny`];\n}\n')
  // test/spec/nesting.tsv: braces in a backtick string, across lines
  roundTrip('syntax = "proto2";\noption a = `{{{\n}`;\n')
  // one with no line break is written quoted, which reads back the same
  roundTrip('syntax = "proto2";\noption a = `x`;\n', 'syntax = "proto2";\noption a = "`x`";\n')
  // a backtick escaped inside one, and a backslash escaped before its end
  roundTrip('syntax = "proto2";\noption a = `a\\`b\nc`;\n', 'syntax = "proto2";\noption a = `a\\`b\nc`;\n')
  roundTrip('syntax = "proto2";\noption a = `x\\\\\ny\\\\`;\n', 'syntax = "proto2";\noption a = `x\\\\\ny\\\\`;\n')
})

// An option's value holding both quotes, from a literal, escapes the
// quotes it is between, so it is written between them; as an aggregate it
// had read back as an error. An escaped backslash escapes no quote after
// it, which had been taken for an escaped quote and written between the
// quotes it does not escape. An aggregate's text, its quotes unescaped,
// stays an aggregate.
test('the render writes a text holding both quotes between the quotes it escapes', { skip }, () => {
  roundTrip('syntax = "proto2";\noption a = "a\\"b\'c";\n', 'syntax = "proto2";\noption a = "a\\"b\'c";\n')
  roundTrip('syntax = "proto2";\noption a = \'a"b\\\'c\';\n', 'syntax = "proto2";\noption a = \'a"b\\\'c\';\n')
  roundTrip('syntax = "proto2";\noption a = \'p\\\\"q\\\'r\';\n', 'syntax = "proto2";\noption a = \'p\\\\"q\\\'r\';\n')
  roundTrip('syntax = "proto2";\noption a = "p\\\\\'q\\"r";\n', 'syntax = "proto2";\noption a = "p\\\\\'q\\"r";\n')
  roundTrip('syntax = "proto2";\nmessage M {\n  optional string s = 1 [default = \'a"b\\\'c\'];\n}\n')
  roundTrip('syntax = "proto2";\noption a = { a: "x"\n c: \'y\' };\n')
})

// The reader takes a reserved name from a quoted literal or an
// identifier, and reads the words of a backtick string as names of their
// own, so a reserved name is written between quotes, those it escapes
// where it holds both, and one that no quotes hold is refused.
test('the render writes a reserved name between quotes that hold it', { skip }, () => {
  roundTrip('syntax = "proto2";\nmessage M {\n  reserved \'a"b\\\'c\', "d\\"e";\n}\n',
    'syntax = "proto2";\nmessage M {\n  reserved \'a"b\\\'c\', \'d\\"e\';\n}\n')
  const tree = parse('syntax = "proto2";\nmessage M {\n  reserved "a";\n}\n')
  for (const name of ['`x\ny`', 'x"y\'z', 'x\\']) {
    const copy = JSON.parse(JSON.stringify(tree))
    copy.messageType[0].reservedName = [name]
    refuses(copy, /a reserved name holds what no quotes hold/)
  }
})

// No literal holds a control character, a backtick string only a line
// break, and an aggregate only the tab and the line breaks between its
// tokens; no quotes hold a text with both quotes unescaped, or one ending
// in a backslash that would escape the closing quote. A tree a program
// built with such a string is refused, where it had been written as a
// file the reader refuses or reads as another.
test('the render refuses a string that nothing holds where it goes', { skip }, () => {
  const tree = parse('syntax = "proto2";\noption a = "x";\nmessage M {\n  optional string s = 1 [default = "y"];\n}\n')
  refuses({ ...tree, options: { a: 'x\u0001y' } },
    /an option's value holds a control character that no literal, backtick string or aggregate holds/)
  for (const v of ['x\ty', 'x\ny', 'x"y\'z', 'x\\', '`x\ty`', '`x\ny\\`']) {
    const copy = JSON.parse(JSON.stringify(tree))
    copy.messageType[0].field[0].defaultValue = v
    refuses(copy, /a string holds what no literal or backtick string holds/)
  }
  const named = JSON.parse(JSON.stringify(tree))
  named.messageType[0].field[0].jsonName = 'x"y\'z'
  refuses(named, /a string holds what no literal or backtick string holds/)
})
