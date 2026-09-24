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
