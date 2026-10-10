/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// value_test.go — the descriptor as a tree, held to the canonical JSON
// byte for byte.
//
// Every row of test/spec/*.tsv that expects a descriptor carries, in its
// `expected` cell, the canonical JSON.stringify(parse(input, opts)) output
// exactly: ts/test/canonical-json.test.ts holds each cell to it. The
// shared runner (parity_test.go) compares a row after a JSON round trip,
// which ignores member order, so it cannot see the order a host walking
// the value sees. This file can: it writes the tree ParseValue gives the
// way JSON.stringify writes an object and compares the TEXT, so a member
// out of place, an option map in sorted order or a missing member each
// fail the row. rs/tests/value_test.rs holds the Rust port's tree to the
// same cells.

import (
	"fmt"
	"math"
	"strconv"
	"strings"
	"testing"

	tabnas "github.com/tabnas/parser/go"
	support "github.com/tabnas/support/go"
)

func TestParseValueIsTheCanonicalJSON(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	files, err := support.LoadSpecDir(dir, nil)
	if err != nil {
		t.Fatal(err)
	}
	checked := 0
	for _, file := range files {
		for _, row := range file.Rows {
			expected := row.Named("expected")
			if support.IsErrorExpect(expected) {
				continue
			}
			checked++
			opts, err := specOpts(row)
			if err != nil {
				t.Fatalf("%s: bad opts cell: %v", row.Where(), err)
			}
			tree, err := ParseValue(row.UnescNamed("input"), opts)
			if err != nil {
				t.Errorf("%s: %v", row.Where(), err)
				continue
			}
			if got := canonicalJSON(t, tree); got != expected {
				t.Errorf("%s\n  got      %s\n  expected %s", row.Where(), got, expected)
			}
		}
	}
	// Ratcheted at what is on disk today, so a loader that finds fewer rows
	// cannot pass by measuring less.
	if checked != 232 {
		t.Errorf("test/spec holds %d descriptor rows, not the 232 this test was measured against", checked)
	}
}

func TestValueStatementPlacesItsMember(t *testing.T) {
	optionFirst, err := ParseValue("option java_package = \"x\";\npackage p;", nil)
	if err != nil {
		t.Fatal(err)
	}
	packageFirst, err := ParseValue("package p;\noption java_package = \"x\";", nil)
	if err != nil {
		t.Fatal(err)
	}
	if got := strings.Join(optionFirst.Keys[7:], ","); got != "syntax,options,package" {
		t.Errorf("an option ahead of the package: %s", got)
	}
	if got := strings.Join(packageFirst.Keys[7:], ","); got != "syntax,package,options" {
		t.Errorf("the package ahead of an option: %s", got)
	}

	// The descriptors marshal as they always have: the records are not
	// part of the JSON.
	a, _ := Parse("option java_package = \"x\";\npackage p;", nil)
	b, _ := Parse("package p;\noption java_package = \"x\";", nil)
	if !support.EqualValue(jsonFlatten(a), jsonFlatten(b)) {
		t.Errorf("the two descriptors should hold the same values")
	}
}

func TestValueOptionNamesInSourceOrder(t *testing.T) {
	tree, err := ParseValue("option java_package = \"x\";\noption cc_enable_arenas = true;\noption go_package = \"y\";", nil)
	if err != nil {
		t.Fatal(err)
	}
	options, _ := tree.Get("options")
	got := strings.Join(options.(*tabnas.OrderedMap).Keys, ",")
	if got != "java_package,cc_enable_arenas,go_package" {
		t.Errorf("option names: %s", got)
	}
}

func TestValueNamesSetByHandFollowTheRecordedOnes(t *testing.T) {
	file, err := Parse("option java_package = \"x\";", nil)
	if err != nil {
		t.Fatal(err)
	}
	file.Options["b_option"] = true
	file.Options["a_option"] = true
	file.Package = "p"
	file.Name = "p.proto"
	tree := DescriptorValue(file)
	if got := strings.Join(tree.Keys[7:], ","); got != "syntax,options,package,name" {
		t.Errorf("members: %s", got)
	}
	options, _ := tree.Get("options")
	if got := strings.Join(options.(*tabnas.OrderedMap).Keys, ","); got != "java_package,a_option,b_option" {
		t.Errorf("option names: %s", got)
	}
}

func TestValueOfADescriptorBuiltByHand(t *testing.T) {
	opts := map[string]OptionValue{"z": 1.0, "a": 2.0}
	file := FileDescriptorProto{
		Options:          opts,
		OptionDependency: []string{"o.proto"},
		Package:          "p",
		Syntax:           "proto3",
		MessageType: []DescriptorProto{{
			Name:           "M",
			ReservedName:   []string{"a"},
			ReservedRange:  []Range{{Start: 1, End: 2}},
			ExtensionRange: []Range{{Start: 100, End: 200}},
			Options:        opts,
		}},
		EnumType: []EnumDescriptorProto{{
			Name:          "E",
			Options:       opts,
			ReservedName:  []string{"B"},
			ReservedRange: []Range{{Start: 5, End: 5}},
		}},
	}
	got := canonicalJSON(t, DescriptorValue(file))
	want := `{"dependency":[],"publicDependency":[],"weakDependency":[],` +
		`"messageType":[{"name":"M","field":[],"nestedType":[],"enumType":[],"oneofDecl":[],"extension":[],` +
		`"options":{"a":2,"z":1},"extensionRange":[{"start":100,"end":200}],"reservedRange":[{"start":1,"end":2}],"reservedName":["a"]}],` +
		`"enumType":[{"name":"E","value":[],"reservedRange":[{"start":5,"end":5}],"reservedName":["B"],"options":{"a":2,"z":1}}],` +
		`"service":[],"extension":[],"syntax":"proto3","package":"p","optionDependency":["o.proto"],"options":{"a":2,"z":1}}`
	if got != want {
		t.Errorf("\n  got  %s\n  want %s", got, want)
	}
}

func TestCanonicalJSONSpellsAsJavaScriptDoes(t *testing.T) {
	cases := []struct {
		in   any
		want string
	}{
		{1.0, "1"},
		{-0.0 * 1, "0"},
		{math.Copysign(0, -1), "0"},
		{1.5, "1.5"},
		{1e20, "100000000000000000000"},
		{1e21, "1e+21"},
		{0.000001, "0.000001"},
		{1e-7, "1e-7"},
		{1.5e-7, "1.5e-7"},
		{math.NaN(), "null"},
		{math.Inf(1), "null"},
		{"<&> \"\\\n\x01", `"<&>` + " " + `\"\\\n\u0001"`},
	}
	for _, c := range cases {
		if got := canonicalJSON(t, c.in); got != c.want {
			t.Errorf("canonicalJSON(%#v) = %s, want %s", c.in, got, c.want)
		}
	}
}

// canonicalJSON writes a tree as JSON.stringify writes it: members in
// order, nothing escaped that JSON.stringify leaves alone (encoding/json
// escapes <, > and & and the line and paragraph separators), a number
// spelt as JavaScript spells it, and a number no JSON can hold as null.
func canonicalJSON(t *testing.T, value any) string {
	t.Helper()
	var b strings.Builder
	if err := writeCanonical(&b, value); err != nil {
		t.Fatal(err)
	}
	return b.String()
}

func writeCanonical(b *strings.Builder, value any) error {
	switch v := value.(type) {
	case nil:
		b.WriteString("null")
	case bool:
		b.WriteString(strconv.FormatBool(v))
	case float64:
		b.WriteString(canonicalNumber(v))
	case string:
		writeCanonicalString(b, v)
	case []any:
		b.WriteByte('[')
		for i, item := range v {
			if i > 0 {
				b.WriteByte(',')
			}
			if err := writeCanonical(b, item); err != nil {
				return err
			}
		}
		b.WriteByte(']')
	case *tabnas.OrderedMap:
		b.WriteByte('{')
		for i, k := range v.Keys {
			if i > 0 {
				b.WriteByte(',')
			}
			writeCanonicalString(b, k)
			b.WriteByte(':')
			if err := writeCanonical(b, v.Vals[k]); err != nil {
				return err
			}
		}
		b.WriteByte('}')
	default:
		return fmt.Errorf("canonicalJSON: a tree holds no %T", value)
	}
	return nil
}

// canonicalNumber is ECMA-262 Number::toString: the shortest digits that
// read back as the number, in fixed form from 1e-6 up to 1e21 and in
// exponent form, with an unpadded exponent, outside it. A negative zero
// is 0, and JSON.stringify writes NaN and the infinities as null.
func canonicalNumber(f float64) string {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return "null"
	}
	if f == 0 {
		return "0"
	}
	if abs := math.Abs(f); abs < 1e-6 || abs >= 1e21 {
		mantissa, exponent, _ := strings.Cut(strconv.FormatFloat(f, 'e', -1, 64), "e")
		return mantissa + "e" + exponent[:1] + strings.TrimLeft(exponent[1:], "0")
	}
	return strconv.FormatFloat(f, 'f', -1, 64)
}

// writeCanonicalString is JSON.stringify's QuoteJSONString for well-formed
// text: the quote, the backslash and the control characters are escaped,
// and nothing else is.
func writeCanonicalString(b *strings.Builder, s string) {
	b.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			b.WriteString(`\"`)
		case '\\':
			b.WriteString(`\\`)
		case '\b':
			b.WriteString(`\b`)
		case '\f':
			b.WriteString(`\f`)
		case '\n':
			b.WriteString(`\n`)
		case '\r':
			b.WriteString(`\r`)
		case '\t':
			b.WriteString(`\t`)
		default:
			if r < 0x20 {
				fmt.Fprintf(b, `\u%04x`, r)
			} else {
				b.WriteRune(r)
			}
		}
	}
	b.WriteByte('"')
}
