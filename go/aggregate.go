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
	"strings"

	tabnas "github.com/tabnas/parser/go"
)

const tabWidth = 8

// at is a place in the text as protoc counts it: the line, and the column
// on it.
type at struct{ line, col int }

// step moves a past src[i], protoc's way, and returns the index after it.
// A Go string is bytes, so a byte offset is a column step already: a
// multi-byte character advances the column once per byte, as protoc's
// tokenizer does.
func (a *at) step(src string, i int) int {
	switch src[i] {
	case '\n':
		a.line++
		a.col = 0
	case '\t':
		a.col += tabWidth - a.col%tabWidth
	default:
		a.col++
	}
	return i + 1
}

// aggregateText is the text protoc 36 records for the aggregate whose
// braces are at src[open] and src[close].
func aggregateText(src string, open, close int) string {
	// The column after the opening brace depends on everything ahead of it
	// on its line, because that decides where a later tab stops.
	var a at
	for i := strings.LastIndexByte(src[:open], '\n') + 1; i <= open; {
		i = a.step(src, i)
	}
	return interiorText(src, open+1, close, a.col)
}

// interiorText is the text protoc records for an aggregate whose text
// between its braces is src[from:close], the column after its opening
// brace col: what aggregateText reads from the whole source, and what a
// parse from a reader, which never holds the whole source, reads from the
// tokens the lexer gave between the braces (read.go).
func interiorText(src string, from, close, col int) string {
	a := at{col: col}
	isCommentStart := func(i int) bool {
		return src[i] == '/' && i+1 < len(src) && (src[i+1] == '/' || src[i+1] == '*')
	}

	var out strings.Builder
	i := from
	for i < close {
		c := src[i]
		if isCommentStart(i) {
			out.WriteString(src[from:i])
			gapLine, gapCol := a.line, a.col
			// Comments with nothing between them form one gap: protoc pads
			// from the token before the first to the token after the last.
			for i < close && isCommentStart(i) {
				if src[i+1] == '/' {
					for i < close && src[i] != '\n' {
						i = a.step(src, i)
					}
					if i < close {
						i = a.step(src, i)
					}
				} else {
					i = a.step(src, a.step(src, i))
					for i < close && !(src[i] == '*' && i+1 < len(src) && src[i+1] == '/') {
						i = a.step(src, i)
					}
					if i < close {
						i = a.step(src, a.step(src, i))
					}
				}
			}
			from = i
			if close <= i {
				break
			}
			if gapLine < a.line {
				out.WriteString(strings.Repeat("\n", a.line-gapLine))
				out.WriteString(strings.Repeat(" ", a.col))
			} else if gapCol < a.col {
				out.WriteString(strings.Repeat(" ", a.col-gapCol))
			}
			continue
		}
		if c == '"' || c == '\'' {
			// A string's text is copied as written; a `//` inside one is not
			// a comment. protoc ends an unterminated string at the newline.
			i = a.step(src, i)
			for i < close && src[i] != c && src[i] != '\n' {
				if src[i] == '\\' && i+1 < close && src[i+1] != '\n' {
					i = a.step(src, i)
				}
				i = a.step(src, i)
			}
			if i < close && src[i] == c {
				i = a.step(src, i)
			}
			continue
		}
		i = a.step(src, i)
	}
	if from < close {
		out.WriteString(src[from:close])
	}
	return out.String()
}

// aggregateKey is the member of a closing brace's Use that holds, in a
// parse from a reader, the aggregate it closes (read.go).
const aggregateKey = "protoAggregate"

// aggregateRead is what a closing brace carries in a parse from a reader:
// the position of the aggregate's opening brace, and its text.
type aggregateRead struct {
	open int
	text string
}

// recordAggregate is the after-close action Proto installs on the grammar's
// `constant` rule: on an aggregate value's CST node it sets "aggregate" to
// the text protoc records, read from the source between the value's braces.
// The node's "src" stays the tokens run together, as on every node. The
// opening brace is the rule's first token (O0) and the closing brace the
// last token consumed (V1); a node where either is not a brace is left
// without "aggregate". A parse from a reader holds a window of the source,
// never the whole of it: there the closing brace carries the text, which
// readToken (read.go) kept as the tokens between the braces went by. Port
// of recordAggregate in ts/src/aggregate.ts.
func recordAggregate(r *tabnas.Rule, ctx *tabnas.Context) {
	node, ok := r.Node.(map[string]any)
	if !ok || nrule(node) != "constant" {
		return
	}
	s, _ := node["src"].(string)
	if s == "" || s[0] != '{' || r.O0 == nil || ctx.V1 == nil || ctx.Lex == nil {
		return
	}
	open, close := r.O0.SI, ctx.V1.SI
	if ctx.Lex.Windowed() {
		if read, ok := ctx.V1.Use[aggregateKey].(aggregateRead); ok && read.open == open {
			node["aggregate"] = read.text
		}
		return
	}
	src := ctx.Lex.Src
	if !(0 <= open && open < close && close < len(src)) {
		return
	}
	if src[open] != '{' || src[close] != '}' {
		return
	}
	node["aggregate"] = aggregateText(src, open, close)
}
