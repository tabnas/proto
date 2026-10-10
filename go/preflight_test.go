/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// preflight_test.go — the nesting cap, the Go half of
// rs/tests/untrusted_test.rs's nesting tests; ts/test/preflight.test.ts is
// the TypeScript one. Parse refuses a document nesting deeper than
// MaxNestingDepth before the engine builds a tree that deep, with the
// message the other two runtimes give, and the shared fixture
// test/spec/nesting.tsv holds all three to the same documents.

import (
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"strings"
	"testing"
	"time"

	tabnas "github.com/tabnas/parser/go"
)

// nested is `message M { message M { ... } }`, nested depth levels.
func nested(depth int) string {
	return "syntax = \"proto2\";\n" + strings.Repeat("message M {", depth) + strings.Repeat("}", depth)
}

func levels(fdp FileDescriptorProto) int {
	depth := 0
	for ms := fdp.MessageType; len(ms) > 0; ms = ms[0].NestedType {
		depth++
	}
	return depth
}

func TestNestingCapIsTheOneEveryRuntimeCarries(t *testing.T) {
	if MaxNestingDepth != 100 {
		t.Errorf("MaxNestingDepth is %d", MaxNestingDepth)
	}
}

// A cap nobody tests at is a number, not a bound.
func TestNestingAtAndUnderTheCapParsesWhole(t *testing.T) {
	for _, depth := range []int{MaxNestingDepth - 1, MaxNestingDepth} {
		fdp, err := Parse(nested(depth), nil)
		if err != nil {
			t.Fatalf("%d levels: %v", depth, err)
		}
		if got := levels(fdp); got != depth {
			t.Errorf("%d levels arrived as %d", depth, got)
		}
	}
}

func TestNestingPastTheCapIsRefusedByName(t *testing.T) {
	for _, depth := range []int{MaxNestingDepth + 1, 10 * MaxNestingDepth} {
		_, err := Parse(nested(depth), nil)
		want := fmt.Sprintf("proto: document nests %d levels deep, past the 100 this parser accepts", depth)
		if err == nil || err.Error() != want {
			t.Errorf("%d levels: got %v, want %q", depth, err, want)
		}
	}
}

// Like every refusal the plugin makes itself, it is not an engine error and
// carries no code.
func TestNestingRefusalCarriesNoCode(t *testing.T) {
	_, err := Parse(nested(MaxNestingDepth+1), nil)
	var engine *tabnas.TabnasError
	if err == nil || errors.As(err, &engine) {
		t.Errorf("got %#v, want a plain error", err)
	}
	// An engine refusal, for contrast, is one, with its code.
	if _, err := Parse("message M {", nil); !errors.As(err, &engine) || engine.Code != "unexpected" {
		t.Errorf("an engine refusal: got %#v", err)
	}
}

func TestDeepUnclosedNestingIsRefusedBeforeTheEngineRuns(t *testing.T) {
	start := time.Now()
	_, err := Parse("syntax = \"proto2\";\n"+strings.Repeat("message M {", 100000), nil)
	if err == nil || !strings.Contains(err.Error(), "nests 100000 levels deep") {
		t.Errorf("got %v", err)
	}
	// The engine never runs: unguarded, its tree for this many levels
	// grows past any memory a machine has.
	if elapsed := time.Since(start); elapsed > 5*time.Second {
		t.Errorf("took %s", elapsed)
	}
}

func TestBracesInStringsAndCommentsDoNotCount(t *testing.T) {
	noise := strings.Repeat("{", 4*MaxNestingDepth)
	for _, src := range []string{
		"syntax = \"proto2\";\noption a = \"" + noise + "\";\n",
		"syntax = \"proto2\";\noption a = '" + noise + "';\n",
		"syntax = \"proto2\";\noption a = \"\\\"" + noise + "\";\n",
		"syntax = \"proto2\";\n// " + noise + "\nmessage M {}\n",
		"syntax = \"proto2\";\n/* " + noise + " */\nmessage M {}\n",
		"syntax = \"proto2\";\n# " + noise + "\nmessage M {}\n",
	} {
		if err := Preflight(src); err != nil {
			t.Errorf("%.40q: %v", src, err)
		}
		if _, err := Parse(src, nil); err != nil {
			t.Errorf("%.40q: %v", src, err)
		}
	}
}

func TestEveryBraceCountsAnAggregatesIncluded(t *testing.T) {
	src := "syntax = \"proto2\";\noption (x) = {" + strings.Repeat("a {", 100) + strings.Repeat("}", 101) + ";"
	if err := Preflight(src); err == nil || !strings.Contains(err.Error(), "nests 101 levels deep") {
		t.Errorf("got %v", err)
	}
}

// The check on its own, for a caller that drives the engine: the engine's
// own Parse is not guarded.
func TestPreflightOnItsOwn(t *testing.T) {
	if err := Preflight(nested(MaxNestingDepth - 1)); err != nil {
		t.Error(err)
	}
	if err := Preflight(nested(MaxNestingDepth)); err != nil {
		t.Error(err)
	}
	if err := Preflight(nested(MaxNestingDepth + 1)); err == nil {
		t.Error("one past the cap passed")
	}

	rh := 8192
	j := tabnas.Make(tabnas.Options{Rewind: &tabnas.RewindOptions{History: &rh}})
	if err := Proto(j); err != nil {
		t.Fatal(err)
	}
	src := nested(MaxNestingDepth)
	if err := Preflight(src); err != nil {
		t.Fatal(err)
	}
	cst, err := j.Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	fdp, err := ToDescriptor(cst, nil)
	if err != nil {
		t.Fatal(err)
	}
	if got := levels(fdp); got != MaxNestingDepth {
		t.Errorf("arrived as %d levels", got)
	}

	// And the walk has no bound of its own: a tree past the cap, from a
	// caller who skipped the check, is walked whole. The Rust walk refuses
	// one, which DIVERGENCE.md section 2 records.
	cst, err = j.Parse(nested(MaxNestingDepth + 1))
	if err != nil {
		t.Fatal(err)
	}
	if fdp, err = ToDescriptor(cst, nil); err != nil || levels(fdp) != MaxNestingDepth+1 {
		t.Errorf("a tree past the cap: %d levels, %v", levels(fdp), err)
	}
}

// past is a document nesting one level past the cap after head.
func past(head string) string {
	return head + strings.Repeat("message M {", MaxNestingDepth+1) + strings.Repeat("}", MaxNestingDepth+1)
}

func refusedAt(t *testing.T, src string, depth int) {
	t.Helper()
	want := fmt.Sprintf("nests %d levels deep", depth)
	if err := Preflight(src); err == nil || !strings.Contains(err.Error(), want) {
		t.Errorf("%.60q: got %v, want %q", src, err, want)
	}
	if _, err := Parse(src, nil); err == nil || !strings.Contains(err.Error(), want) {
		t.Errorf("%.60q: Parse got %v, want %q", src, err, want)
	}
}

// The lexer ends a line at a carriage return as well as a line feed, and a
// line comment with it, so the braces after one count: a document that
// breaks its lines with carriage returns alone cannot hide its nesting in
// a comment.
func TestALineCommentEndsAtACarriageReturn(t *testing.T) {
	for _, comment := range []string{"// c", "# c"} {
		refusedAt(t, past("syntax = \"proto2\";\r"+comment+"\r"), MaxNestingDepth+1)
	}
}

// The lexer reads a backtick string as a string, across lines too, and a
// backslash escapes a backtick in it, so its braces nest nothing.
func TestBracesInABacktickStringDoNotCount(t *testing.T) {
	noise := strings.Repeat("{", 4*MaxNestingDepth)
	for _, src := range []string{
		"syntax = \"proto2\";\noption a = `" + noise + "`;\n",
		"syntax = \"proto2\";\noption a = `\\`" + noise + "`;\n",
		"syntax = \"proto2\";\noption a = `" + noise + "\n" + noise + "`;\n",
	} {
		if err := Preflight(src); err != nil {
			t.Errorf("%.40q: %v", src, err)
		}
		if _, err := Parse(src, nil); err != nil {
			t.Errorf("%.40q: %v", src, err)
		}
	}
}

// A quote opens a string only where the lexer starts a token. Inside a
// word the lexer reads it as part of the word (`message a"b` names a
// message a"b), so the braces after it count; straight after a keyword it
// opens a string, whose braces close nothing.
func TestAQuoteOpensAStringOnlyWhereATokenStarts(t *testing.T) {
	for _, quote := range []string{`"`, `'`, "`"} {
		refusedAt(t, past("syntax = \"proto2\";\nmessage a"+quote+"b {")+"}", MaxNestingDepth+2)
	}
	closers := "syntax = \"proto2\";\n" + strings.Repeat("message M {", 90) +
		"reserved\"" + strings.Repeat("}", 90) + "\";" +
		strings.Repeat("message M {", 11) + strings.Repeat("}", 101)
	refusedAt(t, closers, 101)

	noise := strings.Repeat("{", 4*MaxNestingDepth)
	fdp, err := Parse("syntax = \"proto2\";\nmessage M { reserved\""+noise+"\"; }\n", nil)
	if err != nil {
		t.Fatal(err)
	}
	if names := fdp.MessageType[0].ReservedName; len(names) != 1 || names[0] != noise {
		t.Errorf("reserved names: %.40q", names)
	}
}

// The scan's keywords and separators are the grammar's: after each keyword
// the grammar's match tokens name, and after each fixed token, a quote
// opens a string, whose braces close nothing. A keyword the grammar gains
// and the scan does not know fails here.
func TestTheScanKnowsTheGrammarsTokens(t *testing.T) {
	var spec struct {
		Options struct {
			Fixed struct {
				Token map[string]string `json:"token"`
			} `json:"fixed"`
			Match struct {
				Token map[string]string `json:"token"`
			} `json:"match"`
		} `json:"options"`
	}
	if err := json.Unmarshal(grammarSpec, &spec); err != nil {
		t.Fatal(err)
	}
	// Sixty levels open, then a string of sixty closers after the token,
	// then sixty levels more: 120 levels when the string is one, 60 when
	// its closers count.
	probe := func(lead string) string {
		return strings.Repeat("{", 60) + lead + "\"" + strings.Repeat("}", 60) + "\"" + strings.Repeat("{", 60)
	}
	shape := regexp.MustCompile(`^@~/\^([A-Za-z]+)\\b/$`)
	if len(spec.Options.Match.Token) == 0 {
		t.Fatal("the grammar names no match tokens")
	}
	for name, re := range spec.Options.Match.Token {
		m := shape.FindStringSubmatch(re)
		if m == nil {
			t.Errorf("match token %s is not a keyword the scan can read: %s", name, re)
			continue
		}
		if got := braceDepth(probe(" " + m[1])); got != 120 {
			t.Errorf("after the keyword %s the scan counts %d levels, not 120", m[1], got)
		}
		if got := braceDepth(probe(" " + m[1] + "x")); got != 60 {
			t.Errorf("after the word %sx the scan counts %d levels, not 60", m[1], got)
		}
	}
	for name, token := range spec.Options.Fixed.Token {
		if token == "{" || token == "}" {
			continue
		}
		if got := braceDepth(probe("a" + token)); got != 120 {
			t.Errorf("after the fixed token %s (%q) the scan counts %d levels, not 120", name, token, got)
		}
	}
}
