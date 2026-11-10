/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// What the parser reads as the lexer gives it its tokens, a token at a
// time, so that nothing waits for the document's end: the nesting cap,
// and, in a parse from a reader, the text of an aggregate option value.
// The `Proto` plugin installs `readToken`, which reads both; the parser
// `makeStreaming()` builds keeps no tree, and so no aggregate's text, and
// installs `readNesting`, which reads the cap alone.
//
// The cap. The lexer hands every token it cuts to the lex subscribers
// before the parser sees it, ignored ones and the brace a word or a number
// is cut with included. `readToken` counts the braces among them, and
// turns the brace that opens a level past MAX_NESTING_DEPTH into a bad
// token, which the parser refuses at once, at that brace, with the code
// `proto_too_deep`. So no tree deeper than the cap is ever built, whether
// the parser reads a text or a reader, and the parse of a reader refuses a
// document as it reaches the brace past the cap rather than after reading
// it all. `preflight` (./preflight.ts) is the same cap counted over a
// whole text before the engine runs, which `parse` and `parseTree` still
// make, so that their refusal names the document's depth.
//
// The aggregate's text. A parse from a reader holds a window of the
// source, never the whole of it, so `recordAggregate` (./aggregate.ts)
// cannot read an aggregate's text from the source. `readToken` keeps
// protoc's column as the tokens go by, keeps the text of the tokens from
// the brace that opens an option's aggregate value (the first after an
// `=`) to the brace that closes it, every value nested in it included, and
// gives each closing brace the text protoc records for the value it
// closes, which the action reads. A token's text is the source's, so the
// text kept is the text between the braces.

import { MAX_NESTING_DEPTH } from './preflight'
import { AGGREGATE, interiorText, step } from './aggregate'
import type { At } from './aggregate'

// The code the parser refuses a document nesting past the cap with.
const TOO_DEEP = 'proto_too_deep'

// Its message and hint, which the plugin declares (`options.error` and
// `options.hint`).
const TOO_DEEP_MESSAGE =
  `proto: document nests deeper than the ${MAX_NESTING_DEPTH} levels this parser accepts`
const TOO_DEEP_HINT =
  `A .proto document nests at most ${MAX_NESTING_DEPTH} levels of braces, counting ` +
  'every block and every aggregate option value, the recursion budget protoc ' +
  'carries too. This brace opens one level more.'

// The member of a parse's `ctx.u` that holds what `readToken` has read.
const READING = 'proto$read'

// An aggregate value whose closing brace has not been read yet: where its
// opening brace is, where its text starts in `Reading.text`, and protoc's
// column after its opening brace.
type Open = { at: number; from: number; col: number }

// What `readToken` has read of one parse.
type Reading = {
  // Where the token after the last one read starts: a token the lexer
  // gives again is not read again.
  next: number
  // The braces open there.
  depth: number
  // protoc's column there, kept in a parse from a reader.
  at: At
  // Whether the last token read that is not space, a line break or a
  // comment was `=`.
  eq: boolean
  // The text from the outermost open aggregate's opening brace on.
  text: string
  // The aggregates open, outermost first.
  open: Open[]
}

// What has been read of the parse whose context is `ctx`, made at its
// first token; or undefined where `tkn` was read already: a token the
// lexer gives again is not read again.
function readingOf(tkn: any, ctx: any): Reading | undefined {
  let reading: Reading = ctx.u[READING]
  if (undefined === reading) {
    reading = ctx.u[READING] = {
      next: 0, depth: 0, at: { line: 0, col: 0 }, eq: false, text: '', open: [],
    }
  }
  if (tkn.sI < reading.next) return undefined
  reading.next = tkn.sI + tkn.len
  return reading
}

// The cap: a brace opens a level and its closing brace ends one, and the
// brace that opens a level past the cap is made a bad token. Whether `tkn`
// is that brace.
function refused(tkn: any, ctx: any, reading: Reading): boolean {
  const fixed = ctx.cfg.fixed.token
  const tin = tkn.tin
  if (fixed['{'] === tin) {
    if (MAX_NESTING_DEPTH <= reading.depth) {
      tkn.tin = ctx.cfg.t['#BD']
      tkn.name = '#BD'
      tkn.why = TOO_DEEP
      tkn.err = TOO_DEEP
      return true
    }
    reading.depth++
  } else if (fixed['}'] === tin && 0 < reading.depth) {
    reading.depth--
  }
  return false
}

// The lex subscriber the `Proto` plugin installs: the cap, and in a parse
// from a reader the aggregates' text, which the CST keeps.
function readToken(tkn: any, _rule: any, ctx: any): void {
  const reading = readingOf(tkn, ctx)
  if (undefined === reading || refused(tkn, ctx, reading)) return
  if ('' === ctx.src()) readAggregate(tkn, ctx, reading)
}

// The lex subscriber of the parser `makeStreaming()` builds: the cap
// alone, since that parser builds no CST to keep an aggregate's text in.
function readNesting(tkn: any, _rule: any, ctx: any): void {
  const reading = readingOf(tkn, ctx)
  if (undefined !== reading) refused(tkn, ctx, reading)
}

// In a parse from a reader: protoc's column, and the text of the
// aggregates open, which a closing brace takes away in its `use`.
function readAggregate(tkn: any, ctx: any, reading: Reading): void {
  const src: string = tkn.src
  const fixed = ctx.cfg.fixed.token
  const t = ctx.cfg.t
  const tin = tkn.tin
  const open = reading.open
  if (fixed['{'] === tin && (reading.eq || 0 < open.length)) {
    if (0 < open.length) reading.text += src
    advance(src, reading.at)
    open.push({ at: tkn.sI, from: reading.text.length, col: reading.at.col })
  } else if (fixed['}'] === tin && 0 < open.length) {
    const value = open.pop() as Open
    const text = interiorText(reading.text, value.from, reading.text.length, value.col)
    tkn.use = { ...(tkn.use ?? {}), [AGGREGATE]: { open: value.at, text } }
    reading.text = 0 < open.length ? reading.text + src : ''
    advance(src, reading.at)
  } else {
    if (0 < open.length) reading.text += src
    advance(src, reading.at)
  }
  if (t['#SP'] !== tin && t['#LN'] !== tin && t['#CM'] !== tin) {
    reading.eq = fixed['='] === tin
  }
}

// Move `at` past `src`, protoc's way.
function advance(src: string, at: At): void {
  for (let i = 0; i < src.length;) i = step(src, i, at)
}

export { TOO_DEEP, TOO_DEEP_HINT, TOO_DEEP_MESSAGE, readNesting, readToken }
