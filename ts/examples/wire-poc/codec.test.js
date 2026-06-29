'use strict'

const { describe, it } = require('node:test')
const assert = require('node:assert')

const { encode, decode, fdp } = require('./schema')

describe('wire-poc codec (driven by @tabnas/proto descriptor)', () => {
  it('loaded the schema from @tabnas/proto', () => {
    assert.equal(fdp.syntax, 'proto3')
    assert.deepEqual(fdp.messageType.map((m) => m.name), ['ChatMessage', 'Ack'])
  })

  it('encodes spec-correct protobuf bytes (golden)', () => {
    // Ack{ id: 7, ok: true }: field 1 varint 7 = 08 07; field 2 varint 1 = 10 01.
    assert.equal(encode('Ack', { id: 7, ok: true }).toString('hex'), '08071001')
    // ChatMessage{ id: 1 }: field 1 varint 1 = 08 01.
    assert.equal(encode('ChatMessage', { id: 1 }).toString('hex'), '0801')
    // proto3 omits singular default/zero values -> empty message.
    assert.equal(encode('Ack', { id: 0, ok: false, note: '' }).length, 0)
  })

  it('round-trips a full ChatMessage through real wire bytes', () => {
    const msg = {
      id: 42,
      user: 'ada',
      text: 'hello, protobuf',
      timestamp: 1700000000000,
      tags: ['urgent', 'demo'],
      priority: 'HIGH',
      meta: { client: 'cli/1.0', encrypted: true },
    }
    const bytes = encode('ChatMessage', msg)
    assert.ok(bytes.length > 0)
    const back = decode('ChatMessage', bytes)
    assert.deepEqual(back, msg)
  })

  it('fills proto3 defaults for omitted fields on decode', () => {
    const back = decode('ChatMessage', encode('ChatMessage', { user: 'x' }))
    assert.equal(back.id, 0)
    assert.equal(back.text, '')
    assert.deepEqual(back.tags, [])
    assert.equal(back.priority, 'NORMAL')
    assert.equal(back.meta, null)
  })
})
