/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// passes_test.go — the rule passes the transducer part
// (alchemy/transduce.alc) reads a statement by, held to the shared
// fixtures in test/passes/*.tsv, as ts/test/passes.test.ts holds the
// TypeScript port and rs/tests/passes_test.rs the Rust one; and the
// ten-thousand items test: repetition is replacement, so the items of a
// list add no rule depth however many there are.

import (
	"encoding/json"
	"fmt"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	tabnas "github.com/tabnas/parser/go"
	support "github.com/tabnas/support/go"
)

var statementRule = regexp.MustCompile(
	`^((syntaxOrEdition|topLevelDef|messageElement|enumElement|oneofElement|serviceElement)\$alt\d+|field|optionStmt|fieldOption|messageValueEntry)$`)

// statements is the names of the statements' rules, in the order their open
// passes come, and constant$alt5 where a constant opens an aggregate.
func statements(src string) ([]string, error) {
	j, err := Make()
	if err != nil {
		return nil, err
	}
	names := []string{}
	j.SubRuleDone(func(rule *tabnas.Rule, _ *tabnas.Context, done tabnas.RuleDone) {
		if done.Probe || done.State != tabnas.OPEN {
			return
		}
		if statementRule.MatchString(rule.Name) {
			names = append(names, rule.Name)
		}
		if rule.Name == "constant" && done.Alt != nil && done.Alt.P == "constant$alt5" {
			names = append(names, "constant$alt5")
		}
	})
	if _, err := j.Parse(src); err != nil {
		return nil, err
	}
	return names, nil
}

func TestPassesSpec(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	support.Runner{
		Parse: func(input string) (any, error) {
			names, err := statements(input)
			if err != nil {
				return nil, err
			}
			text, err := json.Marshal(names)
			return string(text), err
		},
		ParseExpected: func(expected string, _ *support.Row) (any, error) {
			return expected, nil
		},
	}.Dir(t, filepath.Join(dir, "..", "passes"))
}

// depth is the deepest rule a parse of src reaches.
func depth(t *testing.T, src string) int {
	t.Helper()
	j, err := Make()
	if err != nil {
		t.Fatal(err)
	}
	deepest := 0
	j.SubRuleDone(func(rule *tabnas.Rule, _ *tabnas.Context, done tabnas.RuleDone) {
		if !done.Probe && rule.D > deepest {
			deepest = rule.D
		}
	})
	if _, err := j.Parse(src); err != nil {
		t.Fatal(err)
	}
	return deepest
}

func TestRepetitionAddsNoDepth(t *testing.T) {
	const n = 10000
	items := func(n int, item func(i int) string, sep string) string {
		parts := make([]string, n)
		for i := range parts {
			parts[i] = item(i)
		}
		return strings.Join(parts, sep)
	}
	repeated := []struct {
		what string
		doc  func(n int) string
	}{
		{"declarations of a file", func(n int) string { return strings.Repeat("message M {}\n", n) }},
		{"fields of a message", func(n int) string {
			return "message M {\n" + items(n, func(i int) string { return fmt.Sprintf("  int32 f%d = %d;\n", i, i+1) }, "") + "}\n"
		}},
		{"values of an enum", func(n int) string {
			return "enum E {\n" + items(n, func(i int) string { return fmt.Sprintf("  V%d = %d;\n", i, i) }, "") + "}\n"
		}},
		{"fields of an aggregate", func(n int) string { return "option (a) = {" + strings.Repeat(" b: 1", n) + " };\n" }},
		{"options in brackets", func(n int) string {
			return "message M { int32 f = 1 [" + items(n, func(i int) string { return fmt.Sprintf("(o%d) = 1", i) }, ", ") + "]; }\n"
		}},
		{"ranges of a reserved statement", func(n int) string {
			return "message M { reserved " + items(n, func(i int) string { return fmt.Sprint(i + 1) }, ", ") + "; }\n"
		}},
	}
	// Two items, since a list's first item can sit apart from its repeated
	// ones (x *( "," x )): the loop's own item is the second.
	for _, r := range repeated {
		if got, want := depth(t, r.doc(n)), depth(t, r.doc(2)); got != want {
			t.Errorf("the %s: %d reach rule depth %d, where two reach %d", r.what, n, got, want)
		}
	}
}
