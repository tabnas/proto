/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Tests for the exported helper API mirroring the TS public exports
// (declaredVersion, resolveVersion, isEdition, editionEnum, SCALAR_TYPES,
// buildFile, grammarText).

package tabnasproto

import (
	"strings"
	"testing"

	tabnas "github.com/tabnas/parser/go"
)

func TestDeclaredVersion(t *testing.T) {
	cases := []struct {
		src  string
		want ProtoVersion
	}{
		{`syntax="proto3";`, "proto3"},
		{`syntax='proto2';`, "proto2"},
		{`edition="2023";`, "2023"},
		{`edition="2024";`, "2024"},
	}
	for _, c := range cases {
		got, err := DeclaredVersion(map[string]any{"src": c.src})
		if err != nil {
			t.Fatalf("DeclaredVersion(%q) error: %v", c.src, err)
		}
		if got != c.want {
			t.Errorf("DeclaredVersion(%q) = %q, want %q", c.src, got, c.want)
		}
	}

	// Nil node and non-matching src yield "" with no error.
	if got, err := DeclaredVersion(nil); got != "" || err != nil {
		t.Errorf("DeclaredVersion(nil) = %q, %v; want \"\", nil", got, err)
	}
	if got, err := DeclaredVersion(map[string]any{"src": `message M{}`}); got != "" || err != nil {
		t.Errorf("DeclaredVersion(no decl) = %q, %v; want \"\", nil", got, err)
	}

	// Unknown version value is an error.
	if _, err := DeclaredVersion(map[string]any{"src": `syntax="proto9";`}); err == nil ||
		!strings.Contains(err.Error(), `unknown syntax version "proto9"`) {
		t.Errorf("DeclaredVersion(proto9) error = %v, want unknown-version error", err)
	}
}

func TestResolveVersion(t *testing.T) {
	// Declaration and option agree.
	if v, err := ResolveVersion("proto3", "proto3", true); err != nil || v != "proto3" {
		t.Errorf("ResolveVersion agree = %q, %v", v, err)
	}
	// Mismatch with reconcile true is an error.
	if _, err := ResolveVersion("proto3", "proto2", true); err == nil ||
		!strings.Contains(err.Error(), "version mismatch") {
		t.Errorf("ResolveVersion mismatch error = %v, want version-mismatch error", err)
	}
	// Mismatch with reconcile false: the declaration wins.
	if v, err := ResolveVersion("proto3", "proto2", false); err != nil || v != "proto3" {
		t.Errorf("ResolveVersion(reconcile=false) = %q, %v; want proto3", v, err)
	}
	// Declaration only, option only, neither.
	if v, _ := ResolveVersion("2023", "", true); v != "2023" {
		t.Errorf("ResolveVersion declared-only = %q, want 2023", v)
	}
	if v, _ := ResolveVersion("", "proto3", true); v != "proto3" {
		t.Errorf("ResolveVersion option-only = %q, want proto3", v)
	}
	if v, _ := ResolveVersion("", "", true); v != "proto2" {
		t.Errorf("ResolveVersion default = %q, want proto2", v)
	}
}

func TestIsEdition(t *testing.T) {
	for _, v := range []ProtoVersion{"2023", "2024"} {
		if !IsEdition(v) {
			t.Errorf("IsEdition(%q) = false, want true", v)
		}
	}
	for _, v := range []ProtoVersion{"proto2", "proto3", ""} {
		if IsEdition(v) {
			t.Errorf("IsEdition(%q) = true, want false", v)
		}
	}
}

func TestEditionEnum(t *testing.T) {
	if got := EditionEnum("2023"); got != "EDITION_2023" {
		t.Errorf("EditionEnum(2023) = %q, want EDITION_2023", got)
	}
	if got := EditionEnum("2024"); got != "EDITION_2024" {
		t.Errorf("EditionEnum(2024) = %q, want EDITION_2024", got)
	}
}

func TestScalarTypes(t *testing.T) {
	if len(ScalarTypes) != 15 {
		t.Errorf("len(ScalarTypes) = %d, want 15", len(ScalarTypes))
	}
	for scalar, want := range map[string]string{
		"double": "TYPE_DOUBLE",
		"int32":  "TYPE_INT32",
		"string": "TYPE_STRING",
		"bytes":  "TYPE_BYTES",
	} {
		if got := ScalarTypes[scalar]; got != want {
			t.Errorf("ScalarTypes[%q] = %q, want %q", scalar, got, want)
		}
	}
	if _, ok := ScalarTypes["group"]; ok {
		t.Error("ScalarTypes should not contain non-scalar 'group'")
	}
}

func TestBuildFile(t *testing.T) {
	// Parse to a CST with the engine directly, then descend to the exported
	// BuildFile with an explicitly resolved version.
	rh := 8192
	j := tabnas.Make(tabnas.Options{Rewind: &tabnas.RewindOptions{History: &rh}})
	if err := Proto(j); err != nil {
		t.Fatalf("Proto install error: %v", err)
	}
	cst, err := j.Parse(`syntax = "proto3";
package demo;
message Person { string name = 1; }
`)
	if err != nil {
		t.Fatalf("parse error: %v", err)
	}
	root, _ := cst.(map[string]any)
	if root == nil {
		t.Fatal("expected map CST root")
	}

	file := BuildFile(root, "proto3")
	if file.Syntax != "proto3" || file.Edition != "" {
		t.Errorf("BuildFile syntax = %q / edition = %q, want proto3 / \"\"", file.Syntax, file.Edition)
	}
	if file.Package != "demo" {
		t.Errorf("BuildFile package = %q, want demo", file.Package)
	}
	if len(file.MessageType) != 1 || file.MessageType[0].Name != "Person" {
		t.Fatalf("BuildFile messageType = %+v, want one message Person", file.MessageType)
	}
	f := findField(file.MessageType[0].Field, "name")
	if f == nil || f.Type != "TYPE_STRING" {
		t.Errorf("BuildFile Person.name field = %+v, want TYPE_STRING", f)
	}

	// Edition versions are recorded via Edition (see EditionEnum).
	if got := BuildFile(root, "2023").Edition; got != "EDITION_2023" {
		t.Errorf("BuildFile edition = %q, want EDITION_2023", got)
	}
}

func TestGrammarText(t *testing.T) {
	if GrammarText == "" {
		t.Fatal("GrammarText is empty")
	}
	for _, rule := range []string{
		"proto          = [ syntaxOrEdition ] *topLevelDef",
		"; ===== edition-2024.abnf =====",
		"symbolVisibility = \"export\" / \"local\"",
	} {
		if !strings.Contains(GrammarText, rule) {
			t.Errorf("GrammarText missing %q", rule)
		}
	}
}
