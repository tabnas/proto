/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// protobuf_test.go — third-party conformance: protobuf's OWN parser test corpus.
//
//	upstream  https://github.com/protocolbuffers/protobuf  v35.1
//	commit    35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03   (pinned)
//	source    src/google/protobuf/compiler/parser_unittest.cc
//
// The corpus is NOT committed to this repo. scripts/fetch-protobuf-corpus.sh
// downloads it at the pinned commit into test/protobuf-suite/ (gitignored).
//
// THIS TEST MUST NEVER SKIP. If the corpus is absent it FAILS, loudly, with
// instructions — a conformance test that quietly does not run is worse than no
// test at all, because the green tick is a lie.
//
// ts/test/protobuf.test.ts reads the SAME files with the same contracts.

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
)

const fetchScript = "scripts/fetch-protobuf-corpus.sh"

func suiteDir() string { return filepath.Join("..", "test", "protobuf-suite") }

// Case counts observed at the pinned commit. Pinned here so the corpus cannot
// be quietly shrunk (or silently grow stale) without a test going red.
var expectCounts = map[string]int{
	"valid.json":       82,
	"invalid.json":     96,
	"accept-only.json": 50,
	"excluded.json":    6,
}

type corpusCase struct {
	Name     string          `json:"name"`
	Helper   string          `json:"helper"`
	Input    string          `json:"input"`
	Expected json.RawMessage `json:"expected"`
	Error    string          `json:"error"`
	Note     string          `json:"note"`
}

func missingCorpus(what string) string {
	return fmt.Sprintf("\n\nThe protobuf conformance corpus is MISSING (%s) from %s.\n"+
		"This test does not skip. Run:\n\n    %s\n\n"+
		"(CI runs it before the tests; see .github/workflows/conformance.yml.)\n",
		what, suiteDir(), fetchScript)
}

func loadCorpus(t *testing.T, file string) []corpusCase {
	t.Helper()
	path := filepath.Join(suiteDir(), file)
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("cannot read %s: %v%s", path, err, missingCorpus(file))
	}
	var cases []corpusCase
	if err := json.Unmarshal(b, &cases); err != nil {
		t.Fatalf("%s: bad JSON: %v", path, err)
	}
	if want := expectCounts[file]; len(cases) != want {
		t.Fatalf("%s: %d cases, expected %d. The corpus changed — re-pin it "+
			"deliberately, do not just edit this number to make the suite pass.",
			file, len(cases), want)
	}
	return cases
}

// normCorpus is the ONE normalisation, applied identically to BOTH sides: drop
// a key whose value is null, an empty array, or an empty object. It exists only
// because protojson omits unset repeated fields while this package emits []
// for them. It can never hide a difference in a PRESENT value — if either side
// has a value the other lacks, the compare still fails.
//
// Nothing else is normalised. In particular this does NOT paper over `syntax`
// emitted where protoc leaves it unset, `type` emitted for an unresolved type
// reference, or descriptor fields landing inside `options`. Those are real
// divergences and are supposed to show up red.
func normCorpus(v any) any {
	switch t := v.(type) {
	case []any:
		out := make([]any, 0, len(t))
		for _, e := range t {
			out = append(out, normCorpus(e))
		}
		return out
	case map[string]any:
		out := map[string]any{}
		keys := make([]string, 0, len(t))
		for k := range t {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			nv := normCorpus(t[k])
			if nv == nil {
				continue
			}
			if a, ok := nv.([]any); ok && len(a) == 0 {
				continue
			}
			if m, ok := nv.(map[string]any); ok && len(m) == 0 {
				continue
			}
			out[k] = nv
		}
		return out
	default:
		return v
	}
}

func corpusLabel(c corpusCase) string {
	one := strings.Join(strings.Fields(c.Input), " ")
	if 60 < len(one) {
		one = one[:57] + "..."
	}
	return c.Name + ": " + one
}

// parseGuarded runs Parse and reports rejection. A panic counts as rejection,
// exactly as a thrown TypeError does in the TS runner — the two runtimes must
// classify the same input the same way, or the divergence is the finding.
func parseGuarded(src string) (fdp FileDescriptorProto, rejected bool, why string) {
	defer func() {
		if r := recover(); r != nil {
			rejected = true
			why = fmt.Sprintf("panic: %v", r)
		}
	}()
	v, err := Parse(src, nil)
	if err != nil {
		return FileDescriptorProto{}, true, err.Error()
	}
	return v, false, ""
}

// TestProtobufCorpusValid — must parse AND equal protoc's parser descriptor.
func TestProtobufCorpusValid(t *testing.T) {
	for _, c := range loadCorpus(t, "valid.json") {
		t.Run(corpusLabel(c), func(t *testing.T) {
			fdp, rejected, why := parseGuarded(c.Input)
			if rejected {
				t.Fatalf("rejected a valid document: %s\n  input: %q", why, c.Input)
			}
			b, err := json.Marshal(fdp)
			if err != nil {
				t.Fatalf("marshal: %v", err)
			}
			var got, want any
			if err := json.Unmarshal(b, &got); err != nil {
				t.Fatalf("unmarshal: %v", err)
			}
			if err := json.Unmarshal(c.Expected, &want); err != nil {
				t.Fatalf("bad golden: %v", err)
			}
			ng, nw := normCorpus(got), normCorpus(want)
			if !reflect.DeepEqual(ng, nw) {
				gj, _ := json.Marshal(ng)
				wj, _ := json.Marshal(nw)
				t.Errorf("descriptor mismatch\n  input: %q\n  got  %s\n  want %s",
					c.Input, gj, wj)
			}
		})
	}
}

// TestProtobufCorpusInvalid — must be REJECTED. protoc's parser errors on these.
func TestProtobufCorpusInvalid(t *testing.T) {
	for _, c := range loadCorpus(t, "invalid.json") {
		t.Run(corpusLabel(c), func(t *testing.T) {
			fdp, rejected, _ := parseGuarded(c.Input)
			if !rejected {
				b, _ := json.Marshal(fdp)
				t.Errorf("accepted input that protoc rejects with %q\n  input:  %q\n  parsed: %s",
					c.Error, c.Input, b)
			}
		})
	}
}

// TestProtobufCorpusAcceptOnly — protoc's PARSER accepts these (upstream
// asserts the error collector is empty); they fail only later in DescriptorPool
// validation, which this package does not perform. Upstream publishes no
// descriptor golden, so this lane can only assert accept/reject. Reported
// separately — it is never folded into the headline valid figure.
func TestProtobufCorpusAcceptOnly(t *testing.T) {
	for _, c := range loadCorpus(t, "accept-only.json") {
		t.Run(corpusLabel(c), func(t *testing.T) {
			_, rejected, why := parseGuarded(c.Input)
			if rejected {
				t.Errorf("rejected input that protoc's parser accepts (upstream %s): %s\n  input: %q",
					c.Helper, why, c.Input)
			}
		})
	}
}

// TestProtobufCorpusExclusions pins the mechanical exclusions so the extractor
// cannot quietly start dropping cases. Every exclusion must carry a reason.
func TestProtobufCorpusExclusions(t *testing.T) {
	loadCorpus(t, "excluded.json") // pins the count at exactly 6
	path := filepath.Join(suiteDir(), "excluded.json")
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("cannot read %s: %v%s", path, err, missingCorpus("excluded.json"))
	}
	var ex []map[string]any
	if err := json.Unmarshal(b, &ex); err != nil {
		t.Fatalf("%s: bad JSON: %v", path, err)
	}
	for _, e := range ex {
		if r, _ := e["reason"].(string); strings.TrimSpace(r) == "" {
			t.Errorf("excluded case %v has no reason recorded", e["case"])
		}
	}
}
