/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// An aggregate option value: `option (foo) = { a: 1 };`.
//
// Port of ts/src/aggregate.ts, which explains protoc's rule in full. In
// short: protoc records the source text between the braces as the option's
// aggregate_value, every token, space and newline as written, with each run
// of comments replaced by the newlines and spaces that keep the next token
// on its line and column. A comment directly ahead of the closing brace
// leaves nothing. Columns are protoc's: a tab moves to the next multiple of
// 8, and every other byte is one column.

import (
	"regexp"
	"strings"

	tabnas "github.com/tabnas/parser/go"
)

const tabWidth = 8

// aggregateText is the text protoc 36 records for the aggregate whose
// braces are at src[open] and src[close].
//
// A Go string is bytes, so a byte offset is a column step already: a
// multi-byte character advances the column once per byte, as protoc's
// tokenizer does.
func aggregateText(src string, open, close int) string {
	line := 0
	col := 0
	step := func(i int) int {
		switch src[i] {
		case '\n':
			line++
			col = 0
		case '\t':
			col += tabWidth - col%tabWidth
		default:
			col++
		}
		return i + 1
	}

	// The column after the opening brace depends on everything ahead of it
	// on its line, because that decides where a later tab stops.
	for i := strings.LastIndexByte(src[:open], '\n') + 1; i <= open; {
		i = step(i)
	}
	line = 0

	isCommentStart := func(i int) bool {
		return src[i] == '/' && i+1 < len(src) && (src[i+1] == '/' || src[i+1] == '*')
	}

	var out strings.Builder
	from := open + 1
	i := from
	for i < close {
		c := src[i]
		if isCommentStart(i) {
			out.WriteString(src[from:i])
			gapLine, gapCol := line, col
			// Comments with nothing between them form one gap: protoc pads
			// from the token before the first to the token after the last.
			for i < close && isCommentStart(i) {
				if src[i+1] == '/' {
					for i < close && src[i] != '\n' {
						i = step(i)
					}
					if i < close {
						i = step(i)
					}
				} else {
					i = step(step(i))
					for i < close && !(src[i] == '*' && i+1 < len(src) && src[i+1] == '/') {
						i = step(i)
					}
					if i < close {
						i = step(step(i))
					}
				}
			}
			from = i
			if close <= i {
				break
			}
			if gapLine < line {
				out.WriteString(strings.Repeat("\n", line-gapLine))
				out.WriteString(strings.Repeat(" ", col))
			} else if gapCol < col {
				out.WriteString(strings.Repeat(" ", col-gapCol))
			}
			continue
		}
		if c == '"' || c == '\'' {
			// A string's text is copied as written; a `//` inside one is not
			// a comment. protoc ends an unterminated string at the newline.
			i = step(i)
			for i < close && src[i] != c && src[i] != '\n' {
				if src[i] == '\\' && i+1 < close && src[i+1] != '\n' {
					i = step(i)
				}
				i = step(i)
			}
			if i < close && src[i] == c {
				i = step(i)
			}
			continue
		}
		i = step(i)
	}
	if from < close {
		out.WriteString(src[from:close])
	}
	return out.String()
}

// recordAggregate is the after-close action Proto installs on the grammar's
// `constant` rule: on an aggregate value's CST node it sets "aggregate" to
// the text protoc records, read from the source between the value's braces.
// The node's "src" stays the tokens run together, as on every node. The
// opening brace is the rule's first token (O0) and the closing brace the
// last token consumed (V1); a node where either is not a brace is left
// without "aggregate". Port of recordAggregate in ts/src/aggregate.ts.
func recordAggregate(r *tabnas.Rule, ctx *tabnas.Context) {
	node, ok := r.Node.(map[string]any)
	if !ok || nrule(node) != "constant" {
		return
	}
	s, _ := node["src"].(string)
	if s == "" || s[0] != '{' || r.O0 == nil || ctx.V1 == nil || ctx.Lex == nil {
		return
	}
	src := ctx.Lex.Src
	open, close := r.O0.SI, ctx.V1.SI
	if !(0 <= open && open < close && close < len(src)) {
		return
	}
	if src[open] != '{' || src[close] != '}' {
		return
	}
	node["aggregate"] = aggregateText(src, open, close)
}

// ---- words inside an aggregate --------------------------------------------
//
// Port of the matcher in ts/src/aggregate.ts, which explains it in full.
// Text format has no keywords, so inside an aggregate value this matcher,
// which Proto runs ahead of the grammar's own, lexes as an identifier (#TX):
//
//   - a word the grammar spells as a keyword, always;
//   - `true`, `false`, `null`, `export` and `local` where they name a field:
//     followed by `:`, `{` or `<`;
//   - `[x.y]` or `[type.googleapis.com/x.Y]` followed by `:`, `{` or `<`,
//     the name between the brackets read as one, as text format reads it.

// aggregateKeywords is every word the grammar spells as a literal, less
// `export` and `local`. TestAggregateKeywordsMatchTheGrammar keeps it in
// step with the grammar.
var aggregateKeywords = map[string]bool{
	"syntax": true, "import": true, "weak": true, "public": true, "package": true,
	"option": true, "message": true, "required": true, "optional": true,
	"repeated": true, "oneof": true, "map": true, "enum": true, "service": true,
	"rpc": true, "stream": true, "returns": true, "extend": true,
	"extensions": true, "reserved": true, "to": true, "max": true, "group": true,
	"edition": true,
}

// aggregatePunct is the grammar's punctuation, where the lexer ends a word.
const aggregatePunct = "{}[]:,;=()<>-.+"

func isWordStart(c byte) bool {
	return ('A' <= c && c <= 'Z') || ('a' <= c && c <= 'z') || c == '_'
}

func isWordPart(c byte) bool { return isWordStart(c) || ('0' <= c && c <= '9') }

func isLexSpace(c byte) bool { return c == ' ' || c == '\t' || c == '\r' || c == '\n' }

// endsWord reports whether the lexer ends a word at src[i].
func endsWord(src string, i int) bool {
	if i >= len(src) {
		return true
	}
	c := src[i]
	if isLexSpace(c) || strings.IndexByte(aggregatePunct, c) >= 0 || c == '#' {
		return true
	}
	return c == '/' && i+1 < len(src) && (src[i+1] == '/' || src[i+1] == '*')
}

// skipSpace returns the index of the first byte at or after i that is neither
// space nor inside a comment, or len(src).
func skipSpace(src string, i int) int {
	for i < len(src) {
		c := src[i]
		switch {
		case isLexSpace(c):
			i++
		case c == '#' || (c == '/' && i+1 < len(src) && src[i+1] == '/'):
			for i < len(src) && src[i] != '\n' {
				i++
			}
		case c == '/' && i+1 < len(src) && src[i+1] == '*':
			end := strings.Index(src[i+2:], "*/")
			if end < 0 {
				i = len(src)
			} else {
				i += 2 + end + 2
			}
		default:
			return i
		}
	}
	return i
}

// namesField reports whether a field name ends at i: is the next thing a
// `:`, `{` or `<`?
func namesField(src string, i int) bool {
	j := skipSpace(src, i)
	if j >= len(src) {
		return false
	}
	c := src[j]
	return c == ':' || c == '{' || c == '<'
}

// typeNameRe is a type name as text format checks one.
var typeNameRe = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$`)

// bracketName reads `[x.y]` or `[type.googleapis.com/x.Y]` at open: the index
// after its `]` and the name with its spaces and comments left out. The name
// is checked as text format checks it: after the last `/`, if there is one, a
// type name; before it, a prefix that does not start with `/`.
func bracketName(src string, open int) (int, string, bool) {
	i := open + 1
	var name strings.Builder
	for {
		i = skipSpace(src, i)
		if i < len(src) {
			c := src[i]
			if isWordPart(c) || c == '.' || c == '/' || c == '-' {
				name.WriteByte(c)
				i++
				continue
			}
		}
		if i >= len(src) || src[i] != ']' {
			return 0, "", false
		}
		n := name.String()
		slash := strings.LastIndexByte(n, '/')
		if !typeNameRe.MatchString(n[slash+1:]) || strings.HasPrefix(n, "/") {
			return 0, "", false
		}
		return i + 1, "[" + n + "]", true
	}
}

func isOpenBrace(t *tabnas.Token) bool { return t != nil && t.Src == "{" }

// inAggregate reports whether the lexer is inside an aggregate value: the
// `constant` rule whose first token is its `{`. While that rule is still
// choosing its alternative it peeks the tokens after the brace itself, so
// the rule asking may be that `constant` with the brace already in the
// lookahead; after that, it is one of the rules below it.
func inAggregate(lex *tabnas.Lex, rule *tabnas.Rule) bool {
	if rule != nil && rule.Name == "constant" && lex.Ctx != nil && len(lex.Ctx.T) > 0 &&
		isOpenBrace(lex.Ctx.T[0]) {
		return true
	}
	for r, n := rule, 0; r != nil && r != tabnas.NoRule && n < 100000; r, n = r.Parent, n+1 {
		if r.Name == "constant" && isOpenBrace(r.O0) {
			return true
		}
		if r == r.Parent {
			break
		}
	}
	return false
}

// aggregateWord is the lexer matcher: an identifier token, or nil to let the
// grammar's own matchers read the text.
func aggregateWord(lex *tabnas.Lex, rule *tabnas.Rule) *tabnas.Token {
	src := lex.Src
	pnt := lex.Cursor()
	at := pnt.SI
	if at >= len(src) {
		return nil
	}
	var end int
	var text string
	c := src[at]
	switch {
	case c == '[':
		e, name, ok := bracketName(src, at)
		if !ok || !namesField(src, e) {
			return nil
		}
		end, text = e, name
	case isWordStart(c):
		end = at + 1
		for end < len(src) && isWordPart(src[end]) {
			end++
		}
		if !endsWord(src, end) {
			return nil
		}
		text = src[at:end]
		lower := strings.ToLower(text)
		name := text == "true" || text == "false" || text == "null" ||
			lower == "export" || lower == "local"
		if !aggregateKeywords[lower] && !(name && namesField(src, end)) {
			return nil
		}
	default:
		return nil
	}
	if !inAggregate(lex, rule) {
		return nil
	}
	tkn := lex.Token("#TX", tabnas.TinTX, text, text)
	// A bracketed name may cross lines; keep the row and column the lexer
	// keeps (a newline starts a row at column 1).
	for i := at; i < end; i++ {
		if src[i] == '\n' {
			pnt.RI++
			pnt.CI = 1
		} else {
			pnt.CI++
		}
	}
	pnt.SI = end
	return tkn
}

// makeAggregateWord is the matcher's factory, for the engine's Lex.Match
// option.
func makeAggregateWord(_ *tabnas.LexConfig, _ *tabnas.Options) tabnas.LexMatcher {
	return aggregateWord
}
