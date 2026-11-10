/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// What the parser reads as the lexer gives it its tokens, a token at a
// time, so that nothing waits for the document's end: the nesting cap,
// and, in a parse from a reader, the text of an aggregate option value.
// Port of ts/src/read.ts, which explains both in full.
//
// The cap: readToken counts the braces among the tokens the lexer hands
// the parser, ignored ones and the brace a word or a number is cut with
// included, and turns the brace that opens a level past MaxNestingDepth
// into a bad token, which the parser refuses at once, at that brace, with
// the code proto_too_deep. No tree deeper than the cap is built, from a
// text or from a reader. Preflight is the same cap counted over a whole
// text, which Parse and ParseTree still make first.
//
// The aggregate's text: a parse from a reader holds a window of the
// source, never the whole of it, so readToken keeps protoc's column as the
// tokens go by, keeps the tokens' text from the brace that opens an
// option's aggregate value (the first after an `=`) to the brace that
// closes it, and gives each closing brace the text protoc records for the
// value it closes, which recordAggregate reads.

import (
	"fmt"

	tabnas "github.com/tabnas/parser/go"
)

// tooDeep is the code the parser refuses a document nesting past the cap
// with; tooDeepMessage and tooDeepHint are its message and hint, which
// Proto declares.
const tooDeep = "proto_too_deep"

var (
	tooDeepMessage = fmt.Sprintf(
		"proto: document nests deeper than the %d levels this parser accepts", MaxNestingDepth)
	tooDeepHint = fmt.Sprintf(
		"A .proto document nests at most %d levels of braces, counting every block "+
			"and every aggregate option value, the recursion budget protoc carries too. "+
			"This brace opens one level more.", MaxNestingDepth)
)

// readingKey is the member of a parse's Context.U that holds what
// readToken has read.
const readingKey = "proto$read"

// openAggregate is an aggregate value whose closing brace has not been
// read yet: where its opening brace is, where its text starts in
// reading.text, and protoc's column after its opening brace.
type openAggregate struct{ at, from, col int }

// reading is what readToken has read of one parse.
type reading struct {
	// next is where the token after the last one read starts: a token the
	// lexer gives again is not read again.
	next int
	// depth is the braces open there.
	depth int
	// at is protoc's place there, kept in a parse from a reader.
	at at
	// eq is whether the last token read that is not space, a line break or
	// a comment was `=`.
	eq bool
	// text is the text from the outermost open aggregate's opening brace on.
	text []byte
	// open is the aggregates open, outermost first.
	open []openAggregate
}

// readToken is the lex subscriber Proto installs.
func readToken(tkn *tabnas.Token, _ *tabnas.Rule, ctx *tabnas.Context) {
	if ctx == nil {
		return
	}
	r, _ := ctx.U[readingKey].(*reading)
	if r == nil {
		if ctx.U == nil {
			ctx.U = map[string]any{}
		}
		r = &reading{}
		ctx.U[readingKey] = r
	}
	if tkn.SI < r.next {
		return
	}
	r.next = tkn.SI + len(tkn.Src)
	switch tkn.Src {
	case "{":
		if MaxNestingDepth <= r.depth {
			tkn.Tin = tabnas.TinBD
			tkn.Name = "#BD"
			tkn.Why = tooDeep
			tkn.Err = tooDeep
			return
		}
		r.depth++
	case "}":
		if 0 < r.depth {
			r.depth--
		}
	}
	if ctx.Lex != nil && ctx.Lex.Windowed() {
		r.readAggregate(tkn)
	}
}

// readAggregate keeps, in a parse from a reader, protoc's column and the
// text of the aggregates open, which a closing brace takes away in its Use.
func (r *reading) readAggregate(tkn *tabnas.Token) {
	src := tkn.Src
	switch {
	case src == "{" && (r.eq || 0 < len(r.open)):
		if 0 < len(r.open) {
			r.text = append(r.text, src...)
		}
		r.advance(src)
		r.open = append(r.open, openAggregate{at: tkn.SI, from: len(r.text), col: r.at.col})
	case src == "}" && 0 < len(r.open):
		value := r.open[len(r.open)-1]
		r.open = r.open[:len(r.open)-1]
		text := string(r.text)
		if tkn.Use == nil {
			tkn.Use = map[string]any{}
		}
		tkn.Use[aggregateKey] = aggregateRead{open: value.at, text: interiorText(text, value.from, len(text), value.col)}
		if 0 < len(r.open) {
			r.text = append(r.text, src...)
		} else {
			r.text = r.text[:0]
		}
		r.advance(src)
	default:
		if 0 < len(r.open) {
			r.text = append(r.text, src...)
		}
		r.advance(src)
	}
	if tkn.Tin != tabnas.TinSP && tkn.Tin != tabnas.TinLN && tkn.Tin != tabnas.TinCM {
		r.eq = src == "="
	}
}

// advance moves r's place past src, protoc's way.
func (r *reading) advance(src string) {
	for i := 0; i < len(src); {
		i = r.at.step(src, i)
	}
}
