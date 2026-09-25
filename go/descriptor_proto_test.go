/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

// protobuf's own descriptor.proto (test/descriptor/README.md): the one
// real-world file that names fields after nearly every keyword and
// declares `enum Edition`. TypeScript and Rust check the same facts.

import (
	"os"
	"reflect"
	"testing"
)

func TestDescriptorProto(t *testing.T) {
	src, err := os.ReadFile("../test/descriptor/descriptor.proto")
	if err != nil {
		t.Fatal(err)
	}
	fdp := mustParse(t, string(src), nil)
	if fdp.Syntax != "proto2" || fdp.Package != "google.protobuf" {
		t.Fatalf("syntax %q package %q", fdp.Syntax, fdp.Package)
	}
	var names []string
	fields, nested := 0, 0
	for _, m := range fdp.MessageType {
		names = append(names, m.Name)
		fields += len(m.Field)
		nested += len(m.NestedType)
	}
	want := []string{
		"FileDescriptorSet", "FileDescriptorProto", "DescriptorProto",
		"ExtensionRangeOptions", "FieldDescriptorProto", "OneofDescriptorProto",
		"EnumDescriptorProto", "EnumValueDescriptorProto", "ServiceDescriptorProto",
		"MethodDescriptorProto", "FileOptions", "MessageOptions", "FieldOptions",
		"OneofOptions", "EnumOptions", "EnumValueOptions", "ServiceOptions",
		"MethodOptions", "UninterpretedOption", "FeatureSet", "FeatureSetDefaults",
		"SourceCodeInfo", "GeneratedCodeInfo",
	}
	if !reflect.DeepEqual(names, want) {
		t.Fatalf("messages %v", names)
	}
	if fields != 143 || nested != 12 {
		t.Fatalf("%d fields, %d nested messages", fields, nested)
	}
	var enums []string
	for _, e := range fdp.EnumType {
		enums = append(enums, e.Name)
	}
	if !reflect.DeepEqual(enums, []string{"Edition", "SymbolVisibility"}) {
		t.Fatalf("enums %v", enums)
	}

	var fileFields []string
	for _, f := range fdp.MessageType[1].Field {
		fileFields = append(fileFields, f.Name)
	}
	if !reflect.DeepEqual(fileFields, []string{
		"name", "package", "dependency", "public_dependency", "weak_dependency",
		"option_dependency", "message_type", "enum_type", "service", "extension",
		"options", "source_code_info", "syntax", "edition",
	}) {
		t.Fatalf("FileDescriptorProto fields %v", fileFields)
	}
	for _, m := range fdp.MessageType {
		if m.Name != "FieldOptions" {
			continue
		}
		for _, f := range m.Field {
			if f.Name != "weak" {
				continue
			}
			if f.Number != 10 || f.Type != "TYPE_BOOL" || f.DefaultValue != "false" ||
				len(f.Options) != 1 || f.Options["deprecated"] != true {
				t.Fatalf("FieldOptions.weak = %+v", f)
			}
		}
	}
	edition := fdp.EnumType[0]
	if len(edition.Value) != 14 || edition.Value[0].Name != "EDITION_UNKNOWN" ||
		edition.Value[1].Number != 900 || edition.Value[2].Name != "EDITION_PROTO2" {
		t.Fatalf("Edition values %+v", edition.Value)
	}
}
