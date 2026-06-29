/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Go port of ts/test/proto.test.ts and ts/test/version-detect.test.ts.

package tabnasproto

import (
	"reflect"
	"strings"
	"testing"
)

func mustParse(t *testing.T, src string, opts *ProtoOptions) FileDescriptorProto {
	t.Helper()
	fdp, err := Parse(src, opts)
	if err != nil {
		t.Fatalf("parse error: %v\nsrc:\n%s", err, src)
	}
	return fdp
}

func findField(fs []FieldDescriptorProto, name string) *FieldDescriptorProto {
	for i := range fs {
		if fs[i].Name == name {
			return &fs[i]
		}
	}
	return nil
}

func findNested(ms []DescriptorProto, name string) *DescriptorProto {
	for i := range ms {
		if ms[i].Name == name {
			return &ms[i]
		}
	}
	return nil
}

func boolPtr(b bool) *bool { return &b }

// ---- proto3 ---------------------------------------------------------------

const proto3Src = `syntax = "proto3";
package demo;
import "google/protobuf/timestamp.proto";
import public "other.proto";
message Person {
  string name = 1;          // a line comment
  optional int32 age = 2;   /* a block comment */
  repeated string tags = 3;
  map<string, int32> scores = 4;
  oneof contact { string email = 5; string phone = 6; }
  message Address { string city = 1; }
  enum Kind { UNKNOWN = 0; ADMIN = 1; }
}
enum Status { reserved 2, 9 to 11; UNKNOWN = 0; ACTIVE = 1; }
service Dir { rpc Find (Person) returns (stream Person); }
`

func TestProto3(t *testing.T) {
	fdp := mustParse(t, proto3Src, nil)

	t.Run("package, syntax, dependencies", func(t *testing.T) {
		if fdp.Syntax != "proto3" {
			t.Errorf("syntax = %q, want proto3", fdp.Syntax)
		}
		if fdp.Package != "demo" {
			t.Errorf("package = %q, want demo", fdp.Package)
		}
		if !reflect.DeepEqual(fdp.Dependency,
			[]string{"google/protobuf/timestamp.proto", "other.proto"}) {
			t.Errorf("dependency = %v", fdp.Dependency)
		}
		if !reflect.DeepEqual(fdp.PublicDependency, []int{1}) {
			t.Errorf("publicDependency = %v, want [1]", fdp.PublicDependency)
		}
	})

	t.Run("fields with types, numbers, labels", func(t *testing.T) {
		f := fdp.MessageType[0].Field
		if len(f) < 3 {
			t.Fatalf("want >=3 fields, got %d", len(f))
		}
		type tup struct {
			name, label, typ string
			number           int
		}
		got := []tup{
			{f[0].Name, f[0].Label, f[0].Type, f[0].Number},
			{f[1].Name, f[1].Label, f[1].Type, f[1].Number},
			{f[2].Name, f[2].Label, f[2].Type, f[2].Number},
		}
		want := []tup{
			{"name", "LABEL_OPTIONAL", "TYPE_STRING", 1},
			{"age", "LABEL_OPTIONAL", "TYPE_INT32", 2},
			{"tags", "LABEL_REPEATED", "TYPE_STRING", 3},
		}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("fields:\n got  %v\n want %v", got, want)
		}
	})

	t.Run("proto3Optional on explicit optional", func(t *testing.T) {
		age := findField(fdp.MessageType[0].Field, "age")
		if age == nil || !age.Proto3Optional {
			t.Errorf("age.Proto3Optional = %v, want true", age)
		}
	})

	t.Run("map entry message + repeated message field", func(t *testing.T) {
		scores := findField(fdp.MessageType[0].Field, "scores")
		if scores == nil {
			t.Fatal("no scores field")
		}
		if scores.Label != "LABEL_REPEATED" || scores.Type != "TYPE_MESSAGE" || scores.TypeName != "ScoresEntry" {
			t.Errorf("scores = %+v", scores)
		}
		entry := findNested(fdp.MessageType[0].NestedType, "ScoresEntry")
		if entry == nil {
			t.Fatal("no ScoresEntry nested type")
		}
		if entry.Options["mapEntry"] != true {
			t.Errorf("ScoresEntry.options.mapEntry = %v", entry.Options["mapEntry"])
		}
		kv := [][2]string{
			{entry.Field[0].Name, entry.Field[0].Type},
			{entry.Field[1].Name, entry.Field[1].Type},
		}
		want := [][2]string{{"key", "TYPE_STRING"}, {"value", "TYPE_INT32"}}
		if !reflect.DeepEqual(kv, want) {
			t.Errorf("entry fields = %v, want %v", kv, want)
		}
	})

	t.Run("oneof declarations and back-references", func(t *testing.T) {
		m := fdp.MessageType[0]
		if len(m.OneofDecl) != 1 || m.OneofDecl[0].Name != "contact" {
			t.Errorf("oneofDecl = %v", m.OneofDecl)
		}
		email := findField(m.Field, "email")
		if email == nil {
			t.Fatal("no email field")
		}
		if email.OneofIndex == nil || *email.OneofIndex != 0 {
			t.Errorf("email.OneofIndex = %v, want 0", email.OneofIndex)
		}
		if email.Type != "TYPE_STRING" {
			t.Errorf("email.Type = %q", email.Type)
		}
		if email.Proto3Optional {
			t.Errorf("oneof member must not be proto3Optional")
		}
	})

	t.Run("nested messages and enums", func(t *testing.T) {
		m := fdp.MessageType[0]
		if findNested(m.NestedType, "Address") == nil {
			t.Error("no Address nested message")
		}
		kind := m.EnumType[0]
		got := [][2]any{}
		for _, v := range kind.Value {
			got = append(got, [2]any{v.Name, v.Number})
		}
		want := [][2]any{{"UNKNOWN", 0}, {"ADMIN", 1}}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("Kind values = %v, want %v", got, want)
		}
	})

	t.Run("enum reserved ranges and service streaming", func(t *testing.T) {
		if !reflect.DeepEqual(fdp.EnumType[0].ReservedRange,
			[]Range{{Start: 2, End: 2}, {Start: 9, End: 11}}) {
			t.Errorf("reservedRange = %v", fdp.EnumType[0].ReservedRange)
		}
		method := fdp.Service[0].Method[0]
		if !method.ServerStreaming {
			t.Errorf("serverStreaming = false, want true")
		}
		if method.ClientStreaming {
			t.Errorf("clientStreaming = true, want false")
		}
		if method.InputType != "Person" {
			t.Errorf("inputType = %q, want Person", method.InputType)
		}
	})
}

// ---- proto2 ---------------------------------------------------------------

const proto2Src = `syntax = "proto2";
message Foo {
  required int32 id = 1;
  optional string name = 2 [default = "x"];
  repeated Bar bars = 3;
  extensions 100 to 199;
  group MyGroup = 4 { optional int32 a = 1; }
}
extend Foo { optional string ext = 100; }
`

func TestProto2(t *testing.T) {
	fdp := mustParse(t, proto2Src, nil)

	t.Run("labels and field options", func(t *testing.T) {
		f := fdp.MessageType[0].Field
		id := findField(f, "id")
		if id == nil || id.Label != "LABEL_REQUIRED" {
			t.Errorf("id = %+v", id)
		}
		name := findField(f, "name")
		if name == nil || name.Options["default"] != "x" {
			t.Errorf("name.options.default = %v", name)
		}
		bars := findField(f, "bars")
		if bars == nil || bars.Label != "LABEL_REPEATED" || bars.Type != "TYPE_MESSAGE" || bars.TypeName != "Bar" {
			t.Errorf("bars = %+v", bars)
		}
	})

	t.Run("extension ranges and top-level extend", func(t *testing.T) {
		if !reflect.DeepEqual(fdp.MessageType[0].ExtensionRange,
			[]Range{{Start: 100, End: 199}}) {
			t.Errorf("extensionRange = %v", fdp.MessageType[0].ExtensionRange)
		}
		if len(fdp.Extension) == 0 || fdp.Extension[0].Name != "ext" || fdp.Extension[0].Number != 100 {
			t.Errorf("extension = %v", fdp.Extension)
		}
	})
}

// ---- edition 2023 ---------------------------------------------------------

func TestEdition2023(t *testing.T) {
	fdp := mustParse(t, `edition = "2023";
package e;
option features.field_presence = EXPLICIT;
message M { int32 a = 1 [features.field_presence = IMPLICIT]; }
`, nil)
	if fdp.Edition != "EDITION_2023" {
		t.Errorf("edition = %q", fdp.Edition)
	}
	if fdp.Options["features.field_presence"] != "EXPLICIT" {
		t.Errorf("file option = %v", fdp.Options["features.field_presence"])
	}
	a := fdp.MessageType[0].Field[0]
	if a.Options["features.field_presence"] != "IMPLICIT" {
		t.Errorf("field option = %v", a.Options["features.field_presence"])
	}
}

// ---- edition 2024 ---------------------------------------------------------

func TestEdition2024(t *testing.T) {
	fdp := mustParse(t, `edition = "2024";
import option "custom.proto";
export message Pub { int32 a = 1; }
message Outer { local enum E { A = 0; } }
`, nil)
	if fdp.Edition != "EDITION_2024" {
		t.Errorf("edition = %q", fdp.Edition)
	}
	if !reflect.DeepEqual(fdp.Dependency, []string{"custom.proto"}) {
		t.Errorf("dependency = %v", fdp.Dependency)
	}
	if fdp.MessageType[0].Name != "Pub" {
		t.Errorf("messageType[0].name = %q", fdp.MessageType[0].Name)
	}
	if len(fdp.MessageType) < 2 || len(fdp.MessageType[1].EnumType) == 0 ||
		fdp.MessageType[1].EnumType[0].Name != "E" {
		t.Errorf("local enum E not found: %+v", fdp.MessageType)
	}
}

// ---- whitespace and comments ----------------------------------------------

func TestWhitespaceAndComments(t *testing.T) {
	pretty := mustParse(t, `syntax = "proto3";

// header
message  M  {
  int32   a   =   1 ;   // trailing
  /* block
     comment */
  repeated  string  b  =  2 ;
}
`, nil)
	mini := mustParse(t, `syntax="proto3";message M{int32 a=1;repeated string b=2;}`, nil)
	if !reflect.DeepEqual(pretty.MessageType, mini.MessageType) {
		t.Errorf("pretty vs mini differ:\n pretty %+v\n mini   %+v",
			pretty.MessageType, mini.MessageType)
	}
}

// ---- version detection ----------------------------------------------------

func TestVersionDetection(t *testing.T) {
	t.Run("detects proto2/proto3 from syntax", func(t *testing.T) {
		if got := mustParse(t, `syntax = "proto2";`, nil).Syntax; got != "proto2" {
			t.Errorf("syntax = %q", got)
		}
		if got := mustParse(t, `syntax = "proto3";`, nil).Syntax; got != "proto3" {
			t.Errorf("syntax = %q", got)
		}
	})

	t.Run("detects editions and records edition", func(t *testing.T) {
		e23 := mustParse(t, `edition = "2023";`, nil)
		if e23.Edition != "EDITION_2023" || e23.Syntax != "" {
			t.Errorf("e23 = %+v", e23)
		}
		e24 := mustParse(t, `edition = "2024";`, nil)
		if e24.Edition != "EDITION_2024" {
			t.Errorf("e24.Edition = %q", e24.Edition)
		}
	})

	t.Run("uses explicit option when no declaration", func(t *testing.T) {
		if got := mustParse(t, `message M {}`, &ProtoOptions{Version: "proto3"}).Syntax; got != "proto3" {
			t.Errorf("syntax = %q", got)
		}
		if got := mustParse(t, `message M {}`, &ProtoOptions{Version: "2024"}).Edition; got != "EDITION_2024" {
			t.Errorf("edition = %q", got)
		}
	})

	t.Run("defaults to proto2", func(t *testing.T) {
		if got := mustParse(t, `message M {}`, nil).Syntax; got != "proto2" {
			t.Errorf("syntax = %q", got)
		}
	})

	t.Run("throws on mismatch (reconcile=true)", func(t *testing.T) {
		_, err := Parse(`syntax = "proto3";`, &ProtoOptions{Version: "proto2"})
		if err == nil || !strings.Contains(err.Error(), "version mismatch") {
			t.Errorf("want version mismatch error, got %v", err)
		}
	})

	t.Run("declaration wins when reconcile=false", func(t *testing.T) {
		fdp := mustParse(t, `syntax = "proto3";`,
			&ProtoOptions{Version: "proto2", Reconcile: boolPtr(false)})
		if fdp.Syntax != "proto3" {
			t.Errorf("syntax = %q, want proto3", fdp.Syntax)
		}
	})
}
