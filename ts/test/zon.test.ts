/* Copyright (c) 2025 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Zon } from '../dist/zon'

// Jsonic builds maps with Object.create(null); normalise to plain objects so
// assert.deepStrictEqual can compare against JSON-style literals.
function plain(v: any): any {
  if (v === null || typeof v !== 'object') return v
  if (Array.isArray(v)) return v.map(plain)
  const out: Record<string, any> = {}
  for (const k of Object.keys(v)) out[k] = plain((v as any)[k])
  return out
}

function parse(src: string, opts?: any) {
  const j = new Tabnas().use(jsonic).use(Zon, opts || {})
  return plain(j.parse(src))
}

describe('zon', () => {
  test('scalar values', () => {
    assert.strictEqual(parse('42'), 42)
    assert.strictEqual(parse('3.14'), 3.14)
    assert.strictEqual(parse('true'), true)
    assert.strictEqual(parse('false'), false)
    assert.strictEqual(parse('null'), null)
    assert.strictEqual(parse('"hello"'), 'hello')
  })

  test('numeric bases', () => {
    assert.strictEqual(parse('0x2a'), 42)
    assert.strictEqual(parse('0o52'), 42)
    assert.strictEqual(parse('0b101010'), 42)
    assert.strictEqual(parse('1_000_000'), 1000000)
  })

  test('enum literal as bare string', () => {
    assert.strictEqual(parse('.foo'), 'foo')
    assert.strictEqual(parse('.bar_baz'), 'bar_baz')
  })

  test('empty struct', () => {
    assert.deepStrictEqual(parse('.{}'), [])
  })

  test('simple struct', () => {
    assert.deepStrictEqual(parse('.{ .a = 1 }'), { a: 1 })
    assert.deepStrictEqual(parse('.{ .a = 1, .b = 2 }'), { a: 1, b: 2 })
  })

  test('trailing comma in struct', () => {
    assert.deepStrictEqual(parse('.{ .a = 1, }'), { a: 1 })
    assert.deepStrictEqual(parse('.{ .a = 1, .b = 2, }'), { a: 1, b: 2 })
  })

  test('tuple literal', () => {
    assert.deepStrictEqual(parse('.{ 1, 2, 3 }'), [1, 2, 3])
    assert.deepStrictEqual(parse('.{ "a", "b" }'), ['a', 'b'])
  })

  test('trailing comma in tuple', () => {
    assert.deepStrictEqual(parse('.{ 1, 2, 3, }'), [1, 2, 3])
  })

  test('nested struct', () => {
    assert.deepStrictEqual(parse('.{ .a = .{ .b = 1 } }'), { a: { b: 1 } })
  })

  test('nested tuple', () => {
    assert.deepStrictEqual(parse('.{ .{ 1, 2 }, .{ 3, 4 } }'), [
      [1, 2],
      [3, 4],
    ])
  })

  test('mixed nesting', () => {
    assert.deepStrictEqual(
      parse('.{ .xs = .{ 1, 2, 3 }, .y = .{ .z = true } }'),
      { xs: [1, 2, 3], y: { z: true } },
    )
  })

  test('string escapes', () => {
    assert.strictEqual(parse('"a\\nb"'), 'a\nb')
    assert.strictEqual(parse('"a\\tb"'), 'a\tb')
    assert.strictEqual(parse('"a\\\\b"'), 'a\\b')
  })

  test('enum literal as value', () => {
    assert.deepStrictEqual(parse('.{ .kind = .red }'), { kind: 'red' })
  })

  test('comments', () => {
    const src = `.{
      // a comment
      .name = "x", // trailing comment
      .version = "1.0", // version
    }`
    assert.deepStrictEqual(parse(src), { name: 'x', version: '1.0' })
  })

  test('realistic build.zig.zon', () => {
    const src = `.{
      .name = "example",
      .version = "0.0.1",
      .minimum_zig_version = "0.14.0",
      .dependencies = .{
        .foo = .{
          .url = "https://example.com/foo.tar.gz",
          .hash = "1220deadbeef",
        },
      },
      .paths = .{
        "build.zig",
        "src",
        "",
      },
    }`
    assert.deepStrictEqual(parse(src), {
      name: 'example',
      version: '0.0.1',
      minimum_zig_version: '0.14.0',
      dependencies: {
        foo: {
          url: 'https://example.com/foo.tar.gz',
          hash: '1220deadbeef',
        },
      },
      paths: ['build.zig', 'src', ''],
    })
  })

  test('char literal as number', () => {
    assert.strictEqual(parse("'A'", { charAsNumber: true }), 65)
    assert.strictEqual(parse("'\\n'", { charAsNumber: true }), 10)
    assert.strictEqual(parse("'\\u{1F600}'", { charAsNumber: true }), 0x1f600)
  })

  test('char literal as string', () => {
    assert.strictEqual(parse("'A'"), 'A')
  })

  test('multi-line string', () => {
    const src = `.{
      .text = \\\\hello
              \\\\world
      ,
    }`
    assert.deepStrictEqual(parse(src), { text: 'hello\nworld' })
  })

  test('enumTag option', () => {
    const opts = { enumTag: '$enum' }
    assert.deepStrictEqual(parse('.{ .kind = .red }', opts), {
      kind: { $enum: 'red' },
    })
  })
})
