/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Go port of the "aggregate option values" suite in ts/test/proto.test.ts.
// test/spec/aggregate.tsv holds the rows; this pins the API paths.

package tabnasproto

import (
	"strings"
	"testing"

	tabnas "github.com/tabnas/parser/go"
)

// protoc records the text between the braces, comments turned to the
// spaces and newlines that keep each token where it was (protoc 36.2's
// parser gives exactly this string for this source).
const (
	aggregateSample     = "message M {\n  option (f) = {\n    a: 1 // one\n    b { c: \"x\" /* two */ }\n  };\n}\n"
	aggregateSampleText = "\n    a: 1 \n    b { c: \"x\"           }\n  "
)

func TestAggregateRecordsTheTextBetweenTheBraces(t *testing.T) {
	fdp := mustParse(t, aggregateSample, nil)
	if got := fdp.MessageType[0].Options["(f)"]; got != aggregateSampleText {
		t.Errorf("aggregate: got %q, want %q", got, aggregateSampleText)
	}
}

// The walk has only the CST, so the text has to be on the CST: Proto puts
// it on the aggregate's `constant` node as "aggregate", and leaves "src"
// the tokens run together, as on every node. The C library takes this
// path: an engine built once, then ToDescriptor(j.Parse(src)).
func TestAggregateReadsTheSameThroughToDescriptor(t *testing.T) {
	cst, err := aggregateEngine(t).Parse(aggregateSample)
	if err != nil {
		t.Fatal(err)
	}
	fdp, err := ToDescriptor(cst, nil)
	if err != nil {
		t.Fatal(err)
	}
	if got := fdp.MessageType[0].Options["(f)"]; got != aggregateSampleText {
		t.Errorf("ToDescriptor: got %q, want %q", got, aggregateSampleText)
	}
	var find func(n any) map[string]any
	find = func(n any) map[string]any {
		m, _ := n.(map[string]any)
		if m == nil {
			return nil
		}
		if nrule(m) == "constant" {
			return m
		}
		for _, k := range nkids(m) {
			if found := find(k); found != nil {
				return found
			}
		}
		return nil
	}
	node := find(cst)
	if node == nil {
		t.Fatal("no constant node in the CST")
	}
	if got := nsrc(node); got != `{a:1b{c:"x"}}` {
		t.Errorf("src: got %q", got)
	}
	if got := node["aggregate"]; got != aggregateSampleText {
		t.Errorf("aggregate on the node: got %q, want %q", got, aggregateSampleText)
	}

	// A value inside the braces is a `constant`, a single string included.
	if got := entryKids(cst, `c:"x"`); strings.Join(got, " ") != "constant" {
		t.Errorf(`entry c:"x": got rules %q, want [constant]`, got)
	}
}

func TestAggregateTakesAdjacentStringLiterals(t *testing.T) {
	const src = `message M { option (f) = { a: "x" "y" }; }`
	fdp := mustParse(t, src, nil)
	if got := fdp.MessageType[0].Options["(f)"]; got != ` a: "x" "y" ` {
		t.Errorf("adjacent strings: got %q", got)
	}

	// Two or more literals are the entry's `strLit` children, one each.
	cst, err := aggregateEngine(t).Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	if got := entryKids(cst, `a:"x""y"`); strings.Join(got, " ") != "strLit strLit" {
		t.Errorf(`entry a:"x""y": got rules %q, want [strLit strLit]`, got)
	}
}

// aggregateEngine is an engine built once with Proto installed, the way
// the C library builds one.
func aggregateEngine(t *testing.T) *tabnas.Tabnas {
	t.Helper()
	rh := 8192
	j := tabnas.Make(tabnas.Options{Rewind: &tabnas.RewindOptions{History: &rh}})
	if err := Proto(j); err != nil {
		t.Fatal(err)
	}
	return j
}

// entryKids is the rules under the entry whose src is entry: the CST shape
// of one value inside the braces. Nil when the tree has no such entry.
func entryKids(n any, entry string) []string {
	m, _ := n.(map[string]any)
	if m == nil {
		return nil
	}
	if nrule(m) == "messageValueEntry" && nsrc(m) == entry {
		rules := []string{}
		for _, k := range nkids(m) {
			rules = append(rules, nrule(k))
		}
		return rules
	}
	for _, k := range nkids(m) {
		if found := entryKids(k, entry); found != nil {
			return found
		}
	}
	return nil
}
