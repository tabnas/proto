/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// FileDescriptorProto-shaped output types. This mirrors the canonical
// descriptor.proto message set (the shape `protoc --descriptor_set_out`
// produces, in its JSON form): camelCase fields, enum values as their string
// names (TYPE_INT32, LABEL_REPEATED). Only fields the parser can populate
// from a single .proto source are included; cross-file type resolution is
// left to a later pass, so TypeName is stored as written.
//
// Go port of ts/src/descriptor.ts.

package tabnasproto

// OptionValue is a string, float64, bool, or nested map[string]OptionValue —
// modelled as `any` (the TS union OptionValue).
type OptionValue = any

// FieldDescriptorProto describes a single field, extension, or map entry leaf.
type FieldDescriptorProto struct {
	Name   string `json:"name"`
	Number int    `json:"number"`
	Label  string `json:"label,omitempty"`
	Type   string `json:"type,omitempty"`
	// TypeName is set for message/enum/group field types (resolution deferred).
	TypeName string `json:"typeName,omitempty"`
	// Proto3Optional marks a proto3 explicit `optional` (protoc synthesises a
	// single-field oneof for it).
	Proto3Optional bool `json:"proto3Optional,omitempty"`
	// OneofIndex is the oneof this field belongs to, as an index into the
	// message's OneofDecl. A pointer so index 0 is distinguishable from unset.
	OneofIndex *int                   `json:"oneofIndex,omitempty"`
	Options    map[string]OptionValue `json:"options,omitempty"`
}

// EnumValueDescriptorProto is one `NAME = number` entry in an enum.
type EnumValueDescriptorProto struct {
	Name    string                 `json:"name"`
	Number  int                    `json:"number"`
	Options map[string]OptionValue `json:"options,omitempty"`
}

// Range is a half-closed [start,end] reserved/extension range.
type Range struct {
	Start int `json:"start"`
	End   int `json:"end"`
}

// EnumDescriptorProto describes an enum definition.
type EnumDescriptorProto struct {
	Name          string                     `json:"name"`
	Value         []EnumValueDescriptorProto `json:"value"`
	ReservedRange []Range                    `json:"reservedRange,omitempty"`
	ReservedName  []string                   `json:"reservedName,omitempty"`
	Options       map[string]OptionValue     `json:"options,omitempty"`
}

// OneofDescriptorProto describes a oneof declaration.
type OneofDescriptorProto struct {
	Name    string                 `json:"name"`
	Options map[string]OptionValue `json:"options,omitempty"`
}

// DescriptorProto describes a message type.
type DescriptorProto struct {
	Name           string                 `json:"name"`
	Field          []FieldDescriptorProto `json:"field"`
	NestedType     []DescriptorProto      `json:"nestedType"`
	EnumType       []EnumDescriptorProto  `json:"enumType"`
	OneofDecl      []OneofDescriptorProto `json:"oneofDecl"`
	Extension      []FieldDescriptorProto `json:"extension"`
	ExtensionRange []Range                `json:"extensionRange,omitempty"`
	ReservedRange  []Range                `json:"reservedRange,omitempty"`
	ReservedName   []string               `json:"reservedName,omitempty"`
	Options        map[string]OptionValue `json:"options,omitempty"`
}

// MethodDescriptorProto describes one rpc in a service.
type MethodDescriptorProto struct {
	Name            string                 `json:"name"`
	InputType       string                 `json:"inputType"`
	OutputType      string                 `json:"outputType"`
	ClientStreaming bool                   `json:"clientStreaming,omitempty"`
	ServerStreaming bool                   `json:"serverStreaming,omitempty"`
	Options         map[string]OptionValue `json:"options,omitempty"`
}

// ServiceDescriptorProto describes a service definition.
type ServiceDescriptorProto struct {
	Name    string                  `json:"name"`
	Method  []MethodDescriptorProto `json:"method"`
	Options map[string]OptionValue  `json:"options,omitempty"`
}

// FileDescriptorProto is the root descriptor for one parsed .proto file.
type FileDescriptorProto struct {
	// Name is not present in source; callers may set it.
	Name             string                   `json:"name,omitempty"`
	Package          string                   `json:"package,omitempty"`
	Dependency       []string                 `json:"dependency"`
	PublicDependency []int                    `json:"publicDependency"`
	WeakDependency   []int                    `json:"weakDependency"`
	MessageType      []DescriptorProto        `json:"messageType"`
	EnumType         []EnumDescriptorProto    `json:"enumType"`
	Service          []ServiceDescriptorProto `json:"service"`
	Extension        []FieldDescriptorProto   `json:"extension"`
	Options          map[string]OptionValue   `json:"options,omitempty"`
	// Syntax is 'proto2' | 'proto3' for syntax files; absent for editions.
	Syntax string `json:"syntax,omitempty"`
	// Edition is 'EDITION_2023' | 'EDITION_2024' for edition files.
	Edition string `json:"edition,omitempty"`
}

// scalarTypes maps a bare protobuf scalar type to its FieldDescriptorProto
// type. A field whose type is not here is a message/enum/group reference
// (resolution deferred) and gets a TypeName instead.
var scalarTypes = map[string]string{
	"double":   "TYPE_DOUBLE",
	"float":    "TYPE_FLOAT",
	"int32":    "TYPE_INT32",
	"int64":    "TYPE_INT64",
	"uint32":   "TYPE_UINT32",
	"uint64":   "TYPE_UINT64",
	"sint32":   "TYPE_SINT32",
	"sint64":   "TYPE_SINT64",
	"fixed32":  "TYPE_FIXED32",
	"fixed64":  "TYPE_FIXED64",
	"sfixed32": "TYPE_SFIXED32",
	"sfixed64": "TYPE_SFIXED64",
	"bool":     "TYPE_BOOL",
	"string":   "TYPE_STRING",
	"bytes":    "TYPE_BYTES",
}
