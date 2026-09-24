/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! FileDescriptorProto-shaped output types.
//!
//! This mirrors the canonical `descriptor.proto` message set (the shape
//! `protoc --descriptor_set_out` produces, in its JSON form): camelCase
//! fields, enum values as their string names (`TYPE_INT32`,
//! `LABEL_REPEATED`). Only fields the parser can populate from a single
//! `.proto` source are included; cross-file type resolution is left to a
//! later pass, so `type_name` is stored as written.
//!
//! Rust port of `ts/src/descriptor.ts`.

use indexmap::IndexMap;
use serde::{Serialize, Serializer};
use serde_json::value::RawValue;

use crate::jsnum::js_number_to_string;

/// A field's cardinality label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FieldLabel {
    #[serde(rename = "LABEL_OPTIONAL")]
    Optional,
    #[serde(rename = "LABEL_REQUIRED")]
    Required,
    #[serde(rename = "LABEL_REPEATED")]
    Repeated,
}

impl FieldLabel {
    /// The name descriptor JSON carries, such as `LABEL_REPEATED`.
    pub fn as_str(self) -> &'static str {
        match self {
            FieldLabel::Optional => "LABEL_OPTIONAL",
            FieldLabel::Required => "LABEL_REQUIRED",
            FieldLabel::Repeated => "LABEL_REPEATED",
        }
    }
}

/// A field's wire type, for the types a single source file settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FieldType {
    #[serde(rename = "TYPE_DOUBLE")]
    Double,
    #[serde(rename = "TYPE_FLOAT")]
    Float,
    #[serde(rename = "TYPE_INT64")]
    Int64,
    #[serde(rename = "TYPE_UINT64")]
    Uint64,
    #[serde(rename = "TYPE_INT32")]
    Int32,
    #[serde(rename = "TYPE_FIXED64")]
    Fixed64,
    #[serde(rename = "TYPE_FIXED32")]
    Fixed32,
    #[serde(rename = "TYPE_BOOL")]
    Bool,
    #[serde(rename = "TYPE_STRING")]
    String,
    #[serde(rename = "TYPE_GROUP")]
    Group,
    #[serde(rename = "TYPE_MESSAGE")]
    Message,
    #[serde(rename = "TYPE_BYTES")]
    Bytes,
    #[serde(rename = "TYPE_UINT32")]
    Uint32,
    #[serde(rename = "TYPE_ENUM")]
    Enum,
    #[serde(rename = "TYPE_SFIXED32")]
    Sfixed32,
    #[serde(rename = "TYPE_SFIXED64")]
    Sfixed64,
    #[serde(rename = "TYPE_SINT32")]
    Sint32,
    #[serde(rename = "TYPE_SINT64")]
    Sint64,
}

impl FieldType {
    /// The name descriptor JSON carries, such as `TYPE_INT32`.
    pub fn as_str(self) -> &'static str {
        match self {
            FieldType::Double => "TYPE_DOUBLE",
            FieldType::Float => "TYPE_FLOAT",
            FieldType::Int64 => "TYPE_INT64",
            FieldType::Uint64 => "TYPE_UINT64",
            FieldType::Int32 => "TYPE_INT32",
            FieldType::Fixed64 => "TYPE_FIXED64",
            FieldType::Fixed32 => "TYPE_FIXED32",
            FieldType::Bool => "TYPE_BOOL",
            FieldType::String => "TYPE_STRING",
            FieldType::Group => "TYPE_GROUP",
            FieldType::Message => "TYPE_MESSAGE",
            FieldType::Bytes => "TYPE_BYTES",
            FieldType::Uint32 => "TYPE_UINT32",
            FieldType::Enum => "TYPE_ENUM",
            FieldType::Sfixed32 => "TYPE_SFIXED32",
            FieldType::Sfixed64 => "TYPE_SFIXED64",
            FieldType::Sint32 => "TYPE_SINT32",
            FieldType::Sint64 => "TYPE_SINT64",
        }
    }
}

/// Symbol visibility, an edition-2024 feature (`export` / `local`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SymbolVisibility {
    #[serde(rename = "VISIBILITY_EXPORT")]
    Export,
    #[serde(rename = "VISIBILITY_LOCAL")]
    Local,
}

impl SymbolVisibility {
    /// The name descriptor JSON carries, such as `VISIBILITY_EXPORT`.
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolVisibility::Export => "VISIBILITY_EXPORT",
            SymbolVisibility::Local => "VISIBILITY_LOCAL",
        }
    }
}

/// An option's value, as the source wrote it.
///
/// The canonical `OptionValue` union also admits a nested map, and no
/// input produces one: an aggregate value (`option (f) = { a: 1 };`) is
/// kept as the text between its braces, as protoc records it (every
/// comment turned to the spaces and newlines that hold the layout; see
/// `aggregate.rs`), so the three variants here are the whole range the
/// walk emits.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OptionValue {
    Bool(bool),
    /// A numeric constant, read exactly as JavaScript's `Number()` reads
    /// it. Serializes as `null` when the source names a value no JSON
    /// number can hold, which is what `JSON.stringify` writes for `NaN`
    /// and the infinities.
    Number(#[serde(serialize_with = "json_number")] f64),
    Str(String),
}

/// An option set, keyed by the option name exactly as written
/// (`ctype`, `(foo)`, `foo.(.bar.baz).qux`), in source order.
pub type Options = IndexMap<String, OptionValue>;

/// A number spelt exactly as `JSON.stringify` spells it.
///
/// The descriptor's JSON is the package's output format, and matching the
/// canonical runtime means matching its TEXT, not only its value. Rust's
/// `f64` formatting is not ECMA-262 6.1.6.1.20: it renders every float in
/// float form (`1.0` for a field number), it switches to exponent form at
/// different magnitudes (`1e-6` where JavaScript writes `0.000001`, and
/// `1e+20` where JavaScript writes `100000000000000000000`), and it keeps
/// the sign of a negative zero. [`js_number_to_string`] is the algorithm;
/// this hands its text to `serde_json` verbatim through a `RawValue`.
///
/// Non-finite values have no JSON spelling at all: `JSON.stringify`
/// writes `null` for them and so does this.
pub(crate) fn json_number<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if !value.is_finite() {
        return serializer.serialize_none();
    }
    let text = js_number_to_string(*value);
    let raw = RawValue::from_string(text).map_err(serde::ser::Error::custom)?;
    raw.serialize(serializer)
}

/// A numeric range. `end` is EXCLUSIVE for message extension and reserved
/// ranges and INCLUSIVE for enum reserved ranges, the same asymmetry
/// protoc has.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DescriptorRange {
    #[serde(serialize_with = "json_number")]
    pub start: f64,
    #[serde(serialize_with = "json_number")]
    pub end: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
}

impl DescriptorRange {
    /// A range with no options.
    pub fn new(start: f64, end: f64) -> Self {
        DescriptorRange {
            start,
            end,
            options: None,
        }
    }
}

/// One field, extension, or synthesised map-entry leaf.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FieldDescriptorProto {
    pub name: String,
    #[serde(serialize_with = "json_number")]
    pub number: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<FieldLabel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<FieldType>,
    /// Set for message, enum and group field types (resolution deferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// For an `extend` member: the message being extended, as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extendee: Option<String>,
    /// The `json_name = "..."` pseudo-option, lifted out of `options`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_name: Option<String>,
    /// The `default = ...` pseudo-option, lifted out of `options`. Always
    /// a string, as in descriptor.proto; the literal is kept as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// proto3 explicit `optional`, which synthesises a single-field oneof.
    #[serde(skip_serializing_if = "is_false")]
    pub proto3_optional: bool,
    /// The oneof this field belongs to, as an index into the message's
    /// `oneof_decl`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oneof_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// One `NAME = number` entry in an enum.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnumValueDescriptorProto {
    pub name: String,
    #[serde(serialize_with = "json_number")]
    pub number: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
}

/// An enum definition.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumDescriptorProto {
    pub name: String,
    pub value: Vec<EnumValueDescriptorProto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved_range: Option<Vec<DescriptorRange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved_name: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<SymbolVisibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
}

impl EnumDescriptorProto {
    /// An empty enum with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        EnumDescriptorProto {
            name: name.into(),
            value: Vec::new(),
            reserved_range: None,
            reserved_name: None,
            visibility: None,
            options: None,
        }
    }
}

/// A oneof declaration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OneofDescriptorProto {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
}

/// A message type.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorProto {
    pub name: String,
    pub field: Vec<FieldDescriptorProto>,
    pub nested_type: Vec<DescriptorProto>,
    pub enum_type: Vec<EnumDescriptorProto>,
    pub oneof_decl: Vec<OneofDescriptorProto>,
    pub extension: Vec<FieldDescriptorProto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_range: Option<Vec<DescriptorRange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved_range: Option<Vec<DescriptorRange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved_name: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<SymbolVisibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
}

impl DescriptorProto {
    /// An empty message with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        DescriptorProto {
            name: name.into(),
            field: Vec::new(),
            nested_type: Vec::new(),
            enum_type: Vec::new(),
            oneof_decl: Vec::new(),
            extension: Vec::new(),
            extension_range: None,
            reserved_range: None,
            reserved_name: None,
            visibility: None,
            options: None,
        }
    }
}

/// One rpc in a service.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodDescriptorProto {
    pub name: String,
    pub input_type: String,
    pub output_type: String,
    #[serde(skip_serializing_if = "is_false")]
    pub client_streaming: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub server_streaming: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
}

/// A service definition.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServiceDescriptorProto {
    pub name: String,
    pub method: Vec<MethodDescriptorProto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
}

/// The root descriptor for one parsed `.proto` file.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileDescriptorProto {
    /// Not present in source; callers may set it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    pub dependency: Vec<String>,
    pub public_dependency: Vec<usize>,
    pub weak_dependency: Vec<usize>,
    /// `import option "..."` targets (edition 2024), when used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_dependency: Option<Vec<String>>,
    pub message_type: Vec<DescriptorProto>,
    pub enum_type: Vec<EnumDescriptorProto>,
    pub service: Vec<ServiceDescriptorProto>,
    pub extension: Vec<FieldDescriptorProto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Options>,
    /// `proto2` or `proto3` for syntax files; `editions` for edition
    /// files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syntax: Option<String>,
    /// `EDITION_2023` or `EDITION_2024` for edition files, absent
    /// otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
}

/// protoc's sentinel for `to max` in a message range. Field numbers stop
/// at 2^29-1, so an exclusive `end` is 2^29.
pub const MAX_FIELD_NUMBER_END: f64 = 536_870_912.0;
/// protoc's sentinel for `to max` in a `message_set_wire_format` message,
/// which may use the full 32-bit space.
pub const MAX_MESSAGE_SET_END: f64 = 2_147_483_647.0;
/// protoc's sentinel for `to max` in an enum reserved range. Enum numbers
/// are plain int32 and enum reserved ranges are inclusive.
pub const MAX_ENUM_NUMBER: f64 = 2_147_483_647.0;

/// Scalar protobuf types and their `FieldDescriptorProto` type, in the
/// order `ts/src/descriptor.ts` declares them. A field whose type is not
/// here is a message, enum or group reference (resolution deferred) and
/// gets a `type_name` instead.
pub const SCALAR_TYPES: &[(&str, FieldType)] = &[
    ("double", FieldType::Double),
    ("float", FieldType::Float),
    ("int32", FieldType::Int32),
    ("int64", FieldType::Int64),
    ("uint32", FieldType::Uint32),
    ("uint64", FieldType::Uint64),
    ("sint32", FieldType::Sint32),
    ("sint64", FieldType::Sint64),
    ("fixed32", FieldType::Fixed32),
    ("fixed64", FieldType::Fixed64),
    ("sfixed32", FieldType::Sfixed32),
    ("sfixed64", FieldType::Sfixed64),
    ("bool", FieldType::Bool),
    ("string", FieldType::String),
    ("bytes", FieldType::Bytes),
];

/// The scalar type a bare type name names, or `None` for a reference the
/// parser leaves unresolved.
pub fn scalar_type(name: &str) -> Option<FieldType> {
    SCALAR_TYPES
        .iter()
        .find(|(scalar, _)| *scalar == name)
        .map(|(_, field_type)| *field_type)
}
