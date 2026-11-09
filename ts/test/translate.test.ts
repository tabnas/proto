/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

import * as assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import * as path from 'node:path'
import { test } from 'node:test'

const { translate } = require('..')
const root = path.resolve(__dirname, '..', '..')

// The parts a host sees are the package's own copies, written by
// `npm run embed`; they must be the repository's files.
test('translation parts expose the manifest, source and explicit entry', () => {
  const parts = translate()
  assert.ok(parts)
  assert.equal(parts.manifest, readFileSync(path.join(root, 'tabnas.plugin.json'), 'utf8'))
  assert.equal(parts.lift, undefined)
  assert.equal(parts.render?.entry, 'proto-render')
  assert.equal(parts.render?.source, readFileSync(path.join(root, 'alchemy', 'render.alc'), 'utf8'))
})

// An embed takes a plain tree into a format's own schema. proto's render
// writes the descriptor the reader builds and nothing else, so its
// manifest names no embed and the package carries none; a manifest that
// named one would be held to its file here, as the render is above.
test('translation parts carry the embed the manifest names, and none where it names none', () => {
  const parts = translate()
  const spec = JSON.parse(readFileSync(path.join(root, 'tabnas.plugin.json'), 'utf8')).translate
  if (null == spec.embed) {
    assert.equal(parts.embed, undefined)
  } else {
    assert.equal(parts.embed?.entry, 'proto-embed')
    assert.equal(parts.embed?.source, readFileSync(path.join(root, spec.embed), 'utf8'))
  }
})

// The transducer part the manifest names is the one the package carries,
// under the entry a host calls.
test('translation parts carry the transducer the manifest names', () => {
  const parts = translate()
  const spec = JSON.parse(readFileSync(path.join(root, 'tabnas.plugin.json'), 'utf8')).translate
  assert.equal(spec.transduce, 'alchemy/transduce.alc')
  assert.equal(parts.transduce?.entry, 'proto-transduce')
  assert.equal(parts.transduce?.source, readFileSync(path.join(root, spec.transduce), 'utf8'))
})

// The tree is the file's declarations in source order, the format's own
// shape, so the manifest names it; a document streams through the part,
// so the manifest says nothing of reading it whole.
test('the manifest names the source-ordered schema and an object root', () => {
  const spec = JSON.parse(translate().manifest).translate
  assert.equal(spec.reads, 'tree')
  assert.equal(spec.writes, 'tree')
  assert.equal(spec.root, 'object')
  assert.equal(spec.schema, 'proto-ast')
  assert.equal(spec.whole, undefined)
  for (const line of spec.loss) {
    assert.match(line, /^[A-Z].*\.$/s, `${JSON.stringify(line)} is not a sentence`)
  }
})

// A host links the parts with its own program and other formats' parts,
// so every definition is named for proto and neither file defines an
// export of its own.
test('the render and the transducer are libraries named for proto', () => {
  const parts = translate()
  for (const [entry, source] of [['proto-render', parts.render.source], ['proto-transduce', parts.transduce.source]]) {
    const names = [...source.matchAll(/^def ([^\s\[]+)/gm)].map((m) => m[1])
    assert.ok(names.includes(entry), entry)
    for (const name of names) assert.ok(name.startsWith('proto-'), `${name} is not named for proto`)
  }
})
