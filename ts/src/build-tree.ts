/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Walk the `{rule, src, kids}` CST into the source-ordered tree, the
// schema `proto-ast` (alchemy's docs/streaming-plan.md, section 13.1):
// the file's declarations in the order the file writes them, each node an
// object whose first member, `type`, names it and whose other members
// follow in the order the statement spells them. A scalar is the text
// the source spells, its tokens run together as a CST node's `src` keeps
// them: a name, a type, a number with its sign, an option's name, and a
// string literal with its quotes and escapes as written. An aggregate
// option value is its fields, `[{name, value}]`, each value text or an
// aggregate again. An empty statement declares nothing and is left out.
//
// The transducer part (alchemy/transduce.alc) makes the same tree's
// events from the parse's rule passes; this walk is the tree a host reads
// whole, and the one the part is held to. It finds each statement the way
// ./build-descriptor.ts does, by the keyword ahead of the node's first
// child (`kw`), since the ABNF compiler inlines a leading rule into the
// dispatch node that holds it.

import {
  Node, R, kw, child, gaps, gapsBefore, optionNameOf, typeTextOf, isGroup,
  isOptionStmt, parenthesised,
} from './build-descriptor'

// A node of the tree: an object whose members are in the schema's order.
export type TreeNode = { [key: string]: TreeValue }
export type TreeValue = string | boolean | TreeValue[] | TreeNode

// A constant's value: its text, or an aggregate's fields.
function constantTree(n: Node | undefined): TreeValue {
  if (null == n) return ''
  if (!n.src.startsWith('{')) return n.src
  return R(n).filter((k) => 'messageValueEntry' === k.rule).map(aggregateField)
}

// `messageValueEntry = ident [":"] (strLit 1*strLit / constant) [","/";"]`.
// The leading `ident` is inlined, so the name is the text ahead of the
// first child, less the colon; adjacent string literals are their texts
// run together, as the entry's `src` keeps them.
function aggregateField(e: Node): TreeNode {
  const kids = R(e)
  const name = (gaps(e)[0] ?? '').replace(/:$/, '')
  const value: TreeValue =
    1 === kids.length && 'constant' === kids[0].rule
      ? constantTree(kids[0])
      : kids.map((k) => k.src).join('')
  return { name, value }
}

// `option name = constant;`, as a statement.
function optionTree(el: Node): TreeNode {
  const cst = child(el, 'constant')
  return { type: 'option', name: optionNameOf(el, cst), value: constantTree(cst) }
}

// `[name = constant, ...]`: a field's, an enum value's or an extension
// range's options.
function optionsTree(opts: Node | undefined): TreeNode[] {
  return R(opts).filter((k) => 'fieldOption' === k.rule).map((fo) => {
    const cst = child(fo, 'constant')
    return { name: optionNameOf(fo, cst), value: constantTree(cst) }
  })
}

// The members of a statement that carry `[options]`, appended where the
// statement writes them.
function withOptions(node: TreeNode, el: Node): TreeNode {
  const opts = child(el, 'fieldOptions')
  if (opts) node.options = optionsTree(opts)
  return node
}

// `[label] fieldType ident "=" fieldNumber [fieldOptions] ";"`.
function fieldTree(n: Node): TreeNode {
  const node: TreeNode = { type: 'field' }
  const label = child(n, 'label')
  if (label) node.label = label.src
  node.fieldType = typeTextOf(n)
  node.name = child(n, 'ident')?.src ?? ''
  node.number = child(n, 'fieldNumber')?.src ?? ''
  return withOptions(node, n)
}

// `[label] "group" ident "=" fieldNumber messageBody`.
function groupTree(n: Node): TreeNode {
  const node: TreeNode = { type: 'group' }
  const label = child(n, 'label')
  if (label) node.label = label.src
  node.name = child(n, 'ident')?.src ?? ''
  node.number = child(n, 'fieldNumber')?.src ?? ''
  node.body = messageBodyTree(child(n, 'messageBody'))
  return node
}

// `"map" "<" fieldType "," fieldType ">" ident "=" fieldNumber [fieldOptions] ";"`.
function mapTree(n: Node): TreeNode {
  const types = R(n).filter((k) => 'fieldType' === k.rule)
  return withOptions({
    type: 'map',
    keyType: types[0]?.src ?? '',
    valueType: types[1]?.src ?? '',
    name: child(n, 'ident')?.src ?? '',
    number: child(n, 'fieldNumber')?.src ?? '',
  }, n)
}

// The ranges of a `reserved` or `extensions` statement, each `from` and,
// where the range writes one, `to`. The leading `range` is inlined into
// `ranges`, so they are read from its text, which no number or `max`
// spells with a `t`.
function rangesTree(n: Node | undefined): TreeNode[] {
  if (null == n) return []
  return n.src.split(',').map((part): TreeNode => {
    const at = part.indexOf('to')
    return at < 0 ? { from: part } : { from: part.slice(0, at), to: part.slice(at + 2) }
  })
}

// The names of a `reserved` statement, each as written: a string literal
// with its quotes, or an identifier. They are read from the statement's
// text, cut at the commas outside a literal.
function reservedNamesTree(n: Node): string[] {
  const names = child(n, 'fieldNames')
  const src = names ? names.src : ''
  const out: string[] = []
  let from = 0
  for (let i = 0; i < src.length; i++) {
    const c = src[i]
    if ('"' === c || "'" === c || '`' === c) {
      for (i++; i < src.length && c !== src[i]; i++) if ('\\' === src[i]) i++
    } else if (',' === c) {
      out.push(src.slice(from, i))
      from = i + 1
    }
  }
  out.push(src.slice(from))
  return out
}

function reservedTree(n: Node): TreeNode {
  const ranges = child(n, 'ranges')
  return ranges
    ? { type: 'reserved', ranges: rangesTree(ranges) }
    : { type: 'reserved', names: reservedNamesTree(n) }
}

// The edition-2024 `export` / `local` modifier keeps the message or enum
// as a child node, which the walk reads with the modifier ahead of it.
function visibleTree(el: Node, k: string): TreeNode | undefined {
  const visibility = k.startsWith('export') ? 'export' : 'local'
  const m = child(el, 'message')
  if (m) return messageTree(m, visibility)
  const e = child(el, 'enumDef')
  if (e) return enumTree(e, visibility)
  return undefined
}

function messageTree(n: Node, visibility?: string): TreeNode {
  const node: TreeNode = { type: 'message' }
  if (visibility) node.visibility = visibility
  node.name = child(n, 'ident')?.src ?? ''
  node.body = messageBodyTree(child(n, 'messageBody'))
  return node
}

function messageBodyTree(body: Node | undefined): TreeNode[] {
  const out: TreeNode[] = []
  for (const el of R(body)) {
    if ('messageElement' !== el.rule) continue
    const node = messageElementTree(el)
    if (node) out.push(node)
  }
  return out
}

function messageElementTree(el: Node): TreeNode | undefined {
  const k = kw(el)
  if (isOptionStmt(el)) return optionTree(el)
  if (k.startsWith('map<')) return mapTree(el)
  if (k.startsWith('oneof')) return oneofTree(el)
  if (k.startsWith('export') || k.startsWith('local')) return visibleTree(el, k)
  if (isGroup(el)) return groupTree(el)
  if (k.startsWith('message')) return messageTree(el)
  if (k.startsWith('enum')) return enumTree(el)
  if (k.startsWith('reserved')) return reservedTree(el)
  if (k.startsWith('extensions')) {
    return withOptions({ type: 'extensions', ranges: rangesTree(child(el, 'ranges')) }, el)
  }
  if (k.startsWith('extend')) return extendTree(el)
  if (';' === el.src) return undefined
  const first = R(el)[0]
  if (first && ('fieldType' === first.rule || 'label' === first.rule)) return fieldTree(el)
  return undefined
}

function oneofTree(el: Node): TreeNode {
  const body: TreeNode[] = []
  for (const of of R(el)) {
    if ('oneofElement' !== of.rule) continue
    if (isOptionStmt(of)) body.push(optionTree(of))
    else if (';' === of.src) continue
    else body.push(isGroup(of) ? groupTree(of) : fieldTree(of))
  }
  return { type: 'oneof', name: child(el, 'ident')?.src ?? '', body }
}

function enumTree(n: Node, visibility?: string): TreeNode {
  const node: TreeNode = { type: 'enum' }
  if (visibility) node.visibility = visibility
  node.name = child(n, 'ident')?.src ?? ''
  const body: TreeNode[] = []
  for (const el of R(n)) {
    if ('enumElement' !== el.rule) continue
    // The first child says what the statement is (./build-descriptor.ts
    // has why): `optionName` an option, `ranges` or `fieldNames` a
    // `reserved`, `fieldNumber` a value.
    const first = R(el)[0]
    const rule = first?.rule
    if ('ranges' === rule || 'fieldNames' === rule) body.push(reservedTree(el))
    else if ('optionName' === rule) body.push(optionTree(el))
    else if ('fieldNumber' === rule) {
      // `ident "=" ["-"] fieldNumber`: the name and the sign are in the
      // text ahead of the number node.
      const gap = gapsBefore(el, 'fieldNumber')[0] ?? ''
      const eq = gap.indexOf('=')
      const name = eq < 0 ? gap : gap.slice(0, eq)
      const number = (gap.endsWith('-') ? '-' : '') + first.src
      body.push(withOptions({ type: 'enumValue', name, number }, el))
    }
  }
  node.body = body
  return node
}

// `extend messageType "{" *( field / emptyStmt ) "}"`.
function extendTree(n: Node): TreeNode {
  const body: TreeNode[] = []
  for (const f of R(n)) {
    if ('field' === f.rule || 'messageElement' === f.rule) body.push(fieldTree(f))
  }
  return { type: 'extend', extendee: child(n, 'messageType')?.src ?? '', body }
}

function serviceTree(n: Node): TreeNode {
  const body: TreeNode[] = []
  for (const el of R(n)) {
    if ('serviceElement' !== el.rule) continue
    if (kw(el).startsWith('rpc')) body.push(rpcTree(el))
    else if (isOptionStmt(el)) body.push(optionTree(el))
  }
  return { type: 'service', name: child(n, 'ident')?.src ?? '', body }
}

// `rpc ident "(" ["stream"] messageType ")" "returns" "(" ["stream"]
// messageType ")" ( ";" / "{" *( optionStmt / emptyStmt ) "}" )`. Whether
// each side streams is read from its parenthesised span, as the
// descriptor's walk reads it; the block is there where the statement
// ends in a brace.
function rpcTree(el: Node): TreeNode {
  const types = R(el).filter((k) => 'messageType' === k.rule)
  const [input, output] = parenthesised(el.src)
  const inputType = types[0]?.src ?? ''
  const outputType = types[1]?.src ?? ''
  const node: TreeNode = { type: 'rpc', name: child(el, 'ident')?.src ?? '' }
  if ('stream' + inputType === input) node.inputStream = true
  node.inputType = inputType
  if ('stream' + outputType === output) node.outputStream = true
  node.outputType = outputType
  if (el.src.endsWith('}')) {
    node.body = R(el).filter((k) => 'optionStmt' === k.rule).map(optionTree)
  }
  return node
}

function topLevelTree(def: Node): TreeNode | undefined {
  const k = kw(def)
  if (k.startsWith('import')) {
    const node: TreeNode = { type: 'import' }
    const modifier = k.slice('import'.length)
    if ('' !== modifier) node.modifier = modifier
    node.path = child(def, 'strLit')?.src ?? ''
    return node
  }
  if (k.startsWith('package')) {
    return { type: 'package', name: child(def, 'fullIdent')?.src ?? '' }
  }
  if (isOptionStmt(def)) return optionTree(def)
  if (k.startsWith('export') || k.startsWith('local')) return visibleTree(def, k)
  if (k.startsWith('message')) return messageTree(def)
  if (k.startsWith('enum')) return enumTree(def)
  if (k.startsWith('service')) return serviceTree(def)
  if (k.startsWith('extend')) return extendTree(def)
  return undefined
}

// The file's tree: its declarations in source order. An empty file, which
// parses to no node at all, is a file with an empty body.
export function buildTree(proto: Node | undefined | null): TreeNode {
  const body: TreeNode[] = []
  for (const k of R(proto)) {
    if ('syntaxOrEdition' === k.rule) {
      body.push({
        type: kw(k).startsWith('edition') ? 'edition' : 'syntax',
        value: child(k, 'strLit')?.src ?? '',
      })
    } else if ('topLevelDef' === k.rule) {
      const node = topLevelTree(k)
      if (node) body.push(node)
    }
  }
  return { type: 'file', body }
}
