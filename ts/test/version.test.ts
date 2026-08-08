/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The exported VERSION must equal package.json "version".
//
// This is the CI check for version drift. It exists because the constant HAS
// drifted: @tabnas/json exported Version = '1.0.0' for several releases while
// the package shipped 0.4.x, because nothing rewrote it and AGENTS.md wrongly
// claimed `make publish-go` kept it in sync. A release that bumps
// package.json and forgets the constant now fails here.

import { describe, it } from 'node:test'
import assert from 'node:assert'

// Read as a file, not `require`d: the check must FAIL, not silently pass,
// if package.json is missing or unparseable.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const api = require('..')

function loadPkg(): { name: string; version: string } {
  const path = join(__dirname, '..', 'package.json')
  let raw: string
  try {
    raw = readFileSync(path, 'utf8')
  } catch (e: any) {
    assert.fail(`cannot read ${path}, so VERSION cannot be checked: ${e.message}`)
  }
  try {
    return JSON.parse(raw)
  } catch (e: any) {
    assert.fail(`${path} is not readable JSON: ${e.message}`)
  }
}

describe('version', () => {
  it('VERSION matches package.json', () => {
    const pkg = loadPkg()
    assert.ok(pkg.version, 'package.json has no version field')
    assert.equal(
      api.VERSION,
      pkg.version,
      `VERSION drift: ${pkg.name} exports ${api.VERSION} but package.json is ` +
        `${pkg.version}. Both are rewritten by admin/publish.sh at release; ` +
        `if you bumped one by hand, bump the other.`,
    )
  })

  it('VERSION is exported and looks like a semver', () => {
    assert.equal(typeof api.VERSION, 'string', 'VERSION must be exported as a string')
    assert.match(api.VERSION, /^\d+\.\d+\.\d+/, 'VERSION must be a semver')
  })
})
