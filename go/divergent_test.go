// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasproto

// divergent_test.go — the divergence register, executed in the Go port.
//
// ../test/divergent.tsv holds one row per input where this repo's three
// ports DISAGREE, with a cell per runtime. This file reads the "go"
// column through github.com/tabnas/support/go's Register, the same
// mechanism ts/test/divergent.test.ts and rs/tests/divergent_test.rs read
// the "ts" and "rust" columns with.
//
// WHY THIS IS NOT A FIXTURE. A fixture fails when behaviour REGRESSES.
// The register fails both ways: when a port is repaired to agree with the
// others the row still claims they differ, so the suite goes red and
// names the row to delete. That is what keeps a divergence from outliving
// its own repair, and why the file sits beside test/spec/ rather than in
// it, where TestSpec would run it.
//
// Four of these rows record a defect in THIS port. The register is where
// they are pinned until the repair lands; see ../DIVERGENCE.md for the
// prose and ../test/divergent.tsv for the rows.

import (
	"path/filepath"
	"strings"
	"testing"

	support "github.com/tabnas/support/go"
)

func TestDivergenceRegister(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}

	support.Register{
		Runner: support.Runner{
			ParseRow: func(input string, row *support.Row) (any, error) {
				opts, err := specOpts(row)
				if err != nil {
					return nil, err
				}
				return Parse(input, opts)
			},

			// As in parity_test.go: an ERROR:<want> cell holds a fragment
			// of the message rather than a code, because this package
			// declares none.
			MatchError: func(err error, want string, _ *support.Row) bool {
				return strings.Contains(err.Error(), want)
			},

			Normalize: jsonFlatten,
		},
		Runtime:  "go",
		Runtimes: []string{"ts", "go", "rust"},
	}.File(t, filepath.Join(dir, "..", "divergent.tsv"))
}
