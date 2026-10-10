/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The nesting preflight: a .proto document nesting deeper than
// MaxNestingDepth is refused before the engine builds a tree that deep.
//
// The tree costs more than its nesting suggests. A CST node's src holds
// every token beneath it, so a document nested d levels deep builds source
// text that grows with d squared, and so do the parse's time and memory:
// unchecked, Parse grows past any memory a machine has on a document a few
// hundred kilobytes long. DIVERGENCE.md section 2 has the measurements. So
// every runtime refuses the same documents, with the same message, at the
// same cap: the Rust port's.
//
// Go port of ts/src/preflight.ts, which ports `preflight` and
// `brace_depth` in the Rust crate (rs/src/lib.rs, rs/src/build_descriptor.rs).

package tabnasproto

import (
	"fmt"
	"strings"
)

// MaxNestingDepth is how deep a document may nest, in braces. Parse
// refuses a deeper one. It sits more than an order of magnitude past
// anything a hand-written .proto nests, and matches the recursion budget
// protoc's own parser carries.
const MaxNestingDepth = 100

// Preflight refuses a .proto source that nests deeper than MaxNestingDepth,
// before anything builds a tree that deep. Parse runs it; a caller who
// drives the engine directly (Proto on an instance, then its Parse and
// ToDescriptor) runs it on the source first. The error names the depth;
// like every refusal the plugin makes itself, it is not an engine error
// and carries no code.
func Preflight(src string) error {
	if depth := braceDepth(src); depth > MaxNestingDepth {
		return fmt.Errorf("proto: document nests %d levels deep, past the %d this parser accepts",
			depth, MaxNestingDepth)
	}
	return nil
}

// keywords are the grammar's: its match tokens, each a word the lexer takes
// whole when no word character follows it. A token starts after one, so a
// quote straight after a keyword opens a string (`reserved"x";` reserves
// x). preflight_test.go holds this list to proto-grammar.json.
var keywords = map[string]bool{
	"edition": true, "enum": true, "export": true, "extend": true,
	"extensions": true, "group": true, "import": true, "local": true,
	"map": true, "max": true, "message": true, "oneof": true, "option": true,
	"optional": true, "package": true, "public": true, "repeated": true,
	"required": true, "reserved": true, "returns": true, "rpc": true,
	"service": true, "stream": true, "syntax": true, "to": true, "weak": true,
}

// separators end a token and start the next: a space, a line break, or one
// of the grammar's fixed tokens other than the braces, which the scan
// counts on their own. The test holds these to proto-grammar.json too.
const separators = " \t\r\n[]:,;=()<>-.+"

// isWord is a word character, as the keywords' (?![A-Za-z0-9_]) reads one.
func isWord(c byte) bool {
	return ('0' <= c && c <= '9') || ('A' <= c && c <= 'Z') || ('a' <= c && c <= 'z') || c == '_'
}

// braceDepth is the nesting depth of a .proto source, counted in braces,
// skipping the string literals and the comments the tabnas lexer skips,
// where it skips them:
//
//   - A line comment, // or #, runs to the next line break, and the lexer
//     breaks a line at a carriage return as well as a line feed.
//   - A string is double, single or backtick quoted, a backslash escaping
//     the character after it, and a backtick string runs across lines.
//   - A quote opens a string only where the lexer starts a token: at the
//     start, and after a space, a line break, a fixed token, a string, a
//     comment or a keyword. Inside a word, a"b, the lexer reads the quote
//     as part of the word, and the braces after it count.
//
// Over-counting is safe and under-counting is not, so an unterminated
// string or comment counts every brace inside it: the engine rejects that
// source anyway. Only ASCII bytes decide anything, and a UTF-8 sequence
// never holds one, so a byte scan counts what the TypeScript code-unit
// scan counts.
func braceDepth(src string) int {
	at, depth, deepest := 0, 0, 0
	// start says whether a token starts at at.
	start := true
	for at < len(src) {
		switch c := src[at]; {
		case c == '{':
			depth++
			if depth > deepest {
				deepest = depth
			}
			at++
			start = true
		case c == '}':
			if depth > 0 {
				depth--
			}
			at++
			start = true
		case strings.IndexByte(separators, c) >= 0:
			at++
			start = true
		case c == '#' || (c == '/' && at+1 < len(src) && src[at+1] == '/'):
			// The shared tabnas lexer reads `#` as a line comment; .proto
			// does not, which the leniency corpus records.
			for at < len(src) && src[at] != '\n' && src[at] != '\r' {
				at++
			}
			start = true
		case c == '/' && at+1 < len(src) && src[at+1] == '*':
			at += 2
			for at < len(src) && !(src[at] == '*' && at+1 < len(src) && src[at+1] == '/') {
				at++
			}
			at += 2
			start = true
		case start && (c == '"' || c == '\'' || c == '`'):
			at++
			for at < len(src) && src[at] != c {
				if src[at] == '\\' {
					at += 2 // a backslash escapes one
				} else {
					at++
				}
			}
			at++
			start = true
		case start && isWord(c):
			from := at
			for at < len(src) && isWord(src[at]) {
				at++
			}
			start = keywords[src[from:at]]
		default:
			at++
			start = false
		}
	}
	return deepest
}
