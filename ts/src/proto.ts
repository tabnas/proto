/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// @tabnas/proto — a Tabnas plugin that parses Protocol Buffers `.proto`
// IDL (proto2, proto3, edition 2023/2024) into FileDescriptorProto-shaped
// JSON. The version is auto-detected from the file's `syntax`/`edition`
// declaration and/or supplied via the `version` option.

import { Tabnas } from '@tabnas/parser'
import type { GrammarSpec } from '@tabnas/parser'

// The grammar, compiled at build time from proto-grammar/*.abnf by `npm
// run gen-grammar` (gen-grammar.js). Generated: never edit it.
import compiledGrammar from './proto-grammar.json'
import { buildFile } from './build-descriptor'
import { buildTree } from './build-tree'
import type { TreeNode, TreeValue } from './build-tree'
import { recordAggregate } from './aggregate'
import { MAX_NESTING_DEPTH, preflight } from './preflight'
import { TOO_DEEP, TOO_DEEP_HINT, TOO_DEEP_MESSAGE, readToken } from './read'
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

// The engine's GrammarSpec type describes a match token as a RegExp; the
// serialized form carries it as an `@~/…/` string, which `tn.grammar()`
// resolves on install. Hence the cast.
const COMPILED = compiledGrammar as unknown as GrammarSpec

// The Tabnas plugin: installs the union proto grammar so the engine can
// parse `.proto` source into a `{rule, src, kids}` CST. Use the exported
// `parse()` / `toDescriptor()` to turn that CST into a FileDescriptorProto.
const Proto = ((tn: Tabnas, _options?: Partial<ProtoOptions>) => {
  // The union grammar, compiled. gen-grammar.js ran @tabnas/abnf over the
  // union text (src/grammar.ts) at build time, with the options this
  // plugin used to pass at every install: `wordKeywords` makes a keyword
  // match as a whole word, and `tokenClasses` compiles `ident` (an
  // identifier or any keyword) to one engine token set, so a lookahead
  // position peeks it as one token rather than one alternate per keyword;
  // both are required (see AGENTS.md). The compiler wrote the CST
  // builders as the engine's own `@node$` / `@capture$` / `@bubble$`
  // builtins, so the file is pure data and installing it loads no
  // compiler. The engine installs a copy: the imported object is shared,
  // and nothing writes to it.
  tn.grammar(COMPILED)
  // An aggregate value (`option (f) = { a: 1 };`) is recorded as the text
  // between its braces, which the CST's `src` does not keep: the lexer
  // drops whitespace and comments. This action reads it from the source
  // while the brace tokens are to hand; see ./aggregate.ts.
  tn.rule('constant', (rs: any) => rs.ac(recordAggregate))
  // The nesting cap, counted as the lexer gives the parser its tokens, and
  // in a parse from a reader an aggregate's text: see ./read.ts.
  tn.options({
    error: { [TOO_DEEP]: TOO_DEEP_MESSAGE },
    hint: { [TOO_DEEP]: TOO_DEEP_HINT },
  })
  tn.sub({ lex: readToken })
}) as {
  (tn: Tabnas, options?: Partial<ProtoOptions>): void
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
//
// A document nesting deeper than MAX_NESTING_DEPTH is refused before the
// engine runs, the message naming its depth: see ./preflight.ts. The
// engine refuses one too, as it reads the brace past the cap (./read.ts).
function parse(src: string, options?: Partial<ProtoOptions>): FileDescriptorProto {
  preflight(src)
  const tn = new Tabnas({ rewind: { history: 8192 } })
  Proto(tn, options)
  return toDescriptor(tn.parse(src), options)
}

// The parser a host reads a `.proto` document with: a fresh engine with
// the grammar installed, as `parse` builds one. Its rule passes are what
// the transducer part (alchemy/transduce.alc) reads, and its value is the
// `{rule, src, kids}` CST that `toTree` and `toDescriptor` walk. It
// refuses a document nesting past MAX_NESTING_DEPTH as it reads it, with
// the code `proto_too_deep`, and declares `lex.window`, so that
// `tn.parseReader(reader)` reads a document a window at a time, never
// whole: see ./read.ts.
function make(): Tabnas {
  const tn = new Tabnas({ rewind: { history: 8192 } })
  Proto(tn)
  tn.options({ lex: { window: true } })
  return tn
}

// The source-ordered tree of a parsed proto CST: the file's declarations
// in the order the file writes them (the schema `proto-ast`; see
// ./build-tree.ts). It is the tree the translation parts read and write.
function toTree(cst: any): TreeNode {
  return buildTree(cst)
}

// Convenience: parse a `.proto` source string to its source-ordered tree
// in one call, refusing a document that nests past MAX_NESTING_DEPTH
// first, as `parse` does.
function parseTree(src: string): TreeNode {
  preflight(src)
  return toTree(make().parse(src))
}

// VERSION is this package's version. It MUST equal package.json "version":
// the release orchestrator rewrites both, and test/version.test.ts fails the
// build if they drift. Mirrors `const VERSION` in go/proto.go.
const VERSION = '0.6.13'

export {
  Proto, parse, toDescriptor, parseTree, toTree, make, preflight,
  MAX_NESTING_DEPTH, VERSION,
}
export type { ProtoVersion, TreeNode, TreeValue }
export * from './descriptor'
export { translate } from './translate'
export type { TranslationPart, TranslationParts } from './translate'
