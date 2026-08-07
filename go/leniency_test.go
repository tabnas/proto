/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// leniency_test.go — Go port of ts/test/leniency.test.ts.
//
// Does this package accept things .proto does not allow? The DOCUMENTED setup
// is Proto(j) on a bare tabnas engine — there is no jsonic in the stack, so the
// json5-style leniency leak does not apply here; the json-object probes pin
// that. The same failure CLASS lives one level down, in the shared @tabnas/abnf
// lexer (jsonic's lexer), where `#` line comments, backtick strings and `1_0`
// digit separators tokenise happily and become legal proto.
//
// Probe inputs: test/leniency-probes.json (ours, committed).
// Probe VERDICTS: recorded by running the real protoc v35.1 —
// scripts/fetch-protobuf-corpus.sh writes test/protobuf-suite/leniency.json.
//
// THIS TEST MUST NEVER SKIP. Missing verdicts = loud failure.

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	tabnas "github.com/tabnas/parser/go"
)

const expectProbes = 13

type leniencyProbe struct {
	Name     string `json:"name"`
	Input    string `json:"input"`
	Why      string `json:"why"`
	Accepted bool   `json:"accepted"`
	Protoc   string `json:"protoc"`
}

type leniencyFile struct {
	ProtocVersion string          `json:"protocVersion"`
	Probes        []leniencyProbe `json:"probes"`
}

func TestLeniencyVsProtoc(t *testing.T) {
	path := filepath.Join(suiteDir(), "leniency.json")
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("protoc leniency verdicts are MISSING (%v).\n"+
			"This test does not skip. Run:\n\n    %s\n", err, fetchScript)
	}
	var f leniencyFile
	if err := json.Unmarshal(b, &f); err != nil {
		t.Fatalf("%s: bad JSON: %v", path, err)
	}
	if len(f.Probes) != expectProbes {
		t.Fatalf("expected %d probes, got %d. Do not shrink the probe set to "+
			"get green.", expectProbes, len(f.Probes))
	}

	for _, p := range f.Probes {
		t.Run(p.Name, func(t *testing.T) {
			fdp, rejected, why := parseGuarded(p.Input)
			if p.Accepted {
				if rejected {
					t.Errorf("rejected input that protoc %s accepts: %s\n  input: %q\n  why:   %s",
						f.ProtocVersion, why, p.Input, p.Why)
				}
				return
			}
			if !rejected {
				j, _ := json.Marshal(fdp)
				t.Errorf("LENIENT: accepted input that protoc %s rejects.\n"+
					"  input:  %q\n  protoc: %s\n  why:    %s\n  parsed: %s",
					f.ProtocVersion, p.Input, p.Protoc, p.Why, j)
			}
		})
	}
}

// TestLeniencyBarePluginVsDocumentedStack — the json5 leak, tested directly:
// the bare plugin and the documented entry point must classify the same inputs
// the same way. If a future change puts jsonic under this grammar, the
// divergence shows up here.
func TestLeniencyBarePluginVsDocumentedStack(t *testing.T) {
	inputs := []string{
		"{a:1}",
		"{a:1",
		`syntax = "proto3"; message M { int32 a = 1; }`,
	}
	for _, src := range inputs {
		t.Run(src, func(t *testing.T) {
			bare := func() string {
				rh := 8192
				j := tabnas.Make(tabnas.Options{Rewind: &tabnas.RewindOptions{History: &rh}})
				if err := Proto(j); err != nil {
					return "reject"
				}
				if _, err := j.Parse(src); err != nil {
					return "reject"
				}
				return "accept"
			}()
			documented := "accept"
			if _, rejected, _ := parseGuarded(src); rejected {
				documented = "reject"
			}
			if bare != documented {
				t.Errorf("plugin alone says %s but the documented stack says %s "+
					"— that is the json5 leniency leak appearing in proto.",
					bare, documented)
			}
		})
	}
}
