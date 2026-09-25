/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// A string value written as adjacent literals: `option (f) = "a" "b";`.
//
// Port of ts/src/strings.ts, which explains protoc's reading in full: a
// string wherever protoc reads one is every literal that follows, each
// decoded by the tokenizer's ParseStringAppend and the bytes run together.
// A value written as ONE literal keeps the text between its quotes as
// written, escapes included, as it always has here. Adjacent literals that
// protoc's tokenizer refuses are refused (adjacentStrings below).

import (
	"fmt"
	"strings"
	"unicode/utf8"

	tabnas "github.com/tabnas/parser/go"
)

// splitLiterals returns the literals a CST src holds, in order, or nil when
// src is not one or more whole literals. src is the literals' own text run
// together, so each literal ends at the first unescaped copy of the quote it
// opened with.
func splitLiterals(src string) []string {
	var out []string
	i := 0
	for i < len(src) {
		q := src[i]
		if q != '"' && q != '\'' {
			return nil
		}
		j := i + 1
		for j < len(src) && src[j] != q {
			if src[j] == '\\' {
				j += 2
			} else {
				j++
			}
		}
		if j >= len(src) {
			return nil
		}
		out = append(out, src[i:j+1])
		i = j + 1
	}
	return out
}

func isOctal(c byte) bool { return '0' <= c && c <= '7' }

func isHex(c byte) bool {
	return ('0' <= c && c <= '9') || ('A' <= c && c <= 'F') || ('a' <= c && c <= 'f')
}

// digitValue is a digit's value in any base up to 36, as the tokenizer's
// DigitValue reads it.
func digitValue(c byte) int {
	switch {
	case '0' <= c && c <= '9':
		return int(c - '0')
	case 'A' <= c && c <= 'Z':
		return int(c-'A') + 10
	case 'a' <= c && c <= 'z':
		return int(c-'a') + 10
	}
	return 36
}

// escapes are the letters TranslateEscape knows, as the byte each stands for.
var escapes = map[byte]byte{
	'a': 0x07, 'b': 0x08, 'f': 0x0c, 'n': 0x0a, 'r': 0x0d, 't': 0x09,
	'v': 0x0b, '\\': '\\', '?': '?', '\'': '\'', '"': '"',
}

// appendUTF8 appends a code point as the tokenizer's AppendUTF8 does: a
// surrogate is encoded like any other code point, and one past U+10FFFF is
// written out as `\U` and eight lower-case hex digits.
func appendUTF8(cp int, out []byte) []byte {
	switch {
	case cp <= 0x7f:
		return append(out, byte(cp))
	case cp <= 0x7ff:
		return append(out, byte(0xc0|cp>>6), byte(0x80|cp&0x3f))
	case cp <= 0xffff:
		return append(out, byte(0xe0|cp>>12), byte(0x80|(cp>>6)&0x3f), byte(0x80|cp&0x3f))
	case cp <= 0x10ffff:
		return append(out, byte(0xf0|cp>>18), byte(0x80|(cp>>12)&0x3f),
			byte(0x80|(cp>>6)&0x3f), byte(0x80|cp&0x3f))
	}
	return append(out, fmt.Sprintf("\\U%08x", cp)...)
}

// readHex is ReadHexDigits: n digits read as hex, or false where the text
// ends first.
func readHex(b string, at, n int) (int, bool) {
	if len(b) < at+n {
		return 0, false
	}
	v := 0
	for k := 0; k < n; k++ {
		v = v*16 + digitValue(b[at+k])
	}
	return v, true
}

// decodeLiteral decodes one literal, quotes included, as ParseStringAppend
// decodes it, appending the bytes to out.
func decodeLiteral(b string, out []byte) []byte {
	quote := b[0]
	for i := 1; i < len(b); i++ {
		c := b[i]
		switch {
		case c == '\\' && i+1 < len(b):
			i++
			e := b[i]
			switch {
			case isOctal(e):
				code := digitValue(e)
				if i+1 < len(b) && isOctal(b[i+1]) {
					i++
					code = code*8 + digitValue(b[i])
				}
				if i+1 < len(b) && isOctal(b[i+1]) {
					i++
					code = code*8 + digitValue(b[i])
				}
				out = append(out, byte(code&0xff))
			case e == 'x' || e == 'X':
				code := 0
				if i+1 < len(b) && isHex(b[i+1]) {
					i++
					code = digitValue(b[i])
				}
				if i+1 < len(b) && isHex(b[i+1]) {
					i++
					code = code*16 + digitValue(b[i])
				}
				out = append(out, byte(code))
			case e == 'u' || e == 'U':
				n := 4
				if e == 'U' {
					n = 8
				}
				cp, ok := readHex(b, i+1, n)
				if !ok {
					out = append(out, e)
					continue
				}
				next := i + 1 + n
				// A head surrogate followed by `\u` and a trail surrogate is
				// one code point; a lone one is emitted as it stands.
				if 0xd800 <= cp && cp < 0xdc00 && next+1 < len(b) && b[next] == '\\' && b[next+1] == 'u' {
					if trail, ok := readHex(b, next+2, 4); ok && 0xdc00 <= trail && trail < 0xe000 {
						cp = 0x10000 + ((cp-0xd800)<<10 | (trail - 0xdc00))
						next += 6
					}
				}
				out = appendUTF8(cp, out)
				i = next - 1
			default:
				if r, ok := escapes[e]; ok {
					out = append(out, r)
				} else {
					out = append(out, '?')
				}
			}
		case c == quote && i == len(b)-1:
			// The closing quote.
		default:
			out = append(out, c)
		}
	}
	return out
}

// utf8Text reads bytes as text, each ill-formed sequence replaced by U+FFFD
// as the WHATWG decoder replaces it (the maximal subpart rule), so every
// runtime records the same text. A leading byte order mark is kept.
func utf8Text(b []byte) string {
	var sb strings.Builder
	for i := 0; i < len(b); {
		c := b[i]
		if c < 0x80 {
			sb.WriteByte(c)
			i++
			continue
		}
		need, cp := 0, rune(0)
		lower, upper := byte(0x80), byte(0xbf)
		switch {
		case 0xc2 <= c && c <= 0xdf:
			need, cp = 1, rune(c&0x1f)
		case 0xe0 <= c && c <= 0xef:
			need, cp = 2, rune(c&0x0f)
			if c == 0xe0 {
				lower = 0xa0
			} else if c == 0xed {
				upper = 0x9f
			}
		case 0xf0 <= c && c <= 0xf4:
			need, cp = 3, rune(c&0x07)
			if c == 0xf0 {
				lower = 0x90
			} else if c == 0xf4 {
				upper = 0x8f
			}
		default:
			sb.WriteRune(utf8.RuneError)
			i++
			continue
		}
		j := i + 1
		ok := true
		for k := 0; k < need; k++ {
			if j >= len(b) || b[j] < lower || upper < b[j] {
				ok = false
				break
			}
			cp = cp<<6 | rune(b[j]&0x3f)
			lower, upper = 0x80, 0xbf
			j++
		}
		if ok {
			sb.WriteRune(cp)
		} else {
			// The byte that broke the sequence is read again on its own.
			sb.WriteRune(utf8.RuneError)
		}
		i = j
	}
	return sb.String()
}

// cEscape is absl::CEscape, which protoc applies to a bytes field's default:
// the usual C escapes, and every other byte outside printable ASCII as three
// octal digits.
func cEscape(b []byte) string {
	var sb strings.Builder
	for _, c := range b {
		switch {
		case c == '\n':
			sb.WriteString(`\n`)
		case c == '\r':
			sb.WriteString(`\r`)
		case c == '\t':
			sb.WriteString(`\t`)
		case c == '"':
			sb.WriteString(`\"`)
		case c == '\'':
			sb.WriteString(`\'`)
		case c == '\\':
			sb.WriteString(`\\`)
		case c < 0x20 || 0x7f <= c:
			fmt.Fprintf(&sb, "\\%03o", c)
		default:
			sb.WriteByte(c)
		}
	}
	return sb.String()
}

// adjacentValue is the value protoc records for a string written as adjacent
// literals, or false when src is a single literal (or not literals at all),
// which the caller reads as it always has. bytes selects protoc's reading of
// a bytes field's default, which escapes the result again.
func adjacentValue(src string, bytes bool) (string, bool) {
	literals := splitLiterals(src)
	if len(literals) < 2 {
		return "", false
	}
	var out []byte
	for _, lit := range literals {
		out = decodeLiteral(lit, out)
	}
	if bytes {
		return cEscape(out), true
	}
	return utf8Text(out), true
}

// stringValue is a string value as this package records it: one literal
// keeps the text between its quotes as written; adjacent literals are the
// one string protoc records for them. Port of stringValue in
// ts/src/build-descriptor.ts.
func stringValue(src string, bytes bool) string {
	if v, ok := adjacentValue(src, bytes); ok {
		return v
	}
	return unquote(src)
}

// ---- adjacent literals protoc refuses -------------------------------------
//
// Port of the matcher in ts/src/strings.ts, which explains it in full.
// protoc's tokenizer refuses a literal holding an escape it does not know,
// `\x` without a hex digit, `\u` without four, `\U` without eight that
// start `000` or `001`, and has no backtick strings. Outside an aggregate
// value, a literal that runs into another is refused where protoc refuses
// either of the two; a single literal is kept as written, as it always was.

func isQuote(c byte) bool { return c == '"' || c == '\'' || c == '`' }

// letterEscapes are the letters protoc's tokenizer takes after a backslash.
const letterEscapes = "abfnrtv\\?'\""

// literalEnd is the index after the literal that opens at at, as the tabnas
// lexer reads it: a backslash takes the byte after it. -1 where there is no
// closing quote, or where a `"` or `'` literal holds a raw control
// character, which the lexer refuses with an error of its own.
func literalEnd(src string, at int) int {
	quote := src[at]
	for i := at + 1; i < len(src); i++ {
		c := src[i]
		switch {
		case c == quote:
			return i + 1
		case quote != '`' && c < 0x20:
			return -1
		case c == '\\':
			i++
		}
	}
	return -1
}

// hexDigits reports whether there are count hex digits at from, all before
// closeAt.
func hexDigits(src string, from, count, closeAt int) bool {
	if closeAt < from+count {
		return false
	}
	for k := from; k < from+count; k++ {
		if !isHex(src[k]) {
			return false
		}
	}
	return true
}

// protocRefuses reports whether protoc's tokenizer refuses the literal
// src[at:end], reading its escapes as ConsumeString does.
func protocRefuses(src string, at, end int) bool {
	if src[at] == '`' {
		return true
	}
	closeAt := end - 1
	for i := at + 1; i < closeAt; i++ {
		if src[i] != '\\' {
			continue
		}
		i++
		e := src[i]
		switch {
		case strings.IndexByte(letterEscapes, e) >= 0 || isOctal(e):
		case e == 'x' || e == 'X':
			if !hexDigits(src, i+1, 1, closeAt) {
				return true
			}
		case e == 'u':
			if !hexDigits(src, i+1, 4, closeAt) {
				return true
			}
		case e == 'U':
			// Eight hex digits, the first three `00` and then `0` or `1`.
			if closeAt <= i+3 {
				return true
			}
			if lead := src[i+1 : i+4]; lead != "000" && lead != "001" {
				return true
			}
			if !hexDigits(src, i+4, 5, closeAt) {
				return true
			}
		default:
			return true
		}
	}
	return false
}

// adjacentStrings is the lexer matcher: a bad token for a literal that runs
// into another where protoc refuses either, spanning from the first to the
// end of the one refused, or nil to let the grammar's own matchers read the
// text. Checking each literal with the one after it covers every pair in a
// run.
func adjacentStrings(lex *tabnas.Lex, rule *tabnas.Rule) *tabnas.Token {
	src := lex.Src
	at := lex.Cursor().SI
	if at >= len(src) || !isQuote(src[at]) || inAggregate(lex, rule) {
		return nil
	}
	end := literalEnd(src, at)
	if end < 0 {
		return nil
	}
	next := skipSpace(src, end)
	if next >= len(src) || !isQuote(src[next]) {
		return nil
	}
	nextEnd := literalEnd(src, next)
	if nextEnd < 0 {
		return nil
	}
	refused := -1
	if protocRefuses(src, at, end) {
		refused = end
	} else if protocRefuses(src, next, nextEnd) {
		refused = nextEnd
	}
	if refused < 0 {
		return nil
	}
	tkn := lex.Bad("unexpected")
	tkn.Src = src[at:refused]
	return tkn
}

// makeAdjacentStrings is the matcher's factory, for the engine's Lex.Match
// option.
func makeAdjacentStrings(_ *tabnas.LexConfig, _ *tabnas.Options) tabnas.LexMatcher {
	return adjacentStrings
}
