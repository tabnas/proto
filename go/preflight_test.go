/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// preflight_test.go — the nesting cap, the Go half of
// rs/tests/untrusted_test.rs's nesting tests; ts/test/preflight.test.ts is
// the TypeScript one. Parse refuses a document nesting deeper than
// MaxNestingDepth before the engine builds a tree that deep, with the
// message the other two runtimes give, and the shared fixture
// test/spec/nesting.tsv holds all three to the same documents.

import (
	"errors"
	"fmt"
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
