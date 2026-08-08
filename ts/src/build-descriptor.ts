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
  DescriptorRange, SymbolVisibility, OptionValue, FieldLabel, SCALAR_TYPES,
  MAX_FIELD_NUMBER_END, MAX_MESSAGE_SET_END, MAX_ENUM_NUMBER,
} from './descriptor'
import { ProtoVersion, isEdition } from './detect-version'

type Node = { rule?: string; src: string; kids?: Node[] }

// Children that are real rule nodes (terminals fold into `src`). An empty
// `.proto` is a legal (if useless) file and parses to no node at all, so
// tolerate a missing node here rather than crash at the root.
function R(n: Node | undefined | null): Node[] {
  return ((n && n.kids) || []).filter((k) => k && k.rule)
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
  return s // identifier (enum value name, inf, -nan, …) kept verbatim
}

// The option name in an `optionName "=" constant` statement.
//
// `optionStmt` keeps `optionName` as a child node, so use it when present.
// Inside `fieldOption` abnf inlines the leading `optionNamePart`, so there
// the name has to be read out of `src` — as everything BEFORE the trailing
// `"=" constant`. Anchoring at the end matters: `option (file_opt1) = 1;`
// would otherwise find the value `1` inside the name `file_opt1`.
function optionNameOf(stmt: Node, valueNode: Node | undefined): string {
  const named = child(stmt, 'optionName')
  if (named) return named.src
  if (!valueNode) return ''
  const s = stmt.src.replace(/;$/, '')
  const tail = '=' + valueNode.src
  if (s.endsWith(tail)) return s.slice(0, s.length - tail.length)
  const i = s.indexOf(valueNode.src)
  return i <= 0 ? '' : s.slice(0, i).replace(/=$/, '')
}

// The `json_name` and `default` field options are pseudo-options: protoc
// lifts them out of the option set into FieldDescriptorProto fields.
type PseudoOptions = {
  options?: Record<string, OptionValue>
  jsonName?: string
  defaultValue?: string
}

// fieldOptions = "[" fieldOption *( "," fieldOption ) "]"
// fieldOption  = optionName "=" constant
function readFieldOptions(opts: Node | undefined): PseudoOptions {
  const out: PseudoOptions = {}
  if (!opts) return out
  const map: Record<string, OptionValue> = {}
  for (const fo of R(opts)) {
    const cst = child(fo, 'constant')
    const name = optionNameOf(fo, cst)
    if (!cst || !name) continue
    if ('json_name' === name) { out.jsonName = String(unquote(cst.src)); continue }
    if ('default' === name) { out.defaultValue = unquote(cst.src); continue }
    map[name] = constantValue(cst)
  }
  if (Object.keys(map).length) out.options = map
  return out
}

// The plain (non-pseudo) option map, for the places that cannot carry
// `json_name` / `default` — extension ranges, enum values.
function plainOptions(opts: Node | undefined): Record<string, OptionValue> | undefined {
  return readFieldOptions(opts).options
}

// optionStmt = "option" optionName "=" constant ";"
function optionFrom(el: Node): Record<string, OptionValue> {
  const cst = child(el, 'constant')
  if (!cst) return {}
  const name = optionNameOf(el, cst)
  return name ? { [name]: constantValue(cst) } : {}
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
  // A named reference: could be a message OR an enum, and telling them
  // apart needs symbol resolution this parser deliberately does not do.
  // protoc leaves `type` unset here too, and only fills it in once the
  // name resolves — so record `typeName` as written and nothing else.
  return { typeName: typeText }
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

function applyFieldOptions(f: FieldDescriptorProto, opts: Node | undefined): void {
  const po = readFieldOptions(opts)
  if (undefined !== po.jsonName) f.jsonName = po.jsonName
  if (undefined !== po.defaultValue) f.defaultValue = po.defaultValue
  if (po.options) f.options = po.options
}

function buildField(n: Node, version: ProtoVersion): FieldDescriptorProto {
  const label = child(n, 'label')
  const type = typeNodeOf(n)
  const name = child(n, 'ident')
  const number = child(n, 'fieldNumber')

  const f: FieldDescriptorProto = {
    name: name ? name.src : '',
    number: number ? Number(number.src) : 0,
    ...fieldLabel(label, version),
    ...fieldTypeName(type ? type.src : ''),
  }
  applyFieldOptions(f, child(n, 'fieldOptions'))
  return f
}

// A group is a field plus an implicit nested message. protoc lowercases the
// field name and keeps the declared name for the message and for `typeName`.
//   `optional group TestGroup = 1 { … }`
//     -> field { name: 'testgroup', type: TYPE_GROUP, typeName: 'TestGroup' }
//     -> nestedType { name: 'TestGroup', … }
function buildGroup(
  n: Node, version: ProtoVersion, into: DescriptorProto,
): FieldDescriptorProto {
  const groupName = child(n, 'ident')?.src ?? ''
  const number = child(n, 'fieldNumber')
  into.nestedType.push(messageFromBody(groupName, child(n, 'messageBody'), version))

  const f: FieldDescriptorProto = {
    name: groupName.toLowerCase(),
    number: number ? Number(number.src) : 0,
    ...fieldLabel(child(n, 'label'), version),
    type: 'TYPE_GROUP',
    typeName: groupName,
  }
  applyFieldOptions(f, child(n, 'fieldOptions'))
  return f
}

// A group is the one construct with BOTH a field number and a message body;
// `message` has a body but no number, `map`/`field` a number but no body.
function isGroup(n: Node): boolean {
  return !!child(n, 'fieldNumber') && !!child(n, 'messageBody')
}

// The subset of an option map whose names are rooted at `features`.
function features(
  opts: Record<string, OptionValue> | undefined,
): Record<string, OptionValue> | undefined {
  if (!opts) return undefined
  const out: Record<string, OptionValue> = {}
  for (const [k, v] of Object.entries(opts)) {
    if ('features' === k || k.startsWith('features.')) out[k] = v
  }
  return Object.keys(out).length ? out : undefined
}

// protoc's map-entry name: strip `_`, upper-case the letter that follows
// (and the first letter), then append `Entry`. `map_field` -> `MapFieldEntry`.
function mapEntryName(fieldName: string): string {
  let out = ''
  let capNext = true
  for (const ch of fieldName) {
    if ('_' === ch) { capNext = true; continue }
    out += capNext && 'a' <= ch && ch <= 'z' ? ch.toUpperCase() : ch
    capNext = false
  }
  return out + 'Entry'
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
  const entryName = mapEntryName(fname)

  const key: FieldDescriptorProto =
    { name: 'key', number: 1, label: 'LABEL_OPTIONAL', ...fieldTypeName(keyText) }
  const value: FieldDescriptorProto =
    { name: 'value', number: 2, label: 'LABEL_OPTIONAL', ...fieldTypeName(valText) }
  const entry: DescriptorProto = {
    name: entryName,
    field: [key, value],
    nestedType: [], enumType: [], oneofDecl: [], extension: [],
    options: { mapEntry: true },
  }
  into.nestedType.push(entry)

  const f: FieldDescriptorProto = {
    name: fname,
    number: number ? Number(number.src) : 0,
    label: 'LABEL_REPEATED',
    typeName: entryName,
  }
  applyFieldOptions(f, child(n, 'fieldOptions'))

  // `features` on a map field govern the synthesised entry's key and value
  // fields too, so protoc copies them down. Nothing else is propagated.
  const feat = features(f.options)
  if (feat) { key.options = { ...feat }; value.options = { ...feat } }
  return f
}

// ---- enums ---------------------------------------------------------------

function buildEnum(n: Node): EnumDescriptorProto {
  const e: EnumDescriptorProto = { name: child(n, 'ident')?.src ?? '', value: [] }
  for (const el of R(n).filter((k) => k.rule === 'enumElement')) {
    const k = kw(el)
    if (k.startsWith('reserved')) {
      // Enum reserved ranges are INCLUSIVE and span the whole int32 space.
      addReserved(el, e, { exclusive: false, max: MAX_ENUM_NUMBER })
      continue
    }
    if (k.startsWith('option')) {
      e.options = { ...(e.options || {}), ...optionFrom(el) }
      continue
    }
    // enumField: ident "=" ["-"] fieldNumber  -> name is the kw before "="
    const name = k.replace(/=.*$/, '')
    const num = child(el, 'fieldNumber')
    if (name && num) {
      const neg = /=-/.test(el.src.replace(/\s+/g, ''))
      const v = { name, number: (neg ? -1 : 1) * Number(num.src) }
      const vo = plainOptions(child(el, 'fieldOptions'))
      e.value.push(vo ? { ...v, options: vo } : v)
    }
  }
  return e
}

// ---- reserved / extensions ranges ----------------------------------------

type RangeOpts = { exclusive: boolean; max: number }

// `ranges = range *( "," range )`. The leading `range` is inlined into
// `ranges.src`, so parse the (whitespace-stripped) text rather than kids.
function ranges(rangesNode: Node | undefined, ro: RangeOpts): DescriptorRange[] {
  if (!rangesNode) return []
  const out: DescriptorRange[] = []
  for (const part of rangesNode.src.replace(/\s+/g, '').split(',')) {
    const m = part.match(/^(-?\d+)(?:to(-?\d+|max))?$/)
    if (!m) continue
    const start = Number(m[1])
    let end: number
    if (!m[2]) end = ro.exclusive ? start + 1 : start
    else if ('max' === m[2]) end = ro.max
    else end = Number(m[2]) + (ro.exclusive ? 1 : 0)
    out.push({ start, end })
  }
  return out
}

// The reserved-name list. Both the leading `strLit`/`ident` and (for a
// single-item list) the whole list can be inlined into `src`, so read the
// names out of the statement text; whole-word tokens make that unambiguous.
function reservedNames(n: Node): string[] {
  const body = n.src.replace(/^reserved/, '').replace(/;$/, '')
  const re = /"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)'|([A-Za-z_][A-Za-z0-9_]*)/g
  const out: string[] = []
  let m: RegExpExecArray | null
  while (null !== (m = re.exec(body))) out.push(m[1] ?? m[2] ?? m[3])
  return out
}

function addReserved(
  n: Node,
  target: { reservedRange?: DescriptorRange[]; reservedName?: string[] },
  ro: RangeOpts,
): void {
  const rn = child(n, 'ranges')
  if (rn) {
    (target.reservedRange = target.reservedRange || []).push(...ranges(rn, ro))
    return
  }
  const names = reservedNames(n)
  if (names.length) {
    (target.reservedName = target.reservedName || []).push(...names)
  }
}

// ---- messages ------------------------------------------------------------

function buildMessage(n: Node, version: ProtoVersion): DescriptorProto {
  // `n` is a dispatch node whose `message` alt was inlined: kids are
  // [ident, messageBody-children…] or [ident] then messageBody.
  return messageFromBody(
    child(n, 'ident')?.src ?? '', child(n, 'messageBody'), version)
}

function messageFromBody(
  name: string, body: Node | undefined, version: ProtoVersion,
): DescriptorProto {
  const msg: DescriptorProto = {
    name,
    field: [], nestedType: [], enumType: [], oneofDecl: [], extension: [],
  }
  const elements = body ? R(body).filter((k) => k.rule === 'messageElement') : []

  // Options first: `message_set_wire_format` changes what `to max` means in
  // an extension/reserved range, and protoc applies it wherever the option
  // sits in the body.
  for (const el of elements) {
    if (isOptionStmt(el)) msg.options = { ...(msg.options || {}), ...optionFrom(el) }
  }
  const ro: RangeOpts = {
    exclusive: true,
    max: true === msg.options?.message_set_wire_format
      ? MAX_MESSAGE_SET_END : MAX_FIELD_NUMBER_END,
  }

  for (const el of elements) {
    if (!isOptionStmt(el)) addMessageElement(el, version, msg, ro)
  }
  generateSyntheticOneofs(msg)
  return msg
}

// An `option` statement, as opposed to an `optional`-labelled field (whose
// `kw` is empty because the `label` node starts the src).
function isOptionStmt(el: Node): boolean {
  return kw(el).startsWith('option')
}

// protoc synthesises a single-field oneof for every proto3 explicit
// `optional` field, appended after the declared oneofs. The name is the
// field name prefixed with `_`, then prefixed with `X` until unique.
function generateSyntheticOneofs(msg: DescriptorProto): void {
  const names = new Set<string>()
  for (const f of msg.field) names.add(f.name)
  for (const o of msg.oneofDecl) names.add(o.name)
  for (const f of msg.field) {
    if (true !== f.proto3Optional || undefined !== f.oneofIndex) continue
    let oneofName = '_' + f.name
    while (names.has(oneofName)) oneofName = 'X' + oneofName
    names.add(oneofName)
    f.oneofIndex = msg.oneofDecl.length
    msg.oneofDecl.push({ name: oneofName })
  }
}

function addMessageElement(
  el: Node, version: ProtoVersion, msg: DescriptorProto, ro: RangeOpts,
): void {
  const k = kw(el)
  const first = R(el)[0]
  if (k.startsWith('map<')) { msg.field.push(buildMapField(el, version, msg)); return }
  if (k.startsWith('oneof')) { addOneof(el, version, msg); return }
  if (k.startsWith('export') || k.startsWith('local')) {
    // edition 2024 symbol visibility wraps the message/enum as a child node.
    addVisible(el, k, version, msg.nestedType, msg.enumType)
    return
  }
  if (isGroup(el)) { msg.field.push(buildGroup(el, version, msg)); return }
  if (k.startsWith('message')) { msg.nestedType.push(buildMessage(el, version)); return }
  if (k.startsWith('enum')) { msg.enumType.push(buildEnum(el)); return }
  if (k.startsWith('reserved')) { addReserved(el, msg, ro); return }
  if (k.startsWith('extensions')) {
    const opts = plainOptions(child(el, 'fieldOptions'))
    const rs = ranges(child(el, 'ranges'), ro)
    // A compound `extensions 2, 9 to 11 [(i) = 5];` puts the options on
    // every range, as protoc does.
    for (const r of rs) if (opts) r.options = { ...opts }
    ;(msg.extensionRange = msg.extensionRange || []).push(...rs)
    return
  }
  if (k.startsWith('extend')) { addExtend(el, version, msg.extension); return }
  if (';' === el.src) return // emptyStmt
  // No keyword and a fieldType/label lead => a field.
  if (first && (first.rule === 'fieldType' || first.rule === 'label')) {
    msg.field.push(buildField(el, version))
  }
}

// edition 2024 `export` / `local` on a message or enum declaration. The
// wrapped `message` / `enumDef` stays a child node instead of inlining.
function addVisible(
  el: Node, k: string, version: ProtoVersion,
  messages: DescriptorProto[], enums: EnumDescriptorProto[],
): void {
  const vis: SymbolVisibility =
    k.startsWith('export') ? 'VISIBILITY_EXPORT' : 'VISIBILITY_LOCAL'
  const m = child(el, 'message')
  const e = child(el, 'enumDef')
  if (m) messages.push({ ...buildMessage(m, version), visibility: vis })
  else if (e) enums.push({ ...buildEnum(e), visibility: vis })
}

function addOneof(el: Node, version: ProtoVersion, msg: DescriptorProto): void {
  const name = child(el, 'ident')?.src ?? ''
  const index = msg.oneofDecl.length
  msg.oneofDecl.push({ name })
  for (const of of R(el).filter((k) => k.rule === 'oneofElement')) {
    if (kw(of).startsWith('option')) continue
    if (';' === of.src) continue
    const f = isGroup(of) ? buildGroup(of, version, msg) : buildField(of, version)
    f.oneofIndex = index
    delete f.proto3Optional // explicit oneof members aren't proto3-optional
    msg.field.push(f)
  }
}

function addExtend(el: Node, version: ProtoVersion, into: FieldDescriptorProto[]): void {
  // extend messageType "{" *field "}" — fields inline as messageElement-like.
  const extendee = child(el, 'messageType')?.src
  for (const f of R(el).filter((k) => k.rule === 'field' || k.rule === 'messageElement')) {
    const fd = buildField(f, version)
    if (extendee) fd.extendee = extendee
    into.push(fd)
  }
}

// ---- services ------------------------------------------------------------

function buildService(n: Node): ServiceDescriptorProto {
  const svc: ServiceDescriptorProto = { name: child(n, 'ident')?.src ?? '', method: [] }
  for (const el of R(n).filter((k) => k.rule === 'serviceElement')) {
    if (kw(el).startsWith('rpc')) svc.method.push(buildRpc(el))
    else if (isOptionStmt(el)) svc.options = { ...(svc.options || {}), ...optionFrom(el) }
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
  for (const o of R(el).filter((k) => k.rule === 'optionStmt')) {
    m.options = { ...(m.options || {}), ...optionFrom(o) }
  }
  return m
}

// ---- file ----------------------------------------------------------------

export function buildFile(proto: Node, version: ProtoVersion): FileDescriptorProto {
  const file: FileDescriptorProto = {
    dependency: [], publicDependency: [], weakDependency: [],
    messageType: [], enumType: [], service: [], extension: [],
  }
  if (isEdition(version)) {
    file.edition = 'EDITION_' + version
    file.syntax = 'editions'
  } else {
    file.syntax = version
  }

  for (const def of R(proto).filter((k) => k.rule === 'topLevelDef')) {
    const k = kw(def)
    if (k.startsWith('package')) {
      file.package = child(def, 'fullIdent')?.src
    } else if (k.startsWith('import')) {
      const s = child(def, 'strLit')
      if (s) {
        // `import option "x";` (edition 2024) is a separate dependency list.
        if (k.includes('option')) {
          (file.optionDependency = file.optionDependency || []).push(unquote(s.src))
        } else {
          const idx = file.dependency.length
          file.dependency.push(unquote(s.src))
          if (k.includes('public')) file.publicDependency.push(idx)
          if (k.includes('weak')) file.weakDependency.push(idx)
        }
      }
    } else if (isOptionStmt(def)) {
      file.options = { ...(file.options || {}), ...optionFrom(def) }
    } else if (k.startsWith('export') || k.startsWith('local')) {
      addVisible(def, k, version, file.messageType, file.enumType)
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
