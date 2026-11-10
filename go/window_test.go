// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasproto

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole. Make declares Lex.Window: the nesting cap is counted as
// the lexer gives the parser its tokens, and an aggregate's text, which a
// parse of a whole text reads from the source, is kept from the tokens
// between its braces (read.go). The keywords, match tokens, need nothing of
// their own: the engine cuts a word that met the window's end again over a
// wider window (tabnas/parser 0.12.17).
//
// Every row of the shared fixtures (test/spec, test/passes, test/tree),
// every input of the protoc corpus (test/protobuf-suite) and
// test/descriptor/descriptor.proto is parsed whole and from a reader that
// hands its bytes over in pieces of 1, 3 and 64 bytes. The engine's window
// sizes are fixed in Go (only its own tests can make them a few bytes), and
// the window reaches 64 KiB past the cursor before each token is cut, so
// the edge is proved with long documents: their tokens of every kind
// straddle the reads, and the long ones, longer than the window, run past
// its end, in pieces of 61, 4096 and 65536 bytes. The value (the CST, an
// aggregate's text included), the tokens the lexer gives, the rule passes,
// and an error's code, position, row, column and token must be the same. An
// error's excerpt is not compared: a parse from a reader shows a long line
// only as far back as the window reaches (tabnas/parser's DIVERGENCE.md).
//
// ts/test/window.test.ts and rs/tests/window_test.rs hold the TypeScript
// and Rust ports to the same, under windows of a few units too.

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	support "github.com/tabnas/support/go"

	tabnas "github.com/tabnas/parser/go"
)

// pieces is a reader that hands its bytes over at most size at a time.
type pieces struct {
	bytes []byte
	at    int
	size  int
}

func (p *pieces) Read(buf []byte) (int, error) {
	if p.at >= len(p.bytes) {
		return 0, io.EOF
	}
	n := min(p.size, len(buf), len(p.bytes)-p.at)
	copy(buf, p.bytes[p.at:p.at+n])
	p.at += n
	return n, nil
}

// readSizes are the pieces a reader hands a fixture or corpus document
// over in, bigSizes a document's longer than 16 KiB, and longSizes a long
// document's: the Go engine copies its window at each read
// (tabnas/parser#321), so a read of a few bytes costs the window's length,
// and a long document read a byte at a time takes seconds where its parse
// takes milliseconds.
var (
	readSizes = []int{1, 3, 64}
	bigSizes  = []int{64, 4096}
	longSizes = []int{61, 4096, 65536}
)

// windowLog gathers the tokens the lexer gives and the rule passes.
type windowLog struct {
	tokens []string
	rules  []string
}

// logged is Make's parser, its tokens and rule passes subscribed to log.
func logged(t *testing.T, log *windowLog) *tabnas.Tabnas {
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	j.Sub(func(tkn *tabnas.Token, _ *tabnas.Rule, _ *tabnas.Context) {
		val, _ := json.Marshal(tkn.Val)
		log.tokens = append(log.tokens, fmt.Sprintf("%s %q %s %d %d:%d",
			tkn.Name, tkn.Src, val, tkn.SI, tkn.RI, tkn.CI))
	}, nil)
	j.SubRuleDone(func(r *tabnas.Rule, _ *tabnas.Context, done tabnas.RuleDone) {
		log.rules = append(log.rules, fmt.Sprintf("%s %v %d", r.Name, done.State, r.D))
	})
	return j
}

// outcome is a parse's value as JSON, or its error's code, position, row,
// column and token.
func outcome(value any, err error) string {
	if err != nil {
		var te *tabnas.TabnasError
		if errors.As(err, &te) {
			return fmt.Sprintf("err %s at %d %d:%d %q", te.Code, te.Pos, te.Row, te.Col, te.Src)
		}
		return "fail " + err.Error()
	}
	text, err := json.Marshal(value)
	if err != nil {
		return "unmarshallable " + err.Error()
	}
	return "ok " + string(text)
}

// differential parses text whole and from a reader in each of sizes, and
// fails t where the two differ. It returns the count of reader parses
// compared.
func differential(t *testing.T, j *tabnas.Tabnas, log *windowLog, at, text string, sizes []int) int {
	t.Helper()
	log.tokens, log.rules = nil, nil
	value, err := j.Parse(text)
	whole, tokens, rules := outcome(value, err), log.tokens, log.rules
	for _, size := range sizes {
		log.tokens, log.rules = nil, nil
		value, err := j.ParseReader(&pieces{bytes: []byte(text), size: size})
		if read := outcome(value, err); read != whole {
			t.Fatalf("%s in pieces of %d:\nread  %.300s\nwhole %.300s", at, size, read, whole)
		}
		if !reflect.DeepEqual(log.tokens, tokens) {
			t.Fatalf("%s in pieces of %d: the tokens differ\nread  %.400v\nwhole %.400v", at, size, log.tokens, tokens)
		}
		if !reflect.DeepEqual(log.rules, rules) {
			t.Fatalf("%s in pieces of %d: the rule passes differ", at, size)
		}
	}
	return len(sizes)
}

// windowDocument is a document of the fixtures or the corpus, and where it
// is from.
type windowDocument struct{ at, text string }

// windowDocuments are the input of every row of the shared fixtures, every
// input of the protoc corpus and its leniency probes, and descriptor.proto.
func windowDocuments(t *testing.T) []windowDocument {
	var out []windowDocument
	spec, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	for _, dir := range []string{spec, filepath.Join(spec, "..", "passes"), filepath.Join(spec, "..", "tree")} {
		files, _ := filepath.Glob(filepath.Join(dir, "*.tsv"))
		sort.Strings(files)
		for _, file := range files {
			raw, err := os.ReadFile(file)
			if err != nil {
				t.Fatal(err)
			}
			for index, line := range strings.Split(string(raw), "\n") {
				if index == 0 || line == "" || (strings.HasPrefix(line, "#") && !strings.Contains(line, "\t")) {
					continue
				}
				out = append(out, windowDocument{
					at:   fmt.Sprintf("%s:%d", filepath.Base(file), index+1),
					text: support.Unescape(strings.SplitN(line, "\t", 2)[0]),
				})
			}
		}
	}
	for _, file := range []string{"valid", "invalid", "accept-only"} {
		for index, c := range loadCorpus(t, file+".json") {
			out = append(out, windowDocument{at: fmt.Sprintf("%s:%d", file, index), text: c.Input})
		}
	}
	raw, err := os.ReadFile(filepath.Join(suiteDir(), "leniency.json"))
	if err != nil {
		t.Fatal(err)
	}
	var leniency struct {
		Probes []struct {
			Input string `json:"input"`
		} `json:"probes"`
	}
	if err := json.Unmarshal(raw, &leniency); err != nil {
		t.Fatal(err)
	}
	for index, probe := range leniency.Probes {
		out = append(out, windowDocument{at: fmt.Sprintf("leniency:%d", index), text: probe.Input})
	}
	raw, err = os.ReadFile(filepath.Join("..", "test", "descriptor", "descriptor.proto"))
	if err != nil {
		t.Fatal(err)
	}
	return append(out, windowDocument{at: "descriptor.proto", text: string(raw)})
}

// longDocument is a .proto file of every kind of token, the long ones
// longer than the engine's 64 KiB window, with characters past ASCII in its
// comments and strings and aggregate option values of every shape, so that
// tokens of every kind straddle the window's edges as a reader hands the
// text over. The same document as long_document in rs/tests/window_test.rs
// and longDocument in ts/test/window.test.ts.
func longDocument() string {
	var b strings.Builder
	b.WriteString("// " + strings.Repeat("a comment past the window, \u00e9 \U0001D11E; ", 2000) + "\r\n")
	b.WriteString("/* " + strings.Repeat("a block comment past the window\r\n\t", 2000) + "*/\r\n")
	b.WriteString("syntax = \"proto2\";\r\npackage a.b.c;\r\n")
	b.WriteString("option long = \"" + strings.Repeat("a string past the window \u00fc ", 3000) + "\";\r\n")
	b.WriteString("option tick = '" + strings.Repeat("single quotes past the window ", 3000) + "';\r\n")
	b.WriteString("option (named) = " + strings.Repeat("n", 70000) + ";\r\n")
	b.WriteString("option (big) = {\r\n" + strings.Repeat("  a: 1 /* c\t*/ b { c: \"d\" } # e\r\n\t", 2000) + "};\r\n")
	b.WriteString("message " + strings.Repeat("N", 70000) + " { optional int32 f = 1; }\r\n")
	for i := 0; i < 250; i++ {
		fmt.Fprintf(&b, "message M%d { // r\u00e9gle %d\r\n", i, i)
		fmt.Fprintf(&b, "  optional int32 a%d = %d [default = -0x%X, (o) = { x: %d y: \"z\" }];\r\n", i, i+1, i, i)
		fmt.Fprintf(&b, "  repeated string optionals = %d;\r\n  required group G%d = %d { optional bool b = 1; }\r\n", i+2, i, i+3)
		fmt.Fprintf(&b, "  enum E%d { option allow_alias = true; V%d = 0; W%d = 1 [deprecated = true]; }\r\n", i, i, i)
		fmt.Fprintf(&b, "  oneof o%d { int64 one = %d; }\r\n  map<string, int32> m%d = %d;\r\n", i, i+4, i, i+5)
		fmt.Fprintf(&b, "  extensions 100 to max;\r\n  reserved 50 to 60;\r\n  reserved \"res%d\";\r\n}\r\n", i)
		fmt.Fprintf(&b, "service S%d { rpc R%d(stream M%d) returns (M%d) { option (x) = {a: 1}; } }\r\n", i, i, i, i)
	}
	b.WriteString("extend M0 { optional int32 ext = 1000; }\r\n")
	return b.String()
}

func TestMakeReadsAReaderAWindowAtATime(t *testing.T) {
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	lex := j.Options().Lex
	if lex == nil || lex.Window == nil || !*lex.Window {
		t.Fatal("Make does not declare Lex.Window")
	}
}

func TestEveryFixtureAndCorpusDocumentParsesTheSameFromAReader(t *testing.T) {
	documents := windowDocuments(t)
	if len(documents) < 500 {
		t.Fatalf("%d documents", len(documents))
	}
	log := &windowLog{}
	j := logged(t, log)
	compared, aggregates := 0, 0
	for _, document := range documents {
		sizes := readSizes
		if len(document.text) > 16*1024 {
			sizes = bigSizes
		}
		compared += differential(t, j, log, document.at, document.text, sizes)
		if value, err := j.Parse(document.text); err == nil {
			if text, _ := json.Marshal(value); strings.Contains(string(text), `"aggregate"`) {
				aggregates++
			}
		}
	}
	if aggregates < 50 {
		t.Fatalf("%d documents hold an aggregate", aggregates)
	}
	t.Logf("%d documents, %d with an aggregate, %d reader parses", len(documents), aggregates, compared)
}

func TestALongDocumentParsesTheSameFromAReader(t *testing.T) {
	text := longDocument()
	if len(text) < 512*1024 {
		t.Fatalf("%d bytes", len(text))
	}
	tree, err := ParseTree(text)
	if err != nil {
		t.Fatal(err)
	}
	if body, _ := tree.Get("body"); len(body.([]any)) != 508 {
		t.Fatalf("%d declarations", len(body.([]any)))
	}
	log := &windowLog{}
	differential(t, logged(t, log), log, "the long document", text, longSizes)
}

func TestALongDocumentFailsTheSameFromAReader(t *testing.T) {
	// A token that runs past the engine's window and is then refused, or
	// never ends, and nesting past the cap after a long run: the error is
	// the whole text's, code, place and token.
	pad := "// " + strings.Repeat("padding \u00fc ", 9000) + "\r\nsyntax = \"proto3\";\r\n"
	bad := []string{
		pad + "option a = \"" + strings.Repeat("unclosed string ", 6000) + "\r\n",
		pad + "/* " + strings.Repeat("an unclosed comment ", 4000) + "\r\n",
		pad + "message M { " + strings.Repeat("int32 a = 1; ", 6000) + "\r\n",
		pad + "option b = {" + strings.Repeat(" c: 1", 15000) + " ) };\r\n",
		pad + "message " + strings.Repeat("m", 70000) + " } \r\n",
		pad + "option (x) = " + strings.Repeat("{a ", MaxNestingDepth+1) + strings.Repeat("}", MaxNestingDepth+1) + ";\r\n",
		pad + strings.Repeat("message M{", MaxNestingDepth+1) + strings.Repeat("}", MaxNestingDepth+1),
		pad + strings.Repeat("message M {", 100000),
	}
	log := &windowLog{}
	j := logged(t, log)
	for index, text := range bad {
		if _, err := j.Parse(text); err == nil {
			t.Fatalf("long error document %d parses", index)
		}
		differential(t, j, log, fmt.Sprintf("long error document %d", index), text, longSizes)
	}
}

func TestTheParserRefusesNestingPastTheCapAsItReads(t *testing.T) {
	// The brace that opens the level past the cap is refused, with the
	// code proto_too_deep at that brace, whole and from a reader, and the
	// cap itself parses.
	nest := func(depth int, open string) string {
		return "syntax = \"proto2\";\n" + strings.Repeat(open, depth) + strings.Repeat("}", depth)
	}
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := j.Parse(nest(MaxNestingDepth, "message M {")); err != nil {
		t.Fatal(err)
	}
	for _, c := range []struct {
		text string
		pos  int
	}{
		{nest(MaxNestingDepth+1, "message M {"), 19 + 11*MaxNestingDepth + 10},
		{nest(MaxNestingDepth+1, "message M{"), 19 + 10*MaxNestingDepth + 9},
		{"option (x) = " + strings.Repeat("{a ", MaxNestingDepth+1), 13 + 3*MaxNestingDepth},
		{strings.Repeat("message M {", 100000), 11*MaxNestingDepth + 10},
	} {
		for _, read := range []bool{false, true} {
			var err error
			if read {
				_, err = j.ParseReader(&pieces{bytes: []byte(c.text), size: 64})
			} else {
				_, err = j.Parse(c.text)
			}
			var te *tabnas.TabnasError
			if !errors.As(err, &te) || te.Code != tooDeep || te.Pos != c.pos || te.Src != "{" {
				t.Fatalf("%.40q (from a reader %v): %v", c.text, read, err)
			}
			if !strings.Contains(te.Error(), tooDeepMessage) {
				t.Fatalf("%.40q: %s", c.text, te.Error())
			}
		}
	}
}

func TestALongDocumentParsesFromAReaderInTimeLinearInItsLength(t *testing.T) {
	// Ten times the messages, read through the engine's window in pieces
	// of 4 KiB, take about ten times as long, never the square of it.
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	document := func(messages int) string {
		var b strings.Builder
		b.WriteString("syntax = \"proto3\";\n")
		for i := 0; i < messages; i++ {
			fmt.Fprintf(&b, "message M%d { string a = 1; int32 b = 2 [(o) = {x: %d}]; } // c\n", i, i)
		}
		return b.String()
	}
	timed := func(text string) time.Duration {
		started := time.Now()
		if _, err := j.ParseReader(&pieces{bytes: []byte(text), size: 4096}); err != nil {
			t.Fatal(err)
		}
		return time.Since(started)
	}
	small, large := document(300), document(3000)
	timed(small)
	one, ten := timed(small), timed(large)
	t.Logf("300 messages %v, 3000 messages %v, %.1f times", one, ten, float64(ten)/float64(one))
	if ten > 30*one {
		t.Fatalf("300 messages %v, 3000 messages %v", one, ten)
	}
}
