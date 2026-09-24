/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// A string value written as adjacent literals: `option (f) = "a" "b";`.
//
// protoc reads a string wherever it reads one through
// `Parser::ConsumeString` (src/google/protobuf/compiler/parser.cc), which
// takes every string literal that follows the first, as C does, and
// records ONE string: each literal decoded by the tokenizer's
// `ParseStringAppend` (src/google/protobuf/io/tokenizer.cc) and the results
// concatenated. That covers `syntax`, `edition`, every `import`, every
// option's string value, `default`, `json_name` and a reserved name.
//
// This module is that reading. A value written as ONE literal keeps the
// text between its quotes as written, escapes included, as it always has
// here; see the declared deviation in the root AGENTS.md.

// The literals a CST `src` holds, in order. `src` is the literals' own
// text run together (the lexer drops the space between them), so each
// literal ends at the first unescaped copy of the quote it opened with.
// Returns null when `src` is not one or more whole literals.
function splitLiterals(src: string): string[] | null {
  const out: string[] = []
  let i = 0
  while (i < src.length) {
    const q = src[i]
    if ('"' !== q && "'" !== q) return null
    let j = i + 1
    while (j < src.length && q !== src[j]) j += '\\' === src[j] ? 2 : 1
    if (src.length <= j) return null
    out.push(src.slice(i, j + 1))
    i = j + 1
  }
  return 0 < out.length ? out : null
}

const isOctal = (c: number): boolean => 0x30 <= c && c <= 0x37
const isHex = (c: number): boolean =>
  (0x30 <= c && c <= 0x39) || (0x41 <= c && c <= 0x46) || (0x61 <= c && c <= 0x66)

// A digit's value in any base up to 36, as the tokenizer's `DigitValue`
// reads it: 0-9, then a-z and A-Z from 10.
function digitValue(c: number): number {
  if (0x30 <= c && c <= 0x39) return c - 0x30
  if (0x41 <= c && c <= 0x5a) return c - 0x41 + 10
  if (0x61 <= c && c <= 0x7a) return c - 0x61 + 10
  return 36
}

// The escape letters `TranslateEscape` knows, as the byte each stands for.
const ESCAPES: Record<number, number> = {
  0x61: 0x07, 0x62: 0x08, 0x66: 0x0c, 0x6e: 0x0a, 0x72: 0x0d, 0x74: 0x09,
  0x76: 0x0b, 0x5c: 0x5c, 0x3f: 0x3f, 0x27: 0x27, 0x22: 0x22,
}

// Append a code point as UTF-8, as the tokenizer's `AppendUTF8` does: a
// surrogate is encoded like any other code point, and one past U+10FFFF
// is written out as the text `\U` and eight lower-case hex digits.
function appendUtf8(cp: number, out: number[]): void {
  if (cp <= 0x7f) out.push(cp)
  else if (cp <= 0x7ff) out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f))
  else if (cp <= 0xffff) {
    out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f))
  } else if (cp <= 0x10ffff) {
    out.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f),
      0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f))
  } else {
    for (const c of '\\U' + cp.toString(16).padStart(8, '0')) out.push(c.charCodeAt(0))
  }
}

// `ReadHexDigits`: `len` digits read as hex, or null where the text ends
// first. The tokenizer has already refused a `\u` or `\U` that is not
// followed by hex digits, so no other check is made.
function readHex(b: number[], at: number, len: number): number | null {
  if (b.length < at + len) return null
  let v = 0
  for (let k = 0; k < len; k++) v = v * 16 + digitValue(b[at + k])
  return v
}

// One literal, quotes included, decoded as `ParseStringAppend` decodes it,
// appended to `out` as bytes.
function decodeLiteral(text: string, out: number[]): void {
  const b = Array.from(new TextEncoder().encode(text))
  const quote = b[0]
  for (let i = 1; i < b.length; i++) {
    const c = b[i]
    if (0x5c === c && i + 1 < b.length) {
      const e = b[++i]
      if (isOctal(e)) {
        let code = digitValue(e)
        if (i + 1 < b.length && isOctal(b[i + 1])) code = code * 8 + digitValue(b[++i])
        if (i + 1 < b.length && isOctal(b[i + 1])) code = code * 8 + digitValue(b[++i])
        out.push(code & 0xff)
      } else if (0x78 === e || 0x58 === e) {
        let code = 0
        if (i + 1 < b.length && isHex(b[i + 1])) code = digitValue(b[++i])
        if (i + 1 < b.length && isHex(b[i + 1])) code = code * 16 + digitValue(b[++i])
        out.push(code)
      } else if (0x75 === e || 0x55 === e) {
        const len = 0x75 === e ? 4 : 8
        let cp = readHex(b, i + 1, len)
        if (null == cp) {
          out.push(e)
          continue
        }
        let next = i + 1 + len
        // A head surrogate followed by `\u` and a trail surrogate is one
        // code point; a lone one is emitted as it stands.
        if (0xd800 <= cp && cp < 0xdc00 && 0x5c === b[next] && 0x75 === b[next + 1]) {
          const trail = readHex(b, next + 2, 4)
          if (null != trail && 0xdc00 <= trail && trail < 0xe000) {
            cp = 0x10000 + (((cp - 0xd800) << 10) | (trail - 0xdc00))
            next += 6
          }
        }
        appendUtf8(cp, out)
        i = next - 1
      } else {
        out.push(ESCAPES[e] ?? 0x3f)
      }
    } else if (c === quote && i === b.length - 1) {
      // The closing quote.
    } else {
      out.push(c)
    }
  }
}

// Bytes as text. A value that is not UTF-8 has each ill-formed sequence
// replaced by U+FFFD, as the WHATWG decoder does (the maximal subpart
// rule); the descriptor's JSON holds text, and protoc's own bytes have no
// other spelling in it. A leading byte order mark is kept: it is part of
// the value.
function utf8Text(bytes: number[]): string {
  return new TextDecoder('utf-8', { ignoreBOM: true }).decode(Uint8Array.from(bytes))
}

// absl::CEscape, which protoc applies to a `bytes` field's default: the
// usual C escapes, and every other byte outside printable ASCII as three
// octal digits.
function cEscape(bytes: number[]): string {
  let out = ''
  for (const c of bytes) {
    if (0x0a === c) out += '\\n'
    else if (0x0d === c) out += '\\r'
    else if (0x09 === c) out += '\\t'
    else if (0x22 === c) out += '\\"'
    else if (0x27 === c) out += "\\'"
    else if (0x5c === c) out += '\\\\'
    else if (c < 0x20 || 0x7f <= c) out += '\\' + c.toString(8).padStart(3, '0')
    else out += String.fromCharCode(c)
  }
  return out
}

// The value protoc records for a string written as adjacent literals, or
// null when `src` is a single literal (or not literals at all), which the
// caller reads as it always has. `bytes` selects protoc's reading of a
// `bytes` field's default, which escapes the result again.
function adjacentValue(src: string, bytes = false): string | null {
  const literals = splitLiterals(src)
  if (null == literals || literals.length < 2) return null
  const out: number[] = []
  for (const lit of literals) decodeLiteral(lit, out)
  return bytes ? cEscape(out) : utf8Text(out)
}

export { adjacentValue, splitLiterals }
