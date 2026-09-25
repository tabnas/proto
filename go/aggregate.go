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
//     followed by `:`, `{`, `<` or `[`;
//   - `[x.y]` or `[type.googleapis.com/x.Y]` followed by `:`, `{`, `<` or
//     `[`, the name between the brackets read as one, as text format reads
//     it.
//
// A `[` after a name opens a list of messages with the colon left out. A
// value followed by a bracketed name (`a: true [x.y]: 1`) is then read as an
// identifier too, which the grammar takes as a value: the entries and the
// text recorded are the same, and only that value's CST node differs.

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

func isDigit(c byte) bool { return '0' <= c && c <= '9' }

func isWordPart(c byte) bool { return isWordStart(c) || isDigit(c) }

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
// space nor inside a comment, or len(src). A comment ends where the lexer ends
// it: `#` and `//` at a CR or an LF, `/*` after the next `*/`.
func skipSpace(src string, i int) int {
	for i < len(src) {
		c := src[i]
		switch {
		case isLexSpace(c):
			i++
		case c == '#' || (c == '/' && i+1 < len(src) && src[i+1] == '/'):
			for i < len(src) && src[i] != '\n' && src[i] != '\r' {
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
// `:`, `{`, `<` or `[`?
func namesField(src string, i int) bool {
	j := skipSpace(src, i)
	if j >= len(src) {
		return false
	}
	c := src[j]
	return c == ':' || c == '{' || c == '<' || c == '['
}

// typeNameRe is a type name as text format checks one.
var typeNameRe = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$`)

// bracketName reads `[x.y]` or `[type.googleapis.com/x.Y]` at open: the index
// after its `]` and the name with its spaces and comments left out. Port of
// bracketName in ts/src/aggregate.ts, which explains the two readings the
// name must pass: protoc's tokenizer's, which refuses a malformed number
// (numberEnd) and a decimal point with a digit after it directly behind an
// identifier; and text format's, which joins the pieces and wants a type
// name after the last `/` and a prefix that does not start with `/`. Only
// letters, digits, `_`, `.`, `/` and `-` are read, and the sign of a
// number's exponent, where text format also allows the URL characters
// `~!$&()*+,;=%` in a prefix.
func bracketName(src string, open int) (int, string, bool) {
	i := open + 1
	var name strings.Builder
	// identEnd is where the last identifier ended.
	identEnd := -1
	for {
		i = skipSpace(src, i)
		c := byteAt(src, i)
		switch {
		case isWordStart(c):
			from := i
			for isWordPart(byteAt(src, i)) {
				i++
			}
			name.WriteString(src[from:i])
			identEnd = i
			continue
		case isDigit(c) || (c == '.' && isDigit(byteAt(src, i+1))):
			if identEnd == i {
				return 0, "", false
			}
			end := numberEnd(src, i)
			if end < 0 {
				return 0, "", false
			}
			name.WriteString(src[i:end])
			i = end
			continue
		case c == '.' || c == '/' || c == '-':
			name.WriteByte(c)
			i++
			continue
		}
		if c != ']' {
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

// byteAt is src[i], or 0 past the end.
func byteAt(src string, i int) byte {
	if i < len(src) {
		return src[i]
	}
	return 0
}

// numberEnd is the index after the number protoc's tokenizer reads at at, a
// digit or a decimal point with a digit after it, or -1 where it refuses the
// number. Port of numberEnd in ts/src/aggregate.ts (protobuf's
// ConsumeNumber).
func numberEnd(src string, at int) int {
	dot, zero := src[at] == '.', src[at] == '0'
	i := at + 1
	switch c := byteAt(src, i); {
	case zero && (c == 'x' || c == 'X'):
		i++
		if !isHex(byteAt(src, i)) {
			return -1
		}
		for isHex(byteAt(src, i)) {
			i++
		}
	case zero && isDigit(c):
		for isOctal(byteAt(src, i)) {
			i++
		}
		if isDigit(byteAt(src, i)) {
			return -1
		}
	default:
		for isDigit(byteAt(src, i)) {
			i++
		}
		if !dot && byteAt(src, i) == '.' {
			i++
			for isDigit(byteAt(src, i)) {
				i++
			}
		}
		if e := byteAt(src, i); e == 'e' || e == 'E' {
			i++
			if s := byteAt(src, i); s == '-' || s == '+' {
				i++
			}
			if !isDigit(byteAt(src, i)) {
				return -1
			}
			for isDigit(byteAt(src, i)) {
				i++
			}
		}
	}
	if c := byteAt(src, i); isWordStart(c) || c == '.' {
		return -1
	}
	return i
}

func isOpenBrace(t *tabnas.Token) bool { return t != nil && t.Src == "{" }

// insideKey is the keep prop that marks a rule inside an aggregate value.
const insideKey = "protoAggregate"

// markAggregate is the before-open action Proto installs on every rule the
// grammar's `constant` rule pushes: a rule whose parent is an aggregate
// value's `constant`, the one whose first token is its `{`, is marked. The
// engine copies keep props to each rule pushed below a rule and to a rule
// that replaces one, so each rule inside the value carries the mark and no
// rule outside does. Port of markAggregate in ts/src/aggregate.ts.
func markAggregate(r *tabnas.Rule, _ *tabnas.Context) {
	p := r.Parent
	if p != nil && p != tabnas.NoRule && p.Name == "constant" && isOpenBrace(p.O0) {
		r.EnsureK()[insideKey] = true
	}
}

// pushedRules is the rules the grammar's `constant` rule pushes, which
// markAggregate is installed on.
func pushedRules(rs *tabnas.RuleSpec) []string {
	seen := map[string]bool{}
	var names []string
	for _, alt := range rs.OpenAlts() {
		if alt != nil && alt.P != "" && !seen[alt.P] {
			seen[alt.P] = true
			names = append(names, alt.P)
		}
	}
	return names
}

// inAggregate reports whether the lexer is inside an aggregate value. Every
// rule inside one carries the mark markAggregate sets, bar the value's
// `constant` itself: that is inside once its first token is the `{`, and
// while it is still choosing its alternative it peeks the tokens after the
// brace, with the brace already in the lookahead. Each test is a lookup, so
// the answer costs the same however deep the rule stack is.
func inAggregate(lex *tabnas.Lex, rule *tabnas.Rule) bool {
	if rule == nil || rule == tabnas.NoRule {
		return false
	}
	if inside, _ := rule.K[insideKey].(bool); inside {
		return true
	}
	if rule.Name != "constant" {
		return false
	}
	return isOpenBrace(rule.O0) ||
		(lex.Ctx != nil && len(lex.Ctx.T) > 0 && isOpenBrace(lex.Ctx.T[0]))
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
