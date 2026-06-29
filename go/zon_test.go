/* Copyright (c) 2025 Richard Rodger, MIT License */

package tabnaszon

import (
	"reflect"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
)

// parse creates a jsonic instance with the Zon plugin and parses src.
func parse(t *testing.T, src string, opts ...map[string]any) any {
	t.Helper()
	j := jsonic.Make()
	j.UseDefaults(Zon, Defaults, opts...)
	result, err := j.Parse(src)
	if err != nil {
		t.Fatalf("parse(%q) unexpected error: %v", src, err)
	}
	return result
}

func parseErr(t *testing.T, src string, opts ...map[string]any) error {
	t.Helper()
	j := jsonic.Make()
	j.UseDefaults(Zon, Defaults, opts...)
	_, err := j.Parse(src)
	return err
}

func assertEqual(t *testing.T, label string, got, want any) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Errorf("%s: got %#v, want %#v", label, got, want)
	}
}

func TestScalars(t *testing.T) {
	assertEqual(t, "int", parse(t, "42"), float64(42))
	assertEqual(t, "float", parse(t, "3.14"), float64(3.14))
	assertEqual(t, "true", parse(t, "true"), true)
	assertEqual(t, "false", parse(t, "false"), false)
	assertEqual(t, "null", parse(t, "null"), nil)
	assertEqual(t, "string", parse(t, `"hello"`), "hello")
}

func TestNumericBases(t *testing.T) {
	assertEqual(t, "hex", parse(t, "0x2a"), float64(42))
	assertEqual(t, "oct", parse(t, "0o52"), float64(42))
	assertEqual(t, "bin", parse(t, "0b101010"), float64(42))
	assertEqual(t, "sep", parse(t, "1_000_000"), float64(1000000))
}

func TestEnumLiteralAsValue(t *testing.T) {
	assertEqual(t, "single", parse(t, ".foo"), "foo")
	assertEqual(t, "snake_case", parse(t, ".bar_baz"), "bar_baz")
}

func TestEmptyStruct(t *testing.T) {
	got := parse(t, ".{}")
	if !reflect.DeepEqual(got, []any{}) {
		t.Errorf("empty .{} got %#v, want []any{}", got)
	}
}

func TestSimpleStruct(t *testing.T) {
	assertEqual(t, "single", parse(t, ".{ .a = 1 }"),
		map[string]any{"a": float64(1)})
	assertEqual(t, "two", parse(t, ".{ .a = 1, .b = 2 }"),
		map[string]any{"a": float64(1), "b": float64(2)})
}

func TestTrailingCommaStruct(t *testing.T) {
	assertEqual(t, "single", parse(t, ".{ .a = 1, }"),
		map[string]any{"a": float64(1)})
	assertEqual(t, "two", parse(t, ".{ .a = 1, .b = 2, }"),
		map[string]any{"a": float64(1), "b": float64(2)})
}

func TestTuple(t *testing.T) {
	assertEqual(t, "numbers", parse(t, ".{ 1, 2, 3 }"),
		[]any{float64(1), float64(2), float64(3)})
	assertEqual(t, "strings", parse(t, `.{ "a", "b" }`),
		[]any{"a", "b"})
}

func TestTrailingCommaTuple(t *testing.T) {
	assertEqual(t, "trailing", parse(t, ".{ 1, 2, 3, }"),
		[]any{float64(1), float64(2), float64(3)})
}

func TestNestedStruct(t *testing.T) {
	assertEqual(t, "nested", parse(t, ".{ .a = .{ .b = 1 } }"),
		map[string]any{"a": map[string]any{"b": float64(1)}})
}

func TestNestedTuple(t *testing.T) {
	assertEqual(t, "nested",
		parse(t, ".{ .{ 1, 2 }, .{ 3, 4 } }"),
		[]any{
			[]any{float64(1), float64(2)},
			[]any{float64(3), float64(4)},
		})
}

func TestMixedNesting(t *testing.T) {
	got := parse(t, ".{ .xs = .{ 1, 2, 3 }, .y = .{ .z = true } }")
	want := map[string]any{
		"xs": []any{float64(1), float64(2), float64(3)},
		"y":  map[string]any{"z": true},
	}
	assertEqual(t, "mixed", got, want)
}

func TestStringEscapes(t *testing.T) {
	assertEqual(t, "newline", parse(t, `"a\nb"`), "a\nb")
	assertEqual(t, "tab", parse(t, `"a\tb"`), "a\tb")
	assertEqual(t, "backslash", parse(t, `"a\\b"`), `a\b`)
}

func TestEnumAsFieldValue(t *testing.T) {
	assertEqual(t, "field", parse(t, ".{ .kind = .red }"),
		map[string]any{"kind": "red"})
}

func TestComments(t *testing.T) {
	src := `.{
		// a comment
		.name = "x", // trailing comment
		.version = "1.0", // version
	}`
	got := parse(t, src)
	want := map[string]any{"name": "x", "version": "1.0"}
	assertEqual(t, "with comments", got, want)
}

func TestRealisticZon(t *testing.T) {
	src := `.{
		.name = "example",
		.version = "0.0.1",
		.minimum_zig_version = "0.14.0",
		.dependencies = .{
			.foo = .{
				.url = "https://example.com/foo.tar.gz",
				.hash = "1220deadbeef",
			},
		},
		.paths = .{
			"build.zig",
			"src",
			"",
		},
	}`
	got := parse(t, src)
	want := map[string]any{
		"name":                 "example",
		"version":              "0.0.1",
		"minimum_zig_version":  "0.14.0",
		"dependencies": map[string]any{
			"foo": map[string]any{
				"url":  "https://example.com/foo.tar.gz",
				"hash": "1220deadbeef",
			},
		},
		"paths": []any{"build.zig", "src", ""},
	}
	assertEqual(t, "build.zig.zon", got, want)
}

func TestCharLiteralAsNumber(t *testing.T) {
	assertEqual(t, "A", parse(t, "'A'", map[string]any{"charAsNumber": true}), float64(65))
	assertEqual(t, "n", parse(t, `'\n'`, map[string]any{"charAsNumber": true}), float64(10))
	assertEqual(t, "emoji",
		parse(t, `'\u{1F600}'`, map[string]any{"charAsNumber": true}),
		float64(0x1F600))
}

func TestCharLiteralAsString(t *testing.T) {
	assertEqual(t, "A", parse(t, "'A'"), "A")
}

func TestMultiLineString(t *testing.T) {
	src := ".{\n" +
		"\t.text = \\\\hello\n" +
		"\t\t\\\\world\n" +
		"\t,\n" +
		"}"
	got := parse(t, src)
	want := map[string]any{"text": "hello\nworld"}
	assertEqual(t, "multi-line", got, want)
}

func TestEnumTagOption(t *testing.T) {
	got := parse(t, ".{ .kind = .red }", map[string]any{"enumTag": "$enum"})
	want := map[string]any{"kind": map[string]any{"$enum": "red"}}
	assertEqual(t, "enum-tagged", got, want)
}

func TestSyntaxError(t *testing.T) {
	// `{` without `.` is not valid ZON.
	if err := parseErr(t, "{ a = 1 }"); err == nil {
		t.Error("expected error for bare { but got none")
	}
}
