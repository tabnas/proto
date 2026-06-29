'use strict'

// A small, self-contained proto3 wire codec driven entirely by the
// FileDescriptorProto that @tabnas/proto produces. It reads field numbers,
// types, labels and nested type descriptors straight from the descriptor —
// no other protobuf library is involved — and emits / parses real protobuf
// binary (https://protobuf.dev/programming-guides/encoding/).
//
// Scope (enough for the demo, not a full runtime): varint scalars
// (int32/int64/uint32/uint64/bool), enums, length-delimited string/bytes,
// nested messages, and repeated *string*. No fixed32/64, sint zigzag,
// packed repeated scalars, maps, groups, or oneof wire rules.

// Wire types.
const VARINT = 0
const LEN = 2

// FileDescriptorProto scalar types carried as varints.
const VARINT_TYPES = new Set([
  'TYPE_INT32', 'TYPE_INT64', 'TYPE_UINT32', 'TYPE_UINT64', 'TYPE_BOOL',
])
const SIGNED_TYPES = new Set(['TYPE_INT32', 'TYPE_INT64'])

// `.chat.ChatMessage.Meta` / `Meta` -> `Meta`
function simpleName(typeName) {
  return String(typeName || '').replace(/^\./, '').split('.').pop()
}

// Flatten every message and enum (including nested ones) into a
// simple-name -> {kind, desc} registry the codec resolves field types
// against. (Simple-name lookup is a deliberate simplification — real
// protobuf scope resolution is a separate pass.)
function makeRegistry(fdp) {
  const reg = Object.create(null)
  const addEnum = (e) => { reg[e.name] = { kind: 'enum', desc: e } }
  const addMsg = (m) => {
    reg[m.name] = { kind: 'message', desc: m }
    for (const n of m.nestedType || []) addMsg(n)
    for (const e of m.enumType || []) addEnum(e)
  }
  for (const m of fdp.messageType || []) addMsg(m)
  for (const e of fdp.enumType || []) addEnum(e)
  return reg
}

// Classify a field for the wire: its wire type and how to (de)serialise it.
// @tabnas/proto emits message AND enum references both as TYPE_MESSAGE +
// typeName (it defers resolution), so we look the name up in the registry
// to tell an enum (varint) from a message (length-delimited).
function classify(reg, field) {
  const t = field.type
  if ('TYPE_MESSAGE' === t || 'TYPE_ENUM' === t) {
    const entry = reg[simpleName(field.typeName)]
    if (entry && 'enum' === entry.kind) return { wire: VARINT, kind: 'enum', enum: entry.desc }
    return { wire: LEN, kind: 'message', msg: entry ? entry.desc : null }
  }
  if (VARINT_TYPES.has(t)) return { wire: VARINT, kind: 'varint', type: t }
  return { wire: LEN, kind: 'bytes', type: t } // string / bytes
}

// ---- varint ---------------------------------------------------------------

function writeVarint(arr, value) {
  let v = BigInt(value)
  if (v < 0n) v += 1n << 64n // negative ints encode as 64-bit two's complement
  do {
    let b = Number(v & 0x7fn)
    v >>= 7n
    if (v > 0n) b |= 0x80
    arr.push(b)
  } while (v > 0n)
}

function readVarint(buf, pos) {
  let result = 0n, shift = 0n
  for (;;) {
    const b = buf[pos.i++]
    result |= BigInt(b & 0x7f) << shift
    if (0 === (b & 0x80)) break
    shift += 7n
  }
  return result
}

function toJs(big, type) {
  if ('TYPE_BOOL' === type) return 0n !== big
  let v = big
  if (SIGNED_TYPES.has(type) && v >= 1n << 63n) v -= 1n << 64n
  return (v <= BigInt(Number.MAX_SAFE_INTEGER) && v >= BigInt(Number.MIN_SAFE_INTEGER))
    ? Number(v) : v
}

// ---- enums ----------------------------------------------------------------

function enumNumber(enumDesc, value) {
  if ('number' === typeof value) return value
  const v = (enumDesc.value || []).find((x) => x.name === value)
  return v ? v.number : 0
}

function enumName(enumDesc, number) {
  const v = (enumDesc.value || []).find((x) => x.number === number)
  return v ? v.name : number
}

// ---- skip unknown fields --------------------------------------------------

function skip(buf, pos, wire) {
  if (VARINT === wire) readVarint(buf, pos)
  else if (LEN === wire) pos.i += Number(readVarint(buf, pos))
  else if (1 === wire) pos.i += 8
  else if (5 === wire) pos.i += 4
}

// ---- encode ---------------------------------------------------------------

function isDefault(value) {
  return null == value || 0 === value || false === value || '' === value
}

function encodeField(reg, arr, field, value) {
  const info = classify(reg, field)
  writeVarint(arr, (field.number << 3) | info.wire)
  if ('enum' === info.kind) {
    writeVarint(arr, enumNumber(info.enum, value))
  } else if ('varint' === info.kind) {
    writeVarint(arr, 'TYPE_BOOL' === info.type ? (value ? 1 : 0) : value)
  } else if ('bytes' === info.kind) {
    const bytes = 'TYPE_BYTES' === info.type
      ? Buffer.from(value) : Buffer.from(String(value), 'utf8')
    writeVarint(arr, bytes.length)
    for (const b of bytes) arr.push(b)
  } else { // message
    const sub = encodeMessage(reg, info.msg, value || {})
    writeVarint(arr, sub.length)
    for (const b of sub) arr.push(b)
  }
}

function encodeMessage(reg, msgDesc, obj) {
  const arr = []
  for (const field of msgDesc.field || []) {
    const value = obj[field.name]
    if ('LABEL_REPEATED' === field.label) {
      for (const v of value || []) encodeField(reg, arr, field, v)
    } else if (!isDefault(value)) {
      encodeField(reg, arr, field, value)
    }
  }
  return Buffer.from(arr)
}

// ---- decode ---------------------------------------------------------------

function defaults(reg, msgDesc) {
  const out = {}
  for (const field of msgDesc.field || []) {
    if ('LABEL_REPEATED' === field.label) { out[field.name] = []; continue }
    const info = classify(reg, field)
    if ('varint' === info.kind) out[field.name] = 'TYPE_BOOL' === info.type ? false : 0
    else if ('bytes' === info.kind) out[field.name] = 'TYPE_BYTES' === info.type ? Buffer.alloc(0) : ''
    else if ('enum' === info.kind) out[field.name] = enumName(info.enum, 0)
    else out[field.name] = null
  }
  return out
}

function decodeMessage(reg, msgDesc, buf) {
  const byNum = Object.create(null)
  for (const f of msgDesc.field || []) byNum[f.number] = f
  const out = defaults(reg, msgDesc)
  const pos = { i: 0 }
  while (pos.i < buf.length) {
    const tag = Number(readVarint(buf, pos))
    const number = tag >>> 3, wire = tag & 7
    const field = byNum[number]
    if (!field) { skip(buf, pos, wire); continue }
    const info = classify(reg, field)
    let value
    if (VARINT === info.wire) {
      const big = readVarint(buf, pos)
      value = 'enum' === info.kind ? enumName(info.enum, Number(big)) : toJs(big, info.type)
    } else {
      const len = Number(readVarint(buf, pos))
      const slice = buf.subarray(pos.i, pos.i + len)
      pos.i += len
      if ('message' === info.kind) value = info.msg ? decodeMessage(reg, info.msg, slice) : Buffer.from(slice)
      else if ('TYPE_BYTES' === info.type) value = Buffer.from(slice)
      else value = slice.toString('utf8')
    }
    if ('LABEL_REPEATED' === field.label) out[field.name].push(value)
    else out[field.name] = value
  }
  return out
}

// ---- public API -----------------------------------------------------------

function lookup(reg, typeName) {
  const entry = reg[simpleName(typeName)]
  if (!entry || 'message' !== entry.kind) {
    throw new Error('wire-poc: no message type named "' + typeName + '"')
  }
  return entry.desc
}

function encode(reg, typeName, obj) {
  return encodeMessage(reg, lookup(reg, typeName), obj)
}

function decode(reg, typeName, buf) {
  return decodeMessage(reg, lookup(reg, typeName), buf)
}

module.exports = { makeRegistry, encode, decode, simpleName }
