// Copyright (c) 2025 Richard Rodger and other contributors, MIT License

package tabnasproto

// parity_test.go — cross-runtime conformance, driven by the shared
// `test/spec/*.tsv` fixtures at the repo root (see ../test/AGENTS.md).
//
// The fixture loader, the escape codec, the ERROR: contract and the row
// loop all come from github.com/tabnas/support/go, whose TypeScript half
// ts/test/parity.test.ts uses to run the SAME files — so the two
// implementations cannot drift without one of them going red, and neither
// can the two loaders.
//
// What is left here is only what is specific to proto: the row's options,
// and what an ERROR: cell means.

import (
	"encoding/json"
	"strings"
	"testing"

	support "github.com/tabnas/support/go"
)

// TestSpec runs every fixture in the spec directory. FindSpecDir walks up
// from the package directory, and Dir discovers the files by listing, so
// adding a .tsv runs it in both runtimes without touching either runner.
func TestSpec(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}

	support.Runner{
		ParseRow: func(input string, row *support.Row) (any, error) {
			opts, err := specOpts(row)
			if err != nil {
				return nil, err
			}
			return Parse(input, opts)
		},

		// proto's ERROR:<want> cells hold a fragment of the message —
		// there is one, "version mismatch" — rather than an error code,
		// because the rejection it names is the plugin's own version check
		// rather than a parse failure the engine gives a code to. A bare
		// ERROR still accepts any failure.
		MatchError: func(err error, want string, _ *support.Row) bool {
			return strings.Contains(err.Error(), want)
		},

		// Flatten through JSON so absent fields and field order do not
		// affect the structural comparison.
		Normalize: jsonFlatten,
	}.Dir(t, dir)
}

// specOpts decodes the row's options column into the plugin's option
// struct. An empty cell means the defaults.
func specOpts(row *support.Row) (*ProtoOptions, error) {
	raw := row.Named("opts")
	if "" == strings.TrimSpace(raw) {
		return nil, nil
	}

	var decoded struct {
		Version   ProtoVersion `json:"version"`
		Reconcile *bool        `json:"reconcile"`
	}
	if err := json.Unmarshal([]byte(raw), &decoded); err != nil {
		return nil, err
	}
	return &ProtoOptions{Version: decoded.Version, Reconcile: decoded.Reconcile}, nil
}

// jsonFlatten renders a value as JSON and reads it back as plain
// map/slice/float64/string/bool/nil. A value that will not marshal is
// returned as it is: the comparison then fails and prints it, which says
// more than a panic here would.
func jsonFlatten(v any) any {
	raw, err := json.Marshal(v)
	if err != nil {
		return v
	}
	var out any
	if err := json.Unmarshal(raw, &out); err != nil {
		return v
	}
	return out
}
