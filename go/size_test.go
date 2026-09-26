/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// The size of the compiled grammar is a contract (tabnas/bnf#71,
// docs/design/alt-explosion.md section 9.5): admitting every keyword as
// an identifier used to multiply the dispatch tables into the millions.
// Per-decision lookahead and the `ident` token class hold it to a few
// hundred alternates. TypeScript and Rust pin the same bounds.

import (
	"testing"

	tabnas "github.com/tabnas/parser/go"
)

func TestSizeIsAFewHundredAlternates(t *testing.T) {
	j := tabnas.Make()
	if err := Proto(j); err != nil {
		t.Fatal(err)
	}
	rules := j.Rules()
	total := 0
	biggest, biggestN := "", 0
	for _, rs := range rules {
		n := len(rs.OpenAlts())
		total += n
		if n > biggestN {
			biggest, biggestN = rs.Name, n
		}
	}
	if len(rules) > 500 {
		t.Errorf("%d rules", len(rules))
	}
	if total > 1000 {
		t.Errorf("%d open alternates", total)
	}
	if biggestN > 60 {
		t.Errorf("%s has %d open alternates", biggest, biggestN)
	}
	if j.TokenSet("ident") == nil {
		t.Error("the identifier class is not a token set")
	}
}
