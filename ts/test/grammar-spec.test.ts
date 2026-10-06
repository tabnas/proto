/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The compiled grammar. proto-grammar/*.abnf is compiled once, at build
// time, by `npm run gen-grammar` (ts/gen-grammar.js), into
// src/proto-grammar.json, which the plugin installs. These tests hold
// that arrangement:
//
// - the committed file is what @tabnas/abnf compiles today, so a grammar
//   edit or a compiler release that changes the output fails here until
//   the file is regenerated, rather than shipping a stale grammar;
// - the file is pure data: every function it names is an engine builtin;
// - installing the plugin loads neither @tabnas/abnf nor @tabnas/bnf.
//   Both are devDependencies now, needed only to generate the file.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { execFileSync } from 'node:child_process'
import Fs from 'node:fs'
import Path from 'node:path'

const { BUILTIN_REFS } = require('@tabnas/parser')

const TS = Path.join(__dirname, '..')

// The generator, a plain script beside package.json. Required for its
// compile function: requiring it writes nothing.
const gen = require(Path.join(TS, 'gen-grammar.js')) as {
  compileGrammar: () => string
  SPEC_FILE: string
}

const committed = Fs.readFileSync(gen.SPEC_FILE, 'utf8')

describe('compiled grammar', () => {
  test('src/proto-grammar.json is what @tabnas/abnf compiles today', () => {
    assert.ok(
      committed === gen.compileGrammar(),
      'ts/src/proto-grammar.json is stale: run `npm run gen-grammar` (from ' +
        'ts/) and commit the result',
    )
  })

  test('is pure data: every function it names is an engine builtin', () => {
    const spec = JSON.parse(committed)
    assert.deepStrictEqual(Object.keys(spec).sort(), ['meta', 'options', 'rule', 'v'])
    assert.equal(spec.options.rule.start, '__start__')
    // Every `@` string is a serialized regular expression or a `$`
    // builtin the engine provides; a closure would have to arrive by
    // name from a ref map, and there is none.
    const named = new Set<string>()
    JSON.stringify(spec.rule, (_key, value) => {
      if ('string' === typeof value && value.startsWith('@')) named.add(value)
      return value
    })
    assert.ok(0 < named.size)
    for (const name of named) {
      assert.ok(name in BUILTIN_REFS, `${name} is not an engine builtin`)
    }
    // The CST builders, which the plugin used to get as closures.
    for (const builder of ['@node$', '@capture$', '@bubble$']) {
      assert.ok(named.has(builder), builder)
    }
  })

  test('installing the plugin loads neither @tabnas/abnf nor @tabnas/bnf', () => {
    // A fresh process, so that nothing this suite loaded (the compiler,
    // above) can hide in the module cache. It records every request for
    // either package, then installs the plugin, parses, and reports which
    // loaded files belong to either package's directory.
    const script = `
      const Module = require('node:module')
      const Fs = require('node:fs')
      const Path = require('node:path')
      const asked = []
      const load = Module._load
      Module._load = function (request, ...rest) {
        if (/^@tabnas\\/(abnf|bnf)(\\/|$)/.test(request)) asked.push(request)
        return load.call(this, request, ...rest)
      }
      const { Tabnas } = require('@tabnas/parser')
      const { Proto, parse } = require(${JSON.stringify(Path.join(TS, 'dist', 'proto.js'))})
      const tn = new Tabnas({ rewind: { history: 8192 } }).use(Proto)
      const cst = tn.parse('syntax = "proto3"; message M { int32 a = 1; }')
      const file = parse('syntax = "proto3"; message M { int32 a = 1; }')
      Module._load = load
      // Each package's own directory, wherever npm or a sibling link put it.
      function home(name, from) {
        try {
          const main = Fs.realpathSync(require.resolve(name, { paths: [from] }))
          let dir = Path.dirname(main)
          while (!Fs.existsSync(Path.join(dir, 'package.json'))) dir = Path.dirname(dir)
          return dir + Path.sep
        } catch (e) { return null }
      }
      const abnf = home('@tabnas/abnf', process.cwd())
      const bnf = home('@tabnas/bnf', abnf || process.cwd())
      const loaded = Object.keys(require.cache).filter((f) =>
        [abnf, bnf].some((dir) => null != dir && f.startsWith(dir)))
      process.stdout.write(JSON.stringify({
        root: cst.rule, message: file.messageType[0].name,
        decorated: 'function' === typeof tn.abnf,
        asked, loaded, found: [abnf, bnf],
      }))
    `
    const out = JSON.parse(
      execFileSync(process.execPath, ['-e', script], { cwd: TS, encoding: 'utf8' }),
    )
    assert.equal(out.root, 'proto')
    assert.equal(out.message, 'M')
    assert.equal(out.decorated, false)
    assert.deepStrictEqual(out.asked, [])
    assert.deepStrictEqual(out.loaded, [])
    // The check is only worth something if it could have failed: the
    // compiler is installed here (a devDependency), so a stray require
    // would have found it.
    assert.ok(out.found[0], '@tabnas/abnf should be resolvable from ts/')
  })
})
