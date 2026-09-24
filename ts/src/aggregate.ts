/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// An aggregate option value: `option (foo) = { a: 1 };`.
//
// protoc's parser does not read the text between the braces. It records
// that text as `UninterpretedOption.aggregate_value`, and the text-format
// parser reads it later, once the option's type is known. Since protobuf
// v36 the recorded text keeps its layout (`ParseUninterpretedBlock` in
// src/google/protobuf/compiler/parser.cc). protoc walks the tokens between
// the braces with whitespace and newlines reported as tokens of their own,
// and appends each token's text as written. Comments are not tokens, and
// text format does not read `//` or `/* */`, so before each token protoc
// pads out whatever lay between it and the token before it, the opening
// brace for the first:
//
// - a token that starts on a later line gets one newline per line crossed,
//   then as many spaces as its column;
// - a token that starts further along the same line gets the difference
//   in spaces.
//
// Whitespace is a token, so the only thing ever padded out is a run of
// comments. A comment within one line becomes spaces of its width. A
// block comment that crosses lines, or a line comment, which protoc ends
// AFTER its newline, becomes its newlines followed by spaces up to the
// column where it ends. protoc stops at the closing brace before padding
// for it, so a comment directly ahead of that brace leaves nothing.
//
// Columns are counted as protoc's tokenizer counts them: a tab moves to
// the next multiple of 8, and every other byte of UTF-8 is one column.

const TAB = 8

// The text protoc 36 records for the aggregate whose braces are at
// `src[open]` and `src[close]`.
function aggregateText(src: string, open: number, close: number): string {
  let line = 0
  let col = 0

  // Move past `src[i]`, keeping `line` and `col` protoc's way. Returns the
  // index after it: a surrogate pair is one character and four bytes.
  const step = (i: number): number => {
    const c = src.charCodeAt(i)
    if (10 === c) {
      line++
      col = 0
    } else if (9 === c) {
      col += TAB - (col % TAB)
    } else if (c < 0x80) {
      col += 1
    } else if (c < 0x800) {
      col += 2
    } else if (0xd800 <= c && c < 0xdc00 && i + 1 < src.length) {
      const d = src.charCodeAt(i + 1)
      if (0xdc00 <= d && d < 0xe000) {
        col += 4
        return i + 2
      }
      col += 3
    } else {
      col += 3
    }
    return i + 1
  }

  // The column after the opening brace depends on everything ahead of it
  // on its line, because that decides where a later tab stops.
  for (let i = src.lastIndexOf('\n', open) + 1; i <= open;) i = step(i)
  line = 0

  const isCommentStart = (i: number): boolean =>
    '/' === src[i] && ('/' === src[i + 1] || '*' === src[i + 1])

  let out = ''
  let from = open + 1
  let i = from
  while (i < close) {
    const c = src[i]
    if (isCommentStart(i)) {
      out += src.slice(from, i)
      const gapLine = line
      const gapCol = col
      // Comments with nothing between them form one gap: protoc pads from
      // the token before the first to the token after the last.
      while (i < close && isCommentStart(i)) {
        if ('/' === src[i + 1]) {
          while (i < close && '\n' !== src[i]) i = step(i)
          if (i < close) i = step(i)
        } else {
          i = step(step(i))
          while (i < close && !('*' === src[i] && '/' === src[i + 1])) i = step(i)
          if (i < close) i = step(step(i))
        }
      }
      from = i
      if (close <= i) break
      if (gapLine < line) out += '\n'.repeat(line - gapLine) + ' '.repeat(col)
      else if (gapCol < col) out += ' '.repeat(col - gapCol)
      continue
    }
    if ('"' === c || "'" === c) {
      // A string's text is copied as written; a `//` inside one is not a
      // comment. protoc ends an unterminated string at the newline.
      i = step(i)
      while (i < close && c !== src[i] && '\n' !== src[i]) {
        if ('\\' === src[i] && i + 1 < close && '\n' !== src[i + 1]) i = step(i)
        i = step(i)
      }
      if (i < close && c === src[i]) i = step(i)
      continue
    }
    i = step(i)
  }
  return out + src.slice(from, close)
}

type AggregateNode = { rule?: unknown; src?: unknown; aggregate?: string }

// After-close action for the grammar's `constant` rule, which the `Proto`
// plugin installs: on an aggregate value's CST node it sets `aggregate` to
// the text protoc records, read from the source between the value's
// braces. The node's `src` stays the tokens run together, as on every
// node. The opening brace is the rule's first token (`o0`) and the closing
// brace the last token consumed (`v1`); a node where either is not a brace
// is left without `aggregate`.
function recordAggregate(rule: any, ctx: any): void {
  const node: AggregateNode | undefined = rule?.node
  if (!node || 'constant' !== node.rule) return
  if ('string' !== typeof node.src || '{' !== node.src[0]) return
  const src: string = ctx.src()
  const open: number = rule.o0?.sI
  const close: number = ctx.v1?.sI
  if (!(0 <= open && open < close && close < src.length)) return
  if ('{' !== src[open] || '}' !== src[close]) return
  node.aggregate = aggregateText(src, open, close)
}

// ---- words inside an aggregate --------------------------------------------
//
// Text format has no keywords: `message`, `optional` and `max` are field
// names and enum values like any other word. The grammar spells its own
// keywords as whole-word literals, and the lexer takes one wherever it
// occurs, so inside an aggregate every such word would be refused where
// an identifier belongs. This matcher, which the `Proto` plugin runs ahead
// of the grammar's own, lexes those words as identifiers (#TX) inside an
// aggregate value and nowhere else, as text format reads them:
//
// - a word the grammar spells as a keyword, always;
// - `true`, `false`, `null` (values to the lexer) and `export`, `local`
//   (which stay keywords in a value such as `a: local.x`, as they always
//   have), where they name a field: followed by `:`, `{` or `<`;
// - a field named by an extension or an Any type URL, `[x.y]` or
//   `[type.googleapis.com/x.Y]`, followed by `:`, `{` or `<`. Text format
//   reads the text between those brackets as one name, and so does this;
//   anywhere else a `[` opens a list.
//
// A word glued to a character that would carry the lexer's text on
// (`message/x`) is left alone: the identifier made here is always the one
// the lexer would make if the word were not a keyword.

// Every word the grammar spells as a literal, less `export` and `local`.
// test/ keeps this in step with the grammar.
const KEYWORDS = new Set([
  'syntax', 'import', 'weak', 'public', 'package', 'option', 'message',
  'required', 'optional', 'repeated', 'oneof', 'map', 'enum', 'service',
  'rpc', 'stream', 'returns', 'extend', 'extensions', 'reserved', 'to',
  'max', 'group', 'edition',
])

// Where the lexer ends a word: whitespace, the grammar's punctuation, a
// comment, or the end of the source.
const PUNCT = '{}[]:,;=()<>-.+'

const isWordStart = (c: number): boolean =>
  (0x41 <= c && c <= 0x5a) || (0x61 <= c && c <= 0x7a) || 0x5f === c
const isWordPart = (c: number): boolean => isWordStart(c) || (0x30 <= c && c <= 0x39)
const isSpace = (c: string): boolean => ' ' === c || '\t' === c || '\r' === c || '\n' === c

function endsWord(src: string, i: number): boolean {
  if (src.length <= i) return true
  const c = src[i]
  if (isSpace(c) || PUNCT.includes(c) || '#' === c) return true
  return '/' === c && ('/' === src[i + 1] || '*' === src[i + 1])
}

// The index of the first character at or after `i` that is neither space
// nor inside a comment, or src.length.
function skipSpace(src: string, i: number): number {
  while (i < src.length) {
    const c = src[i]
    if (isSpace(c)) i++
    else if ('#' === c || ('/' === c && '/' === src[i + 1])) {
      while (i < src.length && '\n' !== src[i]) i++
    } else if ('/' === c && '*' === src[i + 1]) {
      const end = src.indexOf('*/', i + 2)
      i = -1 === end ? src.length : end + 2
    } else break
  }
  return i
}

// Does a field name end at `i`: is the next thing a `:`, `{` or `<`?
const namesField = (src: string, i: number): boolean => {
  const c = src[skipSpace(src, i)]
  return ':' === c || '{' === c || '<' === c
}

// A type name as text format checks one: identifiers joined by dots.
const TYPE_NAME = /^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$/

// `[x.y]` or `[type.googleapis.com/x.Y]` at `open`: the index after its
// `]` and the name with its spaces and comments left out, or null. The
// name is checked as text format checks it (`ConsumeAnyTypeUrlOrFullTypeName`
// in protobuf's text_format.cc): after the last `/`, if there is one, a
// type name; before it, a prefix that does not start with `/`.
function bracketName(src: string, open: number): { end: number; name: string } | null {
  let i = open + 1
  let name = ''
  for (;;) {
    i = skipSpace(src, i)
    const c = src.charCodeAt(i)
    if (isWordPart(c) || 0x2e === c || 0x2f === c || 0x2d === c) {
      name += src[i++]
      continue
    }
    if (']' !== src[i]) return null
    const slash = name.lastIndexOf('/')
    if (!TYPE_NAME.test(name.slice(slash + 1)) || 0 === name.indexOf('/')) return null
    return { end: i + 1, name: '[' + name + ']' }
  }
}

const isOpenBrace = (t: any): boolean => null != t && '{' === t.src

// Is the lexer inside an aggregate value? The value is the `constant` rule
// whose first token is its `{`. While that rule is still choosing its
// alternative it peeks the tokens after the brace itself, so the rule
// asking may be that `constant` with the brace already in the lookahead;
// after that, it is one of the rules below it.
function inAggregate(lex: any, rule: any): boolean {
  if ('constant' === rule?.name && isOpenBrace(lex.ctx?.t?.[0])) return true
  for (let r = rule, n = 0; r && r.name && n < 100000; r = r.parent, n++) {
    if ('constant' === r.name && isOpenBrace(r.o0)) return true
    if (r === r.parent) break
  }
  return false
}

// The lexer matcher. Returns an identifier token, or undefined to let the
// grammar's own matchers read the text.
function aggregateWord(lex: any, rule: any): any {
  const src: string = lex.src
  const pnt = lex.pnt
  const at: number = pnt.sI
  const c = src.charCodeAt(at)
  let end: number
  let text: string
  if (0x5b === c) {
    const bn = bracketName(src, at)
    if (null == bn || !namesField(src, bn.end)) return undefined
    end = bn.end
    text = bn.name
  } else if (isWordStart(c)) {
    end = at + 1
    while (end < src.length && isWordPart(src.charCodeAt(end))) end++
    if (!endsWord(src, end)) return undefined
    text = src.slice(at, end)
    const lower = text.toLowerCase()
    const name = 'true' === text || 'false' === text || 'null' === text ||
      'export' === lower || 'local' === lower
    if (!KEYWORDS.has(lower) && !(name && namesField(src, end))) return undefined
  } else {
    return undefined
  }
  if (!inAggregate(lex, rule)) return undefined
  const tkn = lex.token('#TX', text, text, pnt, undefined, undefined, end - at)
  // A bracketed name may cross lines; keep the row and column the lexer
  // keeps (a newline starts a row at column 1).
  for (let i = at; i < end; i++) {
    if ('\n' === src[i]) {
      pnt.rI++
      pnt.cI = 1
    } else {
      pnt.cI++
    }
  }
  pnt.sI = end
  return tkn
}

// The matcher's factory, for the engine's `lex.match` option.
const makeAggregateWord = () => aggregateWord

export { aggregateText, recordAggregate, makeAggregateWord, KEYWORDS }
