#!/usr/bin/env node
// Regenerate test/spec/protobuf-suite.tsv from test/protobuf-suite/valid.json.
//
// One row per in-scope `valid` case, headed by a `# <case name>` line. The
// `expected` column is THIS parser's output, from the canonical TypeScript
// build (ts/dist), so the fixture holds Go and Rust to TypeScript; the
// conformance runners hold all three to protoc's goldens. Run it only when
// those runners are green. The header, every line above the first case,
// is kept from the file being replaced.
//
// usage (from the repository root, after `npm run build` in ts/):
//   node test/protobuf-suite/tools/suite-tsv.js
//
// Each case builds a fresh engine, so a progress line goes to stderr every
// ten cases.

'use strict'

const Fs = require('node:fs')
const Path = require('node:path')

const REPO = Path.join(__dirname, '..', '..', '..')
const VALID = Path.join(REPO, 'test', 'protobuf-suite', 'valid.json')
const TSV = Path.join(REPO, 'test', 'spec', 'protobuf-suite.tsv')
const { parse } = require(Path.join(REPO, 'ts', 'dist', 'proto.js'))

// The editions the package does not claim, as the conformance runners
// exclude them: protoc-internal UNSTABLE and NNNNN_TEST_ONLY.
const OUT_OF_SCOPE = /edition\s*=\s*["'](?!2023|2024)/

// The input column's escapes, as the fixture loader decodes them.
const esc = (s) =>
  s.replace(/\\/g, '\\\\').replace(/\n/g, '\\n').replace(/\r/g, '\\r').replace(/\t/g, '\\t')

const header = []
for (const line of Fs.readFileSync(TSV, 'utf8').split('\n')) {
  if (/^# [A-Za-z]+Test\./.test(line)) break
  header.push(line)
}

const cases = JSON.parse(Fs.readFileSync(VALID, 'utf8')).filter(
  (c) => !OUT_OF_SCOPE.test(c.input))
const out = [...header]
cases.forEach((c, i) => {
  out.push('# ' + c.name)
  out.push(esc(c.input) + '\t' + JSON.stringify(parse(c.input)))
  if (0 === (i + 1) % 10 || i + 1 === cases.length) {
    const pct = Math.floor((100 * (i + 1)) / cases.length)
    process.stderr.write(`protobuf-suite.tsv: ${i + 1}/${cases.length} (${pct}%)\n`)
  }
})
Fs.writeFileSync(TSV, out.join('\n') + '\n')
console.log(`wrote ${cases.length} rows to ${Path.relative(REPO, TSV)}`)
