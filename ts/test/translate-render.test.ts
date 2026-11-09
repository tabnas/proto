/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The render, run: a source-ordered tree (`parseTree`'s, the schema
// `proto-ast`) written as a .proto file by the render the package carries
// (`translate().render`), through a command that runs alchemy programs
// (`dmog run`, tabnas/dmog), which this package does not depend on, then
// read back with this package's reader. Set TABNAS_ALCHEMY to the
// command's path to run these tests; without it they skip.
//
// The contract is the round trip: the text reads back as the tree it was
// written from. A tree that is not the schema's is refused with
// TARGET_VALUE_UNREPRESENTABLE, naming what was met.

import * as assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { spawn } from 'node:child_process'
import { after, test } from 'node:test'

const { parseTree, translate } = require('..')

const ALCHEMY = process.env.TABNAS_ALCHEMY
const skip = ALCHEMY ? false : 'TABNAS_ALCHEMY does not name the alchemy command'

// Every transient task reports progress at least every 30 seconds
// (AGENTS.md), so the command runs asynchronously, and while a run is out
// a line names it and the time it has taken, every BEAT_MS. A run takes
// under a second; LIMIT_MS only stops a command that has hung.
const BEAT_MS = 10000
const LIMIT_MS = 120000

// The most a run may write to either stream: 1 MiB, the default maxBuffer
// spawnSync had. A command that floods its output fails its case rather
// than filling the runner's heap.
const MAX_OUTPUT = 1024 * 1024

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
  // A run is stopped by killing the command and closing its output, as
  // spawnSync stopped one at its timeout and its maxBuffer: a command that
  // started others would leave them holding the output open.
  const kill = () => {
    run.kill('SIGKILL')
    run.stdout.destroy()
    run.stderr.destroy()
  }
  let flooded = false
  const collect = (stream: typeof run.stdout) => {
    const chunks: Buffer[] = []
    let bytes = 0
    stream.on('data', (d: Buffer) => {
      chunks.push(d)
      bytes += d.length
      if (MAX_OUTPUT < bytes && !flooded) {
        flooded = true
        kill()
      }
    })
    return () => Buffer.concat(chunks).toString('utf8')
  }
  const stdout = collect(run.stdout)
  const stderr = collect(run.stderr)
  let stopped = false
  const stop = setTimeout(() => { stopped = true; kill() }, LIMIT_MS)
  return new Promise((resolve) => {
    const done = (out: Out) => {
      clearTimeout(stop)
      current = null
      resolve(out)
    }
    run.on('error', (e) => done({ code: 'NOT_RUN', message: String(e) }))
    run.on('close', (status, signal) => {
      if (flooded) {
        return done({ code: 'OUTPUT_LIMIT',
          message: `${what}: wrote more than ${MAX_OUTPUT / (1024 * 1024)} MiB` })
      }
      if (0 === status) return done({ text: stdout() })
      if (stopped) {
        return done({ code: 'STOPPED', message: `${what}: stopped after ${LIMIT_MS / 1000}s` })
      }
      try {
        done(JSON.parse(stderr()))
      } catch (e) {
        done({ code: 'NOT_JSON', message: `${stderr()} ${signal ?? ''}` })
      }
    })
  })
}

// A file read, written and read again: the same tree, member for member,
// and where `text` is given, that text.
async function roundTrip(src: string, text?: string) {
  const tree = parseTree(src)
  const out = await render(`the round trip of ${brief(src)}`, tree)
  assert.equal(typeof out.text, 'string', JSON.stringify(out))
  if (undefined !== text) assert.equal(out.text, text)
  assert.equal(JSON.stringify(parseTree(out.text as string)), JSON.stringify(tree))
}

async function refuses(what: string, tree: unknown, message: RegExp) {
  const out = await render(`the refusal of ${what}`, tree)
  assert.equal(out.code, 'TARGET_VALUE_UNREPRESENTABLE', JSON.stringify(out))
  assert.match(out.message as string, message)
}

test('the render writes a file that reads back as its tree', { skip }, async () => {
  await roundTrip('syntax = "proto2";\noption a = "x";\nmessage M {\n  optional string s = 1 [default = "y"];\n}\n',
    'syntax = "proto2";\noption a = "x";\nmessage M {\n  optional string s = 1 [default = "y"];\n}\n')
  await roundTrip('', '')
})

// Every declaration, in its place: the file's in source order, each block
// two spaces deeper, an aggregate on one line.
test('the render writes every declaration where the file wrote it', { skip }, async () => {
  await roundTrip(
    'edition = "2024";\npackage p;\n' +
    'import public "a.proto";\nimport option "b.proto";\n' +
    'option (o) = { a: 1 b: "x" "y", c { d: -inf } };\n' +
    'export message M {\n  optional int32 f = 1 [default = -1, (o).p = { q: true }];\n' +
    '  map<string, .x.Y> m = 2;\n  group G = 3 { required bytes b = 1; }\n' +
    '  oneof u { option (v) = 1; .a.B w = 4; }\n  extensions 100 to 199, 300 [(e) = 1];\n' +
    '  reserved 5, 6 to max;\n  reserved x, y;\n  local enum E { option allow_alias = true; A = 0; B = -0x1 [(z) = "w"]; reserved -3 to -2; }\n' +
    '  extend Ext { repeated string s = 101; }\n}\n' +
    'service S { rpc A (M) returns (M); rpc B (stream M) returns (stream .M) { option deprecated = true; } }\n' +
    'extend google.protobuf.FileOptions { optional int32 f = 50000; }\n',
    'edition = "2024";\npackage p;\nimport public "a.proto";\nimport option "b.proto";\n' +
    'option (o) = { a: 1 b: "x""y" c { d: -inf } };\n' +
    'export message M {\n  optional int32 f = 1 [default = -1, (o).p = { q: true }];\n' +
    '  map<string, .x.Y> m = 2;\n  group G = 3 {\n    required bytes b = 1;\n  }\n' +
    '  oneof u {\n    option (v) = 1;\n    .a.B w = 4;\n  }\n  extensions 100 to 199, 300 [(e) = 1];\n' +
    '  reserved 5, 6 to max;\n  reserved x, y;\n' +
    '  local enum E {\n    option allow_alias = true;\n    A = 0;\n    B = -0x1 [(z) = "w"];\n    reserved -3 to -2;\n  }\n' +
    '  extend Ext {\n    repeated string s = 101;\n  }\n}\n' +
    'service S {\n  rpc A (M) returns (M);\n  rpc B (stream M) returns (stream .M) {\n    option deprecated = true;\n  }\n}\n' +
    'extend google.protobuf.FileOptions {\n  optional int32 f = 50000;\n}\n')
})

// A scalar is the text the source spells, so a backtick string holding a
// line break is written as itself, and both quotes as written.
test('the render writes every scalar as the source spelled it', { skip }, async () => {
  await roundTrip('syntax = "proto2";\noption a = `x\ny`;\n', 'syntax = "proto2";\noption a = `x\ny`;\n')
  await roundTrip('syntax = "proto2";\noption a = \'a"b\\\'c\';\n', 'syntax = "proto2";\noption a = \'a"b\\\'c\';\n')
  await roundTrip('message message { message message = 1; optional option option = 2; }\n')
})

// A tree the reader could not have built is refused, naming what was met.
test('the render refuses a tree that is not the schema\'s', { skip }, async () => {
  const file = (body: unknown[]) => ({ type: 'file', body })
  await refuses('a root that is no file', { type: 'message', name: 'M', body: [] }, /the root's type is not file/)
  await refuses('a field at the top', file([{ type: 'field', fieldType: 'int32', name: 'f', number: '1' }]),
    /a file's block holds no field/)
  await refuses('a member out of its place',
    file([{ type: 'message', name: 'M', body: [{ type: 'field', name: 'f', fieldType: 'int32', number: '1' }] }]),
    /a field's fieldType comes after what it precedes/)
  await refuses('a field with no number',
    file([{ type: 'message', name: 'M', body: [{ type: 'field', fieldType: 'int32', name: 'f' }] }]),
    /a field with no number/)
  await refuses('a name that is no text', file([{ type: 'package', name: 1 }]), /a package's name is text/)
  await refuses('a member no node has', file([{ type: 'package', name: 'p', path: 'x' }]), /a package has no member path/)
})
