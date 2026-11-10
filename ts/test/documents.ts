/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The documents the window, streaming and history tests read: the input of
// every row of the shared fixtures (test/spec, test/passes, test/tree),
// every input of the protoc corpus (test/protobuf-suite) and its leniency
// probes, test/descriptor/descriptor.proto, a long document of every kind
// of token, and long documents that fail. go/window_test.go and
// rs/tests/window_test.rs read the same.

import * as fs from 'node:fs'
import * as path from 'node:path'

import { findSpecDir } from '@tabnas/support'

import { MAX_NESTING_DEPTH } from '../dist/proto'

const ROOT = path.join(__dirname, '..', '..')

// A reader that hands its bytes over at most `size` at a time.
export function pieces(bytes: Uint8Array, size: number) {
  let at = 0
  return {
    read(buf: Uint8Array): number {
      const n = Math.min(size, buf.length, bytes.length - at)
      buf.set(bytes.subarray(at, at + n), 0)
      at += n
      return n
    },
  }
}

// The input of every row of every fixture file of `dir`, its `\n`, `\r`,
// `\t` and `\\` the characters they name.
function rows(dir: string, out: [string, string][]): void {
  for (const file of fs.readdirSync(dir).filter((f) => f.endsWith('.tsv')).sort()) {
    fs.readFileSync(path.join(dir, file), 'utf8').split('\n').forEach((line, i) => {
      if (0 === i || '' === line || (line.startsWith('#') && !line.includes('\t'))) return
      const input = line.split('\t')[0]
        .replace(/\\([nrt\\])/g, (_, c) => ({ n: '\n', r: '\r', t: '\t', '\\': '\\' } as any)[c])
      out.push([file + ':' + (i + 1), input])
    })
  }
}

// The input of every row of the shared fixtures, every input of the protoc
// corpus and its leniency probes, and descriptor.proto.
export function documents(): [string, string][] {
  const out: [string, string][] = []
  const spec = findSpecDir(__dirname)
  rows(spec, out)
  rows(path.join(ROOT, 'test', 'passes'), out)
  rows(path.join(ROOT, 'test', 'tree'), out)
  const suite = path.join(ROOT, 'test', 'protobuf-suite')
  for (const file of ['valid', 'invalid', 'accept-only']) {
    const cases = JSON.parse(fs.readFileSync(path.join(suite, file + '.json'), 'utf8'))
    cases.forEach((c: any, i: number) => out.push([file + ':' + i, c.input]))
  }
  const leniency = JSON.parse(fs.readFileSync(path.join(suite, 'leniency.json'), 'utf8'))
  leniency.probes.forEach((p: any, i: number) => {
    if ('string' === typeof p.input) out.push(['leniency:' + i, p.input])
  })
  const descriptor = path.join(ROOT, 'test', 'descriptor', 'descriptor.proto')
  out.push(['descriptor.proto', fs.readFileSync(descriptor, 'utf8')])
  return out
}

// A `.proto` file of every kind of token, the long ones longer than the
// engine's 64 KiB window, with characters past ASCII in its comments and
// strings and aggregate option values of every shape, so that tokens of
// every kind straddle the window's edges as a reader hands the text over.
// The same document as `long_document` in rs/tests/window_test.rs and
// `longDocument` in go/window_test.go.
export function longDocument(): string {
  const parts: string[] = []
  parts.push('// ' + 'a comment past the window, é \u{1D11E}; '.repeat(2000) + '\r\n')
  parts.push('/* ' + 'a block comment past the window\r\n\t'.repeat(2000) + '*/\r\n')
  parts.push('syntax = "proto2";\r\npackage a.b.c;\r\n')
  parts.push('option long = "' + 'a string past the window ü '.repeat(3000) + '";\r\n')
  parts.push("option tick = '" + 'single quotes past the window '.repeat(3000) + "';\r\n")
  parts.push('option (named) = ' + 'n'.repeat(70000) + ';\r\n')
  parts.push('option (big) = {\r\n' + '  a: 1 /* c\t*/ b { c: "d" } # e\r\n\t'.repeat(2000) + '};\r\n')
  parts.push('message ' + 'N'.repeat(70000) + ' { optional int32 f = 1; }\r\n')
  for (let i = 0; i < 250; i++) {
    const hex = i.toString(16).toUpperCase()
    parts.push(`message M${i} { // régle ${i}\r\n`)
    parts.push(`  optional int32 a${i} = ${i + 1} [default = -0x${hex}, (o) = { x: ${i} y: "z" }];\r\n`)
    parts.push(`  repeated string optionals = ${i + 2};\r\n  required group G${i} = ${i + 3} { optional bool b = 1; }\r\n`)
    parts.push(`  enum E${i} { option allow_alias = true; V${i} = 0; W${i} = 1 [deprecated = true]; }\r\n`)
    parts.push(`  oneof o${i} { int64 one = ${i + 4}; }\r\n  map<string, int32> m${i} = ${i + 5};\r\n`)
    parts.push(`  extensions 100 to max;\r\n  reserved 50 to 60;\r\n  reserved "res${i}";\r\n}\r\n`)
    parts.push(`service S${i} { rpc R${i}(stream M${i}) returns (M${i}) { option (x) = {a: 1}; } }\r\n`)
  }
  parts.push('extend M0 { optional int32 ext = 1000; }\r\n')
  return parts.join('')
}

// Long documents that fail: a token that runs past the engine's window and
// is then refused, or never ends, and nesting past the cap after a long
// run.
export function longErrorDocuments(): string[] {
  const pad = '// ' + 'padding ü '.repeat(9000) + '\r\nsyntax = "proto3";\r\n'
  const cap = MAX_NESTING_DEPTH + 1
  return [
    pad + 'option a = "' + 'unclosed string '.repeat(6000) + '\r\n',
    pad + '/* ' + 'an unclosed comment '.repeat(4000) + '\r\n',
    pad + 'message M { ' + 'int32 a = 1; '.repeat(6000) + '\r\n',
    pad + 'option b = {' + ' c: 1'.repeat(15000) + ' ) };\r\n',
    pad + 'message ' + 'm'.repeat(70000) + ' } \r\n',
    pad + 'option (x) = ' + '{a '.repeat(cap) + '}'.repeat(cap) + ';\r\n',
    pad + 'message M{'.repeat(cap) + '}'.repeat(cap),
    pad + 'message M {'.repeat(100000),
  ]
}
