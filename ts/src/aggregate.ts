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

export { aggregateText, recordAggregate }
