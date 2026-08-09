// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasproto

// protobuf_conformance_test.go — conformance against protoc's own parser test
// corpus, in Go.
//
// `test/protobuf-suite/*.json` is a vendored extraction of upstream protobuf's
// `src/google/protobuf/compiler/parser_unittest.cc` (v35.1) — see
// `../test/protobuf-suite/AGENTS.md` for provenance and lane meanings.
//
// This is the Go counterpart of `ts/test/protobuf-conformance.test.ts`. It
// reads the SAME files with the SAME contracts and the SAME normalisation, so
// the two runtimes cannot drift on the third-party corpus without one of them
// going red. Until this file existed, only TypeScript was measured against
// protoc; Go was measured only against the in-repo `test/spec/*.tsv` fixtures.
//
// The corpus is vendored, so this never skips: if the files are missing the
// suite FAILS rather than quietly passing. A conformance suite that silently
// does not run reports green while measuring nothing.

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"testing"
)

func suiteDir() string { return filepath.Join("..", "test", "protobuf-suite") }

type corpusCase struct {
	Name     string          `json:"name"`
	Helper   string          `json:"helper"`
	Input    string          `json:"input"`
	Expected json.RawMessage `json:"expected"`
}

func loadCorpus(t *testing.T, file string) []corpusCase {
	t.Helper()
	path := filepath.Join(suiteDir(), file)
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("cannot read the vendored conformance corpus %s: %v\n"+
			"This test does not skip: the corpus is committed to this repo, so an "+
			"absent file is a real failure, not a reason to pass quietly.", path, err)
	}
	var cases []corpusCase
	if err := json.Unmarshal(b, &cases); err != nil {
		t.Fatalf("%s: bad JSON: %v", path, err)
	}
	if len(cases) == 0 {
		t.Fatalf("%s: no cases", path)
	}
	return cases
}

// ---- declared deviations from protoc's descriptor encoding ----------------
//
// @tabnas/proto records options as a plain `{name: value}` map rather than
// protoc's `uninterpretedOption` list (see ../AGENTS.md, "Output shape"). The
// two carry the same information, so translate protoc's encoding into ours and
// compare — a name or a value we failed to capture still fails. Kept in step
// with `bridgeOptions` in the TS runner.

func optionName(parts any) string {
	list, _ := parts.([]any)
	names := make([]string, 0, len(list))
	for _, p := range list {
		m, _ := p.(map[string]any)
		part, _ := m["namePart"].(string)
		if ext, _ := m["isExtension"].(bool); ext {
			part = "(" + part + ")"
		}
		names = append(names, part)
	}
	return strings.Join(names, ".")
}

// asNumber accepts protojson's two spellings of an integer — a JSON number or,
// for 64-bit fields, a JSON string — and yields the same float64 either way.
func asNumber(v any) any {
	switch n := v.(type) {
	case float64:
		return n
	case string:
		f, err := strconv.ParseFloat(n, 64)
		if err != nil {
			return n
		}
		return f
	}
	return v
}

func optionValue(u map[string]any) any {
	if v, ok := u["stringValue"]; ok {
		s, _ := v.(string)
		raw, err := base64.StdEncoding.DecodeString(s)
		if err != nil {
			return s
		}
		return string(raw)
	}
	if v, ok := u["positiveIntValue"]; ok {
		return asNumber(v)
	}
	if v, ok := u["negativeIntValue"]; ok {
		return asNumber(v)
	}
	if v, ok := u["doubleValue"]; ok {
		// protoc evaluates `inf` / `-inf` / `-nan` to a double; we keep the
		// literal text the source wrote.
		switch v {
		case "Infinity":
			return "inf"
		case "-Infinity":
			return "-inf"
		case "NaN":
			return "-nan"
		}
		return asNumber(v)
	}
	if v, ok := u["aggregateValue"]; ok {
		return v
	}
	id, _ := u["identifierValue"].(string)
	switch id {
	case "true":
		return true
	case "false":
		return false
	}
	return id
}

func bridgeOptions(v any) any {
	o, ok := v.(map[string]any)
	if !ok {
		return v
	}
	list, ok := o["uninterpretedOption"].([]any)
	if !ok {
		return v
	}
	out := map[string]any{}
	for k, x := range o {
		if k != "uninterpretedOption" {
			out[k] = x
		}
	}
	for _, e := range list {
		u, _ := e.(map[string]any)
		out[optionName(u["name"])] = optionValue(u)
	}
	return out
}

func bridge(v any) any {
	switch t := v.(type) {
	case []any:
		out := make([]any, 0, len(t))
		for _, e := range t {
			out = append(out, bridge(e))
		}
		return out
	case map[string]any:
		out := map[string]any{}
		for k, x := range t {
			if k == "options" {
				out[k] = bridgeOptions(bridge(x))
			} else {
				out[k] = bridge(x)
			}
		}
		return out
	}
	return v
}

// normCorpus is the ONE normalisation, applied identically to BOTH sides: drop
// a key whose value is absent (JSON null) or an empty list, so a golden that
// omits a defaulted field compares equal to a descriptor that spells it out as
// `[]`. It can never hide a difference in a PRESENT value — if either side has
// a value the other lacks, the compare still fails.
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
			out[k] = nv
		}
		return out
	}
	return v
}

// sameDefaults: `defaultValue` is a string in descriptor.proto. protoc
// re-renders a numeric default through the field's C++ type; we keep the
// literal as written. Compare numerically when both sides are numbers.
func sameDefaults(a, b any) bool {
	as, aok := a.(string)
	bs, bok := b.(string)
	if !aok || !bok {
		return false
	}
	x, aerr := strconv.ParseFloat(as, 64)
	y, berr := strconv.ParseFloat(bs, 64)
	return aerr == nil && berr == nil && x == y
}

func corpusEqual(got, want any) bool {
	if reflect.DeepEqual(got, want) {
		return true
	}
	gs, gsok := got.([]any)
	ws, wsok := want.([]any)
	if gsok || wsok {
		if !gsok || !wsok || len(gs) != len(ws) {
			return false
		}
		for i := range gs {
			if !corpusEqual(gs[i], ws[i]) {
				return false
			}
		}
		return true
	}
	gm, gok := got.(map[string]any)
	wm, wok := want.(map[string]any)
	if !gok || !wok {
		return false
	}
	keys := map[string]bool{}
	for k := range gm {
		keys[k] = true
	}
	for k := range wm {
		keys[k] = true
	}
	for k := range keys {
		if corpusEqual(gm[k], wm[k]) {
			continue
		}
		if k == "defaultValue" && sameDefaults(gm[k], wm[k]) {
			continue
		}
		return false
	}
	return true
}

// outOfScope reports an input declaring an edition the plugin does not claim:
// protoc carries internal `UNSTABLE` and `NNNNN_TEST_ONLY` editions for
// in-development features. @tabnas/proto documents proto2, proto3 and editions
// 2023/2024, and rejects any other edition string.
//
// This is the Go spelling of the TS runner's `/edition\s*=\s*["'](?!2023|2024)/`
// — Go's regexp has no lookahead, so match up to the opening quote and test the
// prefix of what follows.
var editionDecl = regexp.MustCompile(`edition\s*=\s*["']`)

func outOfScope(input string) bool {
	for _, loc := range editionDecl.FindAllStringIndex(input, -1) {
		rest := input[loc[1]:]
		if !strings.HasPrefix(rest, "2023") && !strings.HasPrefix(rest, "2024") {
			return true
		}
	}
	return false
}

// internalEdition guards the exclusion list: it must stay exactly the
// protoc-internal editions, not a dumping ground for failures.
var internalEdition = regexp.MustCompile(`edition\s*=\s*["'](UNSTABLE|\d+_TEST_ONLY)["']`)

func corpusLabel(c corpusCase) string {
	one := strings.Join(strings.Fields(c.Input), " ")
	if 60 < len(one) {
		one = one[:57] + "..."
	}
	return c.Name + ": " + one
}

// parseGuarded runs Parse and reports rejection. A panic counts as rejection,
// exactly as a thrown error does in the TS runner — the two runtimes must
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

// TestProtobufCorpusValid — must parse AND equal the FileDescriptorProto
// protoc's parser produces. The real conformance bar.
func TestProtobufCorpusValid(t *testing.T) {
	valid := loadCorpus(t, "valid.json")

	var inScope []corpusCase
	for _, c := range valid {
		if outOfScope(c.Input) {
			continue
		}
		inScope = append(inScope, c)
	}
	skipped := len(valid) - len(inScope)

	t.Run("excludes only the editions the plugin does not claim", func(t *testing.T) {
		if skipped != 11 {
			t.Fatalf("excluded %d valid cases, expected 11 — do not widen the "+
				"exclusion set to hide a failure", skipped)
		}
		for _, c := range valid {
			if !outOfScope(c.Input) {
				continue
			}
			if !internalEdition.MatchString(c.Input) {
				t.Errorf("%s: excluded but does not declare a protoc-internal "+
					"edition: %q", c.Name, c.Input)
			}
		}
	})

	for _, c := range inScope {
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
				t.Fatalf("unmarshal %s: %v", b, err)
			}
			if err := json.Unmarshal(c.Expected, &want); err != nil {
				t.Fatalf("%s: bad golden: %v", c.Name, err)
			}
			ng := normCorpus(got)
			nw := normCorpus(bridge(want))

			// protoc omits `syntax` for a file with no declaration (proto2).
			gm, _ := ng.(map[string]any)
			wm, _ := nw.(map[string]any)
			if gm != nil && wm != nil {
				if _, has := wm["syntax"]; !has && gm["syntax"] == "proto2" {
					delete(gm, "syntax")
				}
			}

			if !corpusEqual(ng, nw) {
				gj, _ := json.Marshal(ng)
				wj, _ := json.Marshal(nw)
				t.Errorf("descriptor mismatch\n  input: %q\n  got  %s\n  want %s",
					c.Input, gj, wj)
			}
		})
	}
}

// TestProtobufCorpusAcceptOnly — protoc's PARSER accepts these (upstream
// asserts the error collector is empty) and publishes no descriptor golden, so
// this lane can only assert accept/reject. Must parse without failing.
func TestProtobufCorpusAcceptOnly(t *testing.T) {
	for _, c := range loadCorpus(t, "accept-only.json") {
		t.Run(corpusLabel(c), func(t *testing.T) {
			if _, rejected, why := parseGuarded(c.Input); rejected {
				t.Errorf("rejected input that protoc's parser accepts (upstream %s): %s\n  input: %q",
					c.Helper, why, c.Input)
			}
		})
	}
}

// TestProtobufLeniency — lexer-leniency probes: places where the shared tabnas
// lexer is more permissive than the .proto grammar. `accepted` is protoc's
// answer, `tabnas` is ours. Pinning both keeps the deviation surface from
// growing silently — a new divergence turns this red until it is a deliberate,
// recorded decision. Same contract as the TS runner, so a TS/Go split in what
// either runtime accepts shows up here as well.
func TestProtobufLeniency(t *testing.T) {
	path := filepath.Join(suiteDir(), "leniency.json")
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("cannot read %s: %v\nThis test does not skip.", path, err)
	}
	var file struct {
		ProtocVersion string `json:"protocVersion"`
		Probes        []struct {
			Name       string `json:"name"`
			Input      string `json:"input"`
			Why        string `json:"why"`
			Accepted   bool   `json:"accepted"`
			Protoc     string `json:"protoc"`
			Tabnas     bool   `json:"tabnas"`
			TabnasNote string `json:"tabnasNote"`
		} `json:"probes"`
	}
	if err := json.Unmarshal(b, &file); err != nil {
		t.Fatalf("%s: bad JSON: %v", path, err)
	}
	if len(file.Probes) == 0 {
		t.Fatalf("%s: no probes", path)
	}

	var diverge []string
	for _, p := range file.Probes {
		if p.Accepted != p.Tabnas {
			diverge = append(diverge, p.Name)
		}
		t.Run(p.Name, func(t *testing.T) {
			_, rejected, why := parseGuarded(p.Input)
			accepted := !rejected
			if accepted != p.Tabnas {
				note := p.TabnasNote
				if note == "" {
					note = p.Why
				}
				t.Errorf("accepted=%v, recorded tabnas=%v (protoc %s says %v)\n"+
					"  input: %q\n  why:   %s\n  detail: %s",
					accepted, p.Tabnas, file.ProtocVersion, p.Accepted, p.Input, note, why)
			}
		})
	}

	t.Run("deviates from protoc on exactly the recorded probes", func(t *testing.T) {
		sort.Strings(diverge)
		want := []string{
			"digit-separator-in-field-number",
			"exponent-field-number",
			"hash-line-comment",
			"underscore-suffixed-number-in-enum",
		}
		if !reflect.DeepEqual(diverge, want) {
			t.Errorf("recorded deviations from protoc:\n  got  %v\n  want %v", diverge, want)
		}
	})
}
