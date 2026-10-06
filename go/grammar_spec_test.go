/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// The compiled grammar. grammar_gen.go (`go generate`) compiles
// GrammarText with github.com/tabnas/abnf/go into proto-grammar.json,
// which proto.go embeds and installs. These tests hold that arrangement:
// the committed file is what the compiler emits today, and the packages
// this module ships do not import the compiler. It is a build- and
// test-time dependency only, which is why go.mod still requires it.

import (
	"bytes"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	abnf "github.com/tabnas/abnf/go"
)

func TestEmbeddedGrammarSpecIsCurrent(t *testing.T) {
	// The calls and options grammar_gen.go makes: change them together.
	spec, err := abnf.Abnf(GrammarText, &abnf.AbnfConvertOptions{
		Tag:          "proto",
		Start:        "proto",
		WordKeywords: true,
		TokenClasses: true,
		Builtins:     true,
	})
	if err != nil {
		t.Fatalf("compiling GrammarText: %v", err)
	}
	data, err := abnf.ToPureSpec(spec)
	if err != nil {
		t.Fatalf("the compiled grammar is not pure data: %v", err)
	}
	if !bytes.Equal(grammarSpec, []byte(abnf.ToJsonic(data, true, 2)+"\n")) {
		t.Fatal("go/proto-grammar.json is stale: run `go generate ./...` " +
			"(or `make generate`) and commit the result")
	}
}

// The packages this module ships (the plugin and its C library) must not
// import the ABNF compiler, directly or through anything else. `go list
// -deps` reports the whole import graph of the non-test packages, which
// is what a consumer compiles; grammar_gen.go carries `//go:build ignore`
// and is in no package.
func TestShippedPackagesImportNoCompiler(t *testing.T) {
	gobin, err := exec.LookPath("go")
	if err != nil {
		gobin = filepath.Join(runtime.GOROOT(), "bin", "go")
	}
	out, err := exec.Command(gobin, "list", "-deps", "-f", "{{.ImportPath}}", "./...").CombinedOutput()
	if err != nil {
		t.Fatalf("go list: %v\n%s", err, out)
	}
	deps := strings.Fields(string(out))
	sawEngine := false
	for _, dep := range deps {
		if dep == "github.com/tabnas/parser/go" {
			sawEngine = true
		}
		for _, compiler := range []string{"github.com/tabnas/abnf/", "github.com/tabnas/bnf/"} {
			if strings.HasPrefix(dep+"/", compiler) {
				t.Errorf("a shipped package depends on %s", dep)
			}
		}
	}
	// The listing is only evidence if it is the real graph.
	if !sawEngine {
		t.Fatalf("go list did not report the engine among %d dependencies", len(deps))
	}
}
