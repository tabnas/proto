/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Walk the `{rule, src, kids}` CST produced by the proto ABNF grammar and
// assemble a FileDescriptorProto-shaped object. Lexical atoms arrive as
// whole-word tokens, and abnf's leading-ref inlining means the specific
// statement rule (message / field / enum …) is folded into the enclosing
// `topLevelDef` / `messageElement` dispatch node; we recover the statement
// kind from the keyword that precedes the node's first child (`kw`).

import {
  FileDescriptorProto, DescriptorProto, FieldDescriptorProto,
  EnumDescriptorProto, ServiceDescriptorProto, MethodDescriptorProto,
  OneofDescriptorProto, OptionValue, FieldLabel, SCALAR_TYPES,
} from './descriptor'
import { ProtoVersion, isEdition } from './detect-version'

type Node = { rule?: string; src: string; kids?: Node[] }

// Children that are real rule nodes (terminals fold into `src`).
function R(n: Node): Node[] {
  return (n.kids || []).filter((k) => k && k.rule)
}

// The keyword(s) consumed before this node's first child — the part of
// `src` ahead of the first child's `src`. For `message Foo {…}` the first
// child is `Foo`, so `kw` is `message`; for an unlabelled field it is ``.
function kw(n: Node): string {
  const k = R(n)
  if (0 === k.length) return n.src
  const i = n.src.indexOf(k[0].src)
  return i <= 0 ? '' : n.src.slice(0, i)
}

function child(n: Node, rule: string): Node | undefined {
  return R(n).find((k) => k.rule === rule)
}

function unquote(s: string): string {
  const m = s.match(/^["']([\s\S]*)["']$/)
  return m ? m[1] : s
}

// ---- constants / option values -------------------------------------------

function constantValue(n: Node): OptionValue {
  const s = n.src
  if ('true' === s) return true
  if ('false' === s) return false
  if (/^["']/.test(s)) return unquote(s)
  if (/^[-+]?(?:\d|\.\d|0x|0o|0b)/i.test(s)) {
    const num = Number(s.replace(/^\+/, ''))
    if (!Number.isNaN(num)) return num
  }
  return s // identifier (enum value name, inf, nan, …) kept verbatim
}

// The option name in an `optionName "=" constant` statement. Because the
// name's leading `optionNamePart` is inlined and the parts are split across
// the node's `src` and later children, read it as everything before the
// `constant`, with the trailing `=` removed.
function optionNameOf(stmt: Node, valueNode: Node | undefined): string {
  if (!valueNode) return ''
  const i = stmt.src.indexOf(valueNode.src)
  return (i <= 0 ? '' : stmt.src.slice(0, i)).replace(/=$/, '')
}

// fieldOptions = "[" fieldOption *( "," fieldOption ) "]"
// fieldOption  = optionName "=" constant
function readFieldOptions(opts: Node | undefined): Record<string, OptionValue> | undefined {
  if (!opts) return undefined
  const out: Record<string, OptionValue> = {}
  for (const fo of R(opts)) {
    const cst = child(fo, 'constant')
    const name = optionNameOf(fo, cst)
    if (cst && name) out[name] = constantValue(cst)
  }
  return Object.keys(out).length ? out : undefined
}

// ---- fields --------------------------------------------------------------

function fieldLabel(
  labelNode: Node | undefined,
  version: ProtoVersion,
): { label: FieldLabel; proto3Optional?: boolean } {
  const lab = labelNode?.src
  if ('required' === lab) return { label: 'LABEL_REQUIRED' }
  if ('repeated' === lab) return { label: 'LABEL_REPEATED' }
  if ('optional' === lab) {
    return 'proto3' === version
      ? { label: 'LABEL_OPTIONAL', proto3Optional: true }
      : { label: 'LABEL_OPTIONAL' }
  }
  // Implicit label.
  return { label: 'LABEL_OPTIONAL' }
}

function fieldTypeName(typeText: string): Pick<FieldDescriptorProto, 'type' | 'typeName'> {
  const bare = typeText.replace(/^\./, '')
  const scalar = SCALAR_TYPES[bare]
  if (scalar) return { type: scalar }
  // Message or enum reference; resolution deferred, store as written.
  return { type: 'TYPE_MESSAGE', typeName: typeText }
}

// A normal field: [label] fieldType ident "=" fieldNumber [fieldOptions].
// The surrounding dispatch node (`messageElement` / `oneofElement`) has these
// inlined as its children.
// The field type. Normally a `fieldType` child; but when `fieldType` is a
// leading ref (an unlabelled oneof member `string x = 1;`) abnf inlines it,
// so the type surfaces as a bare `fullIdent` / `messageType` child instead.
function typeNodeOf(n: Node): Node | undefined {
  return child(n, 'fieldType') ?? child(n, 'messageType') ?? child(n, 'fullIdent')
}

function buildField(n: Node, version: ProtoVersion): FieldDescriptorProto {
  const label = child(n, 'label')
  const type = typeNodeOf(n)
  const name = child(n, 'ident')
  const number = child(n, 'fieldNumber')
  const opts = child(n, 'fieldOptions')

  const f: FieldDescriptorProto = {
    name: name ? name.src : '',
    number: number ? Number(number.src) : 0,
    ...fieldLabel(label, version),
    ...fieldTypeName(type ? type.src : ''),
  }
  const fo = readFieldOptions(opts)
  if (fo) f.options = fo
  return f
}

// map<K,V> name = N;  ->  a repeated message field whose type is a
// synthesised nested `<Name>Entry` message with mapEntry=true.
function buildMapField(
  n: Node, version: ProtoVersion, into: DescriptorProto,
): FieldDescriptorProto {
  const types = R(n).filter((k) => k.rule === 'fieldType')
  const name = child(n, 'ident')
  const number = child(n, 'fieldNumber')
  const keyText = types[0] ? types[0].src : ''
  const valText = types[1] ? types[1].src : ''
  const fname = name ? name.src : ''
  const entryName = fname.replace(/(^|_)([a-z])/g, (_, a, b) => a + b.toUpperCase())
    + 'Entry'

  const entry: DescriptorProto = {
    name: entryName,
    field: [
      { name: 'key', number: 1, label: 'LABEL_OPTIONAL', ...fieldTypeName(keyText) },
      { name: 'value', number: 2, label: 'LABEL_OPTIONAL', ...fieldTypeName(valText) },
    ],
    nestedType: [], enumType: [], oneofDecl: [], extension: [],
    options: { mapEntry: true },
  }
  into.nestedType.push(entry)

  const f: FieldDescriptorProto = {
    name: fname,
    number: number ? Number(number.src) : 0,
    label: 'LABEL_REPEATED',
    type: 'TYPE_MESSAGE',
    typeName: entryName,
  }
  const fo = readFieldOptions(child(n, 'fieldOptions'))
  if (fo) f.options = fo
  return f
}

// ---- enums ---------------------------------------------------------------

function buildEnum(n: Node): EnumDescriptorProto {
  const e: EnumDescriptorProto = { name: child(n, 'ident')?.src ?? '', value: [] }
  for (const el of R(n).filter((k) => k.rule === 'enumElement')) {
    const k = kw(el)
    if (k.startsWith('reserved')) {
      addReserved(el, e)
      continue
    }
    if (k.startsWith('option')) {
      continue // enum-level option; folded into options below if needed
    }
    // enumField: ident "=" ["-"] fieldNumber  -> name is the kw before "="
    const name = kw(el).replace(/=.*$/, '')
    const num = child(el, 'fieldNumber')
    if (name && num) {
      const neg = /=-/.test(el.src.replace(/\s+/g, ''))
      e.value.push({ name, number: (neg ? -1 : 1) * Number(num.src) })
    }
  }
  return e
}

// ---- reserved / extensions ranges ----------------------------------------

// `ranges = range *( "," range )`. The leading `range` is inlined into
// `ranges.src`, so parse the (whitespace-stripped) text rather than kids.
function ranges(rangesNode: Node | undefined): { start: number; end: number }[] {
  if (!rangesNode) return []
  const out: { start: number; end: number }[] = []
  for (const part of rangesNode.src.replace(/\s+/g, '').split(',')) {
    const m = part.match(/^(-?\d+)(?:to(-?\d+|max))?$/)
    if (!m) continue
    const start = Number(m[1])
    const end = !m[2] ? start : (m[2] === 'max' ? 536870911 : Number(m[2]))
    out.push({ start, end })
  }
  return out
}

function addReserved(n: Node, target: { reservedRange?: any[]; reservedName?: string[] }): void {
  const rn = child(n, 'ranges')
  if (rn) {
    (target.reservedRange = target.reservedRange || []).push(...ranges(rn))
    return
  }
  const names = child(n, 'fieldNames')
  if (names) {
    const list = R(names).filter((k) => k.rule === 'strLit').map((k) => unquote(k.src))
    ;(target.reservedName = target.reservedName || []).push(...list)
  }
}

// ---- messages ------------------------------------------------------------

function buildMessage(n: Node, version: ProtoVersion): DescriptorProto {
  // `n` is a dispatch node whose `message` alt was inlined: kids are
  // [ident, messageBody-children…] or [ident] then messageBody.
  const msg: DescriptorProto = {
    name: child(n, 'ident')?.src ?? '',
    field: [], nestedType: [], enumType: [], oneofDecl: [], extension: [],
  }
  const body = child(n, 'messageBody')
  const elements = body ? R(body).filter((k) => k.rule === 'messageElement') : []
  for (const el of elements) addMessageElement(el, version, msg)
  return msg
}

function addMessageElement(el: Node, version: ProtoVersion, msg: DescriptorProto): void {
  const k = kw(el)
  const first = R(el)[0]
  if (k.startsWith('map<')) { msg.field.push(buildMapField(el, version, msg)); return }
  if (k.startsWith('oneof')) { addOneof(el, version, msg); return }
  if (k.startsWith('export') || k.startsWith('local')) {
    // edition 2024 symbol visibility wraps the message/enum as a child node.
    const m = child(el, 'message'); const e = child(el, 'enumDef')
    if (m) msg.nestedType.push(buildMessage(m, version))
    else if (e) msg.enumType.push(buildEnum(e))
    return
  }
  if (k.startsWith('message')) { msg.nestedType.push(buildMessage(el, version)); return }
  if (k.startsWith('enum')) { msg.enumType.push(buildEnum(el)); return }
  if (k.startsWith('reserved')) { addReserved(el, msg); return }
  if (k.startsWith('extensions')) {
    (msg.extensionRange = msg.extensionRange || []).push(...ranges(child(el, 'ranges')))
    return
  }
  if (k.startsWith('extend')) { addExtend(el, version, msg.extension); return }
  if (k.startsWith('option')) {
    msg.options = { ...(msg.options || {}), ...optionFrom(el) }; return
  }
  if (';' === el.src) return // emptyStmt
  // No keyword and a fieldType/label lead => a field.
  if (first && (first.rule === 'fieldType' || first.rule === 'label')) {
    msg.field.push(buildField(el, version))
  }
}

function addOneof(el: Node, version: ProtoVersion, msg: DescriptorProto): void {
  const name = child(el, 'ident')?.src ?? ''
  const index = msg.oneofDecl.length
  msg.oneofDecl.push({ name })
  for (const of of R(el).filter((k) => k.rule === 'oneofElement')) {
    if (kw(of).startsWith('option')) continue
    if (';' === of.src) continue
    const f = buildField(of, version)
    f.oneofIndex = index
    delete f.proto3Optional // explicit oneof members aren't proto3-optional
    msg.field.push(f)
  }
}

function addExtend(el: Node, version: ProtoVersion, into: FieldDescriptorProto[]): void {
  // extend messageType "{" *field "}" — fields inline as messageElement-like.
  for (const f of R(el).filter((k) => k.rule === 'field' || k.rule === 'messageElement')) {
    into.push(buildField(f, version))
  }
}

// ---- options -------------------------------------------------------------

// optionStmt = "option" optionName "=" constant ";"
function optionFrom(el: Node): Record<string, OptionValue> {
  const cst = child(el, 'constant')
  if (!cst) return {}
  const name = optionNameOf(el, cst).replace(/^option/, '')
  return { [name]: constantValue(cst) }
}

// ---- services ------------------------------------------------------------

function buildService(n: Node): ServiceDescriptorProto {
  const svc: ServiceDescriptorProto = { name: child(n, 'ident')?.src ?? '', method: [] }
  for (const el of R(n).filter((k) => k.rule === 'serviceElement')) {
    if (kw(el).startsWith('rpc')) svc.method.push(buildRpc(el))
  }
  return svc
}

// rpc ident "(" ["stream"] messageType ")" "returns" "(" ["stream"] messageType ")"
function buildRpc(el: Node): MethodDescriptorProto {
  const ids = R(el).filter((k) => k.rule === 'ident')
  const types = R(el).filter((k) => k.rule === 'messageType')
  const flat = el.src.replace(/\s+/g, '')
  const m: MethodDescriptorProto = {
    name: ids[0] ? ids[0].src : '',
    inputType: types[0] ? types[0].src : '',
    outputType: types[1] ? types[1].src : '',
  }
  // Split request vs response on the `returns` keyword so a `(stream …)`
  // is attributed to the right side even when in/out types are identical.
  const ri = flat.indexOf('returns(')
  const request = ri >= 0 ? flat.slice(0, ri) : flat
  const response = ri >= 0 ? flat.slice(ri) : ''
  if (/\(stream/.test(request)) m.clientStreaming = true
  if (/\(stream/.test(response)) m.serverStreaming = true
  return m
}

// ---- file ----------------------------------------------------------------

export function buildFile(proto: Node, version: ProtoVersion): FileDescriptorProto {
  const file: FileDescriptorProto = {
    dependency: [], publicDependency: [], weakDependency: [],
    messageType: [], enumType: [], service: [], extension: [],
  }
  if (isEdition(version)) file.edition = 'EDITION_' + version
  else file.syntax = version

  for (const def of R(proto).filter((k) => k.rule === 'topLevelDef')) {
    const k = kw(def)
    if (k.startsWith('package')) {
      file.package = child(def, 'fullIdent')?.src
    } else if (k.startsWith('import')) {
      const s = child(def, 'strLit')
      if (s) {
        const idx = file.dependency.length
        file.dependency.push(unquote(s.src))
        if (k.includes('public')) file.publicDependency.push(idx)
        if (k.includes('weak')) file.weakDependency.push(idx)
      }
    } else if (k.startsWith('option')) {
      file.options = { ...(file.options || {}), ...optionFrom(def) }
    } else if (k.startsWith('export') || k.startsWith('local')) {
      const m = child(def, 'message'); const e = child(def, 'enumDef')
      if (m) file.messageType.push(buildMessage(m, version))
      else if (e) file.enumType.push(buildEnum(e))
    } else if (k.startsWith('message')) {
      file.messageType.push(buildMessage(def, version))
    } else if (k.startsWith('enum')) {
      file.enumType.push(buildEnum(def))
    } else if (k.startsWith('service')) {
      file.service.push(buildService(def))
    } else if (k.startsWith('extend')) {
      addExtend(def, version, file.extension)
    }
  }
  return file
}
