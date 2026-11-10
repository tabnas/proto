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
import { TOO_DEEP, TOO_DEEP_HINT, TOO_DEEP_MESSAGE, readNesting, readToken } from './read'
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

// The compiled grammar's tree builders, the engine's builtins that build
// the `{rule, src, kids}` CST, each with the member of an alternate's `k`
// that configures it, if any.
const TREE_BUILDERS: Record<string, string | undefined> = {
  '@node$': 'node$',
  '@capture$': 'capture$',
  '@bubble$': undefined,
}

// The compiled grammar without its tree builders, which the parser
// `makeStreaming()` builds installs: the same rules, alternates, tokens and
// counters, so the same rule passes, and no node built. A builder is an
// alternate's action, which runs after the alternate has matched, and the
// grammar's conditions read counters, never a node, so taking the builders
// away changes what a parse builds and nothing of how it goes.
const STREAMING = withoutTree(compiledGrammar)

function withoutTree(spec: unknown): GrammarSpec {
  const copy = JSON.parse(JSON.stringify(spec))
  for (const rule of Object.values<any>(copy.rule)) {
    for (const state of ['open', 'close']) {
      for (const alt of rule[state] ?? []) {
        if ('string' !== typeof alt.a || !(alt.a in TREE_BUILDERS)) continue
        const config = TREE_BUILDERS[alt.a]
        delete alt.a
        if (undefined !== config && null != alt.k) {
          delete alt.k[config]
          if (0 === Object.keys(alt.k).length) delete alt.k
        }
      }
    }
  }
  return copy as GrammarSpec
}

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
// one call. Builds a fresh engine each time; for repeated parsing reuse
// `make()`'s parser and call `toDescriptor(tn.parse(src), opts)`.
//
// A document nesting deeper than MAX_NESTING_DEPTH is refused before the
// engine runs, the message naming its depth: see ./preflight.ts. The
// engine refuses one too, as it reads the brace past the cap (./read.ts).
function parse(src: string, options?: Partial<ProtoOptions>): FileDescriptorProto {
  preflight(src)
  const tn = new Tabnas()
  Proto(tn, options)
  return toDescriptor(tn.parse(src), options)
}

// Two parsers: `make()`'s for a tree, `makeStreaming()`'s for a stream.
//
// Each is a fresh engine with the grammar installed, refuses a document
// nesting past MAX_NESTING_DEPTH as it reads it, with the code
// `proto_too_deep`, and declares `lex.window`, so that
// `tn.parseReader(reader)` reads a document a window at a time, never
// whole: see ./read.ts. Both make the same rule passes, token for token,
// and fail the same documents at the same place. Their values differ, so
// neither stands in for the other.
//
// Each keeps the engine's default rewind history, 64 consumed tokens. The
// compiled grammar has no probe, so no parse rewinds, and the one reader
// of that history is `recordAggregate`, which reads the last token
// consumed (`ctx.v1`). Every document of the fixtures and the corpus
// parses alike at 64 and at 8,192, the bound proto kept through 0.7.0
// (test/history.test.ts); a longer history would hold the window's text
// back to its oldest token.

// The parser for a tree. Its value is the `{rule, src, kids}` CST that
// `toTree` and `toDescriptor` walk, as `parse` and `parseTree` build it; a
// parse from a reader keeps an aggregate's text from the tokens between its
// braces, since a window never holds the whole source. A CST holds the
// whole document: for a stream, take `makeStreaming()`.
function make(): Tabnas {
  const tn = new Tabnas()
  Proto(tn)
  tn.options({ lex: { window: true } })
  return tn
}

// The rule history the streaming parser keeps: three predecessors, the
// bound transduce sets for a stream of a grammar it has verified
// (`STREAM_RULE_HISTORY`), and the Rust engine's default.
const STREAMING_RULE_HISTORY = 3

// The parser a host streams a `.proto` document through: its rule passes,
// which the transducer part (alchemy/transduce.alc) reads a declaration at
// a time, are `make()`'s, and it builds no tree. Its grammar is the
// compiled one without the CST's builders, so a parse holds no declaration
// it has closed, nor any of their text, and its value is not the CST:
// `tn.parse(src)` returns nothing a walk takes. For a tree, take `make()`.
//
// It bounds the engine's rule history too, which is unbounded by default:
// the engine links each rule to the one it replaced and the one that
// pushed it, so an unbounded history keeps every rule of a parse, and a
// stream's memory grows with the document. A grammar that read back
// through those links past the bound would parse otherwise; this one has
// no action at all, so nothing reads them, and its passes are `make()`'s
// with the bound as without (test/streaming.test.ts).
function makeStreaming(): Tabnas {
  const tn = new Tabnas()
  tn.grammar(STREAMING)
  tn.options({
    error: { [TOO_DEEP]: TOO_DEEP_MESSAGE },
    hint: { [TOO_DEEP]: TOO_DEEP_HINT },
    lex: { window: true },
    rule: { history: STREAMING_RULE_HISTORY },
  })
  tn.sub({ lex: readNesting })
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
const VERSION = '0.7.0'

export {
  Proto, parse, toDescriptor, parseTree, toTree, make, makeStreaming, preflight,
  MAX_NESTING_DEPTH, VERSION,
}
export type { ProtoVersion, TreeNode, TreeValue }
export * from './descriptor'
export { translate } from './translate'
export type { TranslationPart, TranslationParts } from './translate'
