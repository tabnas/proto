/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// @tabnas/proto — a Tabnas plugin that parses Protocol Buffers `.proto`
// IDL (proto2, proto3, edition 2023/2024) into FileDescriptorProto-shaped
// JSON. The version is auto-detected from the file's `syntax`/`edition`
// declaration and/or supplied via the `version` option.

import { Tabnas } from '@tabnas/parser'
import { abnf } from '@tabnas/abnf'

import { grammarText } from './grammar'
import { buildFile } from './build-descriptor'
import {
  recordAggregate, makeAggregateWord, markAggregate, pushedRules,
} from './aggregate'
import { makeAdjacentStrings } from './strings'
import {
  ProtoVersion, declaredVersion, resolveVersion,
} from './detect-version'
import { FileDescriptorProto } from './descriptor'

export interface ProtoOptions {
  // Explicit protobuf version. When null, auto-detect from the file's
  // syntax/edition declaration.
  version: null | ProtoVersion
  // When true (default), error if an explicit version disagrees with the
  // file's declaration; when false the declaration wins.
  reconcile: boolean
}

type AnyTabnas = Tabnas & { abnf?: Function }

// The Tabnas plugin: installs the union proto grammar so the engine can
// parse `.proto` source into a `{rule, src, kids}` CST. Use the exported
// `parse()` / `toDescriptor()` to turn that CST into a FileDescriptorProto.
const Proto = ((tn: AnyTabnas, _options?: Partial<ProtoOptions>) => {
  // Proto drives parsing through @tabnas/abnf; ensure it is installed.
  if ('function' !== typeof tn.abnf) tn.use(abnf)
  ;(tn.abnf as Function)(grammarText, {
    tag: 'proto',
    start: 'proto',
    wordKeywords: true,
  })
  // An aggregate value (`option (f) = { a: 1 };`) is recorded as the text
  // between its braces, which the CST's `src` does not keep: the lexer
  // drops whitespace and comments. This action reads it from the source
  // while the brace tokens are to hand; see ./aggregate.ts.
  tn.rule('constant', (rs: any) => rs.ac(recordAggregate))
  // Every rule inside an aggregate value carries a mark, set on the rules
  // the `constant` rule pushes and kept by the rules below them, so the
  // matchers below can tell inside from outside at once; see
  // ./aggregate.ts.
  for (const name of pushedRules(tn.rule('constant'))) {
    tn.rule(name, (rs: any) => rs.bo(markAggregate))
  }
  // Text format has no keywords: inside an aggregate value a word the
  // grammar spells as a keyword, and a bracketed extension or Any name,
  // are identifiers. This matcher runs ahead of the grammar's own (order
  // 1e6) and reads them so there; see ./aggregate.ts. The second refuses
  // adjacent string literals that protoc's tokenizer refuses; see
  // ./strings.ts.
  tn.options({
    lex: {
      match: {
        protoAggregateWord: { order: 9e5, make: makeAggregateWord },
        protoAdjacentStrings: { order: 9.5e5, make: makeAdjacentStrings },
      },
    },
  })
}) as {
  (tn: AnyTabnas, options?: Partial<ProtoOptions>): void
  defaults: ProtoOptions
}

Proto.defaults = { version: null, reconcile: true }

// Turn a parsed proto CST into a FileDescriptorProto, resolving the
// version from the file's declaration and the supplied options.
function toDescriptor(cst: any, options?: Partial<ProtoOptions>): FileDescriptorProto {
  const opts: ProtoOptions = { ...Proto.defaults, ...(options || {}) }
  const first = (cst && cst.kids ? cst.kids : []).find((k: any) => k && k.rule)
  const declared =
    first && 'syntaxOrEdition' === first.rule ? declaredVersion(first) : null
  const version = resolveVersion(declared, opts.version, opts.reconcile)
  return buildFile(cst, version)
}

// Convenience: parse a `.proto` source string to a FileDescriptorProto in
// one call. Builds a fresh engine each time; for repeated parsing reuse an
// engine via `const j = new Tabnas().use(Proto)` and call
// `toDescriptor(j.parse(src), opts)`.
function parse(src: string, options?: Partial<ProtoOptions>): FileDescriptorProto {
  const tn = new Tabnas({ rewind: { history: 8192 } }) as AnyTabnas
  Proto(tn, options)
  return toDescriptor(tn.parse(src), options)
}

// VERSION is this package's version. It MUST equal package.json "version":
// the release orchestrator rewrites both, and test/version.test.ts fails the
// build if they drift. Mirrors `const VERSION` in go/proto.go.
const VERSION = '0.5.1'

export { Proto, parse, toDescriptor, VERSION }
export type { ProtoVersion }
export * from './descriptor'
