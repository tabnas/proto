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
import { spawn } from 'node:child_process'
import { after, test } from 'node:test'

const { parse, translate } = require('..')

const ALCHEMY = process.env.TABNAS_ALCHEMY
const skip = ALCHEMY ? false : 'TABNAS_ALCHEMY does not name the alchemy command'

// Every transient task reports progress at least every 30 seconds
// (AGENTS.md), so the command runs asynchronously, and while a run is out
// a line names it and the time it has taken, every BEAT_MS. A run takes
// under a second; LIMIT_MS only stops a command that has hung.
const BEAT_MS = 10000
const LIMIT_MS = 120000

let dir: string | null = null
let runs = 0
let current: { n: number, what: string, start: number } | null = null
let beat: ReturnType<typeof setInterval> | null = null
after(() => {
  if (beat) clearInterval(beat)
  if (dir) rmSync(dir, { recursive: true, force: true })
})

type Out = { text?: string, code?: string, message?: string }

// A text short enough to name a run by, on one line.
function brief(text: string): string {
  const s = text.replace(/\s+/g, ' ').trim()
  return 48 < s.length ? s.slice(0, 47) + '…' : s
}

// The render over a tree: `{ text }` when it writes one, and the failure
// the command reports (`{ code, message }`) when it does not.
function render(what: string, tree: unknown): Promise<Out> {
  if (null == dir) {
    dir = mkdtempSync(path.join(tmpdir(), 'proto-render-'))
    writeFileSync(path.join(dir, 'render.alc'),
      translate().render.source + '\ndef export [input] (proto-render input)\n')
  }
  if (null == beat) {
    beat = setInterval(() => {
      if (current) {
        console.log(`render ${current.n}, ${current.what}: ` +
          `${Math.round((Date.now() - current.start) / 1000)}s so far, percentage unknown`)
      }
    }, BEAT_MS)
    beat.unref()
  }
  const input = path.join(dir, 'tree.json')
  writeFileSync(input, JSON.stringify(tree))
  current = { n: ++runs, what, start: Date.now() }
  const run = spawn(ALCHEMY as string, ['run', path.join(dir, 'render.alc'), input],
    { stdio: ['ignore', 'pipe', 'pipe'] })
  let stdout = ''
  let stderr = ''
  run.stdout.setEncoding('utf8').on('data', (d: string) => { stdout += d })
  run.stderr.setEncoding('utf8').on('data', (d: string) => { stderr += d })
  let stopped = false
  const stop = setTimeout(() => { stopped = true; run.kill('SIGKILL') }, LIMIT_MS)
  return new Promise((resolve) => {
    const done = (out: Out) => {
      clearTimeout(stop)
      current = null
      resolve(out)
    }
    run.on('error', (e) => done({ code: 'NOT_RUN', message: String(e) }))
    run.on('close', (status, signal) => {
      if (0 === status) return done({ text: stdout })
      if (stopped) {
        return done({ code: 'STOPPED', message: `${what}: stopped after ${LIMIT_MS / 1000}s` })
      }
      try {
        done(JSON.parse(stderr))
      } catch (e) {
        done({ code: 'NOT_JSON', message: `${stderr} ${signal ?? ''}` })
      }
    })
  })
}

// A file read, written and read again: the same descriptor, member for
// member, and where `text` is given, that text.
async function roundTrip(src: string, text?: string) {
  const tree = parse(src)
  const out = await render(`the round trip of ${brief(src)}`, tree)
  assert.equal(typeof out.text, 'string', JSON.stringify(out))
  if (undefined !== text) assert.equal(out.text, text)
  assert.equal(JSON.stringify(parse(out.text as string)), JSON.stringify(tree))
}

async function refuses(what: string, tree: unknown, message: RegExp) {
  const out = await render(`the refusal of ${what}`, tree)
  assert.equal(out.code, 'TARGET_VALUE_UNREPRESENTABLE', JSON.stringify(out))
  assert.match(out.message as string, message)
}

test('the render writes a file that reads back as its descriptor', { skip }, async () => {
  await roundTrip('syntax = "proto2";\noption a = "x";\nmessage M {\n  optional string s = 1 [default = "y"];\n}\n',
    'syntax = "proto2";\noption a = "x";\nmessage M {\n  optional string s = 1 [default = "y"];\n}\n')
})

// A backtick string is kept with its backticks, as written, and the lexer
// reads one across lines; no quotes hold a line break, so one holding a
// line break is written as itself. As an aggregate, an option's had read
// back as an error, and quoted, a default's, an import's and a json_name's.
test('the render writes a backtick string holding a line break as itself', { skip }, async () => {
  await roundTrip('syntax = "proto2";\noption a = `x\ny`;\n', 'syntax = "proto2";\noption a = `x\ny`;\n')
  await roundTrip('syntax = "proto2";\nimport `x\ny`;\n')
  await roundTrip('syntax = "proto2";\nmessage M {\n  optional string s = 1 [default = `x\ny`, json_name = `j\nk`, (o) = `p\rq`];\n}\n')
  await roundTrip('syntax = "proto2";\nenum E {\n  A = 0 [(o) = `x\ny`];\n}\n')
  // test/spec/nesting.tsv: braces in a backtick string, across lines
  await roundTrip('syntax = "proto2";\noption a = `{{{\n}`;\n')
  // one with no line break is written quoted, which reads back the same
  await roundTrip('syntax = "proto2";\noption a = `x`;\n', 'syntax = "proto2";\noption a = "`x`";\n')
  // a backtick escaped inside one, and a backslash escaped before its end
  await roundTrip('syntax = "proto2";\noption a = `a\\`b\nc`;\n', 'syntax = "proto2";\noption a = `a\\`b\nc`;\n')
  await roundTrip('syntax = "proto2";\noption a = `x\\\\\ny\\\\`;\n', 'syntax = "proto2";\noption a = `x\\\\\ny\\\\`;\n')
})

// An option's value holding both quotes, from a literal, escapes the
// quotes it is between, so it is written between them; as an aggregate it
// had read back as an error. An escaped backslash escapes no quote after
// it, which had been taken for an escaped quote and written between the
// quotes it does not escape. An aggregate's text, its quotes unescaped,
// stays an aggregate.
test('the render writes a text holding both quotes between the quotes it escapes', { skip }, async () => {
  await roundTrip('syntax = "proto2";\noption a = "a\\"b\'c";\n', 'syntax = "proto2";\noption a = "a\\"b\'c";\n')
  await roundTrip('syntax = "proto2";\noption a = \'a"b\\\'c\';\n', 'syntax = "proto2";\noption a = \'a"b\\\'c\';\n')
  await roundTrip('syntax = "proto2";\noption a = \'p\\\\"q\\\'r\';\n', 'syntax = "proto2";\noption a = \'p\\\\"q\\\'r\';\n')
  await roundTrip('syntax = "proto2";\noption a = "p\\\\\'q\\"r";\n', 'syntax = "proto2";\noption a = "p\\\\\'q\\"r";\n')
  await roundTrip('syntax = "proto2";\nmessage M {\n  optional string s = 1 [default = \'a"b\\\'c\'];\n}\n')
  await roundTrip('syntax = "proto2";\noption a = { a: "x"\n c: \'y\' };\n')
})

// The reader takes a reserved name from a quoted literal or an
// identifier, and reads the words of a backtick string as names of their
// own, so a reserved name is written between quotes, those it escapes
// where it holds both, and one that no quotes hold is refused.
test('the render writes a reserved name between quotes that hold it', { skip }, async () => {
  await roundTrip('syntax = "proto2";\nmessage M {\n  reserved \'a"b\\\'c\', "d\\"e";\n}\n',
    'syntax = "proto2";\nmessage M {\n  reserved \'a"b\\\'c\', \'d\\"e\';\n}\n')
  const tree = parse('syntax = "proto2";\nmessage M {\n  reserved "a";\n}\n')
  for (const name of ['`x\ny`', 'x"y\'z', 'x\\']) {
    const copy = JSON.parse(JSON.stringify(tree))
    copy.messageType[0].reservedName = [name]
    await refuses(`the reserved name ${brief(JSON.stringify(name))}`, copy, /a reserved name holds what no quotes hold/)
  }
})

// No literal holds a control character, a backtick string only a line
// break, and an aggregate only the tab and the line breaks between its
// tokens; no quotes hold a text with both quotes unescaped, or one ending
// in a backslash that would escape the closing quote. A tree a program
// built with such a string is refused, where it had been written as a
// file the reader refuses or reads as another.
test('the render refuses a string that nothing holds where it goes', { skip }, async () => {
  const tree = parse('syntax = "proto2";\noption a = "x";\nmessage M {\n  optional string s = 1 [default = "y"];\n}\n')
  await refuses('an option holding U+0001', { ...tree, options: { a: 'x\u0001y' } },
    /an option's value holds a control character that no literal, backtick string or aggregate holds/)
  for (const v of ['x\ty', 'x\ny', 'x"y\'z', 'x\\', '`x\ty`', '`x\ny\\`']) {
    const copy = JSON.parse(JSON.stringify(tree))
    copy.messageType[0].field[0].defaultValue = v
    await refuses(`the default ${brief(JSON.stringify(v))}`, copy, /a string holds what no literal or backtick string holds/)
  }
  const named = JSON.parse(JSON.stringify(tree))
  named.messageType[0].field[0].jsonName = 'x"y\'z'
  await refuses('a json_name holding both quotes unescaped', named, /a string holds what no literal or backtick string holds/)
})
