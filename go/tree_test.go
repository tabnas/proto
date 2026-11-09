/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// tree_test.go — ParseTree held to the shared fixtures in test/tree/*.tsv
// (see ../test/AGENTS.md): each source and the tree's JSON text, member
// order included, which ts/test/tree.test.ts and rs/tests/tree_test.rs
// hold the other runtimes to.

import (
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"

	support "github.com/tabnas/support/go"
)

func TestTreeSpec(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	support.Runner{
		Parse: func(input string) (any, error) {
			tree, err := ParseTree(input)
			if err != nil {
				return nil, err
			}
			text, err := json.Marshal(tree)
			if err != nil {
				return nil, err
			}
			return string(text), nil
		},
		// The cell is the tree's JSON text, compared as text.
		ParseExpected: func(expected string, _ *support.Row) (any, error) {
			return expected, nil
		},
		// An ERROR:<want> cell holds a fragment of the message: the nesting
		// cap is the plugin's own refusal, which carries no code.
		MatchError: func(err error, want string, _ *support.Row) bool {
			return strings.Contains(err.Error(), want)
		},
	}.Dir(t, filepath.Join(dir, "..", "tree"))
}
