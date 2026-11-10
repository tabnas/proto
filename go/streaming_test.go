// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasproto

// The parser a host streams a .proto document through, MakeStreaming,
// against the parser for a tree, Make. Its grammar is the compiled one
// without the CST's builders, so it builds no tree, and its rule passes,
// which the transducer part (alchemy/transduce.alc) reads, must be Make's:
// over every document the window test reads, every pass alike, its rule,
// state, depth, alternate, counters and tokens, and whether the rule's node
// is a value a pass carries, and every parse ending alike, an error at the
// same code, place and token. ts/test/streaming.test.ts and
// rs/tests/streaming_test.rs hold the TypeScript and Rust ports to the same.

import (
	"errors"
	"fmt"
	"sort"
	"strings"
	"testing"
	"time"

	tabnas "github.com/tabnas/parser/go"
)

// carried says whether a pass carries the rule's node as a value, as
// transduce's parser-event source asks it: a close pass does, where the
// node is not the engine's Undefined and not a container, a scalar or nil,
// which is a null.
func carried(close bool, node any) bool {
	if !close || tabnas.IsUndefined(node) {
		return false
	}
	switch node.(type) {
	case map[string]any, []any, *tabnas.OrderedMap:
		return false
	}
	return true
}

// passLogger is a parser build makes, whose passes parse logs: every pass
// of a parse of text, and how the parse ended.
func passLogger(t *testing.T, build func() (*tabnas.Tabnas, error)) func(text string) []string {
	t.Helper()
	j, err := build()
	if err != nil {
		t.Fatal(err)
	}
	var out []string
	j.SubRuleDone(func(r *tabnas.Rule, _ *tabnas.Context, done tabnas.RuleDone) {
		tokens := r.O[:r.ON]
		if done.State == tabnas.CLOSE {
			tokens = r.C[:r.CN]
		}
		srcs := make([]string, len(tokens))
		for i, tkn := range tokens {
			srcs[i] = fmt.Sprintf("%s %q %d", tkn.Name, tkn.Src, tkn.SI)
		}
		alt := "-"
		if done.Alt != nil {
			alt = fmt.Sprintf("%s/%s/%d/%s", done.Alt.P, done.Alt.R, done.Alt.B, strings.Join(done.Alt.G, ","))
		}
		names := make([]string, 0, len(r.N))
		for name := range r.N {
			names = append(names, fmt.Sprintf("%s=%d", name, r.N[name]))
		}
		sort.Strings(names)
		out = append(out, fmt.Sprintf("%s | %s | %d | %d | %s | %v | %v | %s | %v | %s",
			r.Name, done.State, r.D, r.I, alt, done.Forced, done.Probe, strings.Join(names, ","),
			carried(done.State == tabnas.CLOSE, r.Node), strings.Join(srcs, ", ")))
	})
	return func(text string) []string {
		out = nil
		if _, err := j.Parse(text); err != nil {
			var te *tabnas.TabnasError
			if !errors.As(err, &te) {
				t.Fatalf("not an engine error: %v", err)
			}
			out = append(out, fmt.Sprintf("err %s at %d %d:%d %q", te.Code, te.Pos, te.Row, te.Col, te.Src))
		} else {
			out = append(out, "ok")
		}
		return out
	}
}

func TestMakeStreamingBuildsNoTree(t *testing.T) {
	text := "syntax = \"proto3\";\nmessage M { int32 a = 1 [(o) = { x: 1 }]; }\n"
	stream, err := MakeStreaming()
	if err != nil {
		t.Fatal(err)
	}
	if value, err := stream.Parse(text); err != nil || !(value == nil || tabnas.IsUndefined(value)) {
		t.Fatalf("MakeStreaming's value: %#v, %v", value, err)
	}
	tree, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	value, err := tree.Parse(text)
	if err != nil {
		t.Fatal(err)
	}
	if nrule(value.(map[string]any)) != "proto" {
		t.Fatalf("Make's value: %#v", value)
	}
}

func TestMakeStreamingsPassesAreMakesOverEveryDocument(t *testing.T) {
	documents := windowDocuments(t)
	documents = append(documents, windowDocument{at: "the long document", text: longDocument()})
	for index, text := range longErrorDocuments() {
		documents = append(documents, windowDocument{at: fmt.Sprintf("long error document %d", index), text: text})
	}
	if len(documents) < 500 {
		t.Fatalf("%d documents", len(documents))
	}
	tree, stream := passLogger(t, Make), passLogger(t, MakeStreaming)
	compared, failed := 0, 0
	reported := time.Now()
	for index, document := range documents {
		want := append([]string(nil), tree(document.text)...)
		got := stream(document.text)
		if len(got) != len(want) {
			t.Fatalf("%s: %d passes, where Make makes %d", document.at, len(got), len(want))
		}
		for i := range want {
			if got[i] != want[i] {
				t.Fatalf("%s: pass %d\nstreaming %s\nmake      %s", document.at, i, got[i], want[i])
			}
		}
		compared += len(want)
		if strings.HasPrefix(want[len(want)-1], "err ") {
			failed++
		}
		if time.Since(reported) > 20*time.Second {
			reported = time.Now()
			t.Logf("streaming passes: %d of %d documents (%d%%)", index+1, len(documents), 100*(index+1)/len(documents))
		}
	}
	if failed < 50 {
		t.Fatalf("%d documents fail", failed)
	}
	t.Logf("%d documents, %d failing, %d passes alike", len(documents), failed, compared)
}
