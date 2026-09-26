// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! protobuf's own descriptor.proto (test/descriptor/README.md): the one
//! real-world file that names fields after nearly every keyword and
//! declares `enum Edition`. TypeScript and Go check the same facts.

use tabnas_proto::parse;

const SRC: &str = include_str!("../../test/descriptor/descriptor.proto");

#[test]
fn descriptor_proto_parses_to_the_descriptor_protoc_sees() {
    let fdp = parse(SRC, None).unwrap_or_else(|e| panic!("descriptor.proto: {e}"));
    assert_eq!(fdp.syntax.as_deref(), Some("proto2"));
    assert_eq!(fdp.package.as_deref(), Some("google.protobuf"));

    let names: Vec<&str> = fdp.message_type.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "FileDescriptorSet",
            "FileDescriptorProto",
            "DescriptorProto",
            "ExtensionRangeOptions",
            "FieldDescriptorProto",
            "OneofDescriptorProto",
            "EnumDescriptorProto",
            "EnumValueDescriptorProto",
            "ServiceDescriptorProto",
            "MethodDescriptorProto",
            "FileOptions",
            "MessageOptions",
            "FieldOptions",
            "OneofOptions",
            "EnumOptions",
            "EnumValueOptions",
            "ServiceOptions",
            "MethodOptions",
            "UninterpretedOption",
            "FeatureSet",
            "FeatureSetDefaults",
            "SourceCodeInfo",
            "GeneratedCodeInfo",
        ]
    );
    let enums: Vec<&str> = fdp.enum_type.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(enums, ["Edition", "SymbolVisibility"]);
    let fields: usize = fdp.message_type.iter().map(|m| m.field.len()).sum();
    let nested: usize = fdp.message_type.iter().map(|m| m.nested_type.len()).sum();
    assert_eq!((fields, nested), (143, 12));

    let file = &fdp.message_type[1];
    let file_fields: Vec<&str> = file.field.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        file_fields,
        [
            "name",
            "package",
            "dependency",
            "public_dependency",
            "weak_dependency",
            "option_dependency",
            "message_type",
            "enum_type",
            "service",
            "extension",
            "options",
            "source_code_info",
            "syntax",
            "edition",
        ]
    );
    let weak = fdp
        .message_type
        .iter()
        .find(|m| m.name == "FieldOptions")
        .and_then(|m| m.field.iter().find(|f| f.name == "weak"))
        .expect("FieldOptions.weak");
    assert_eq!(weak.number, 10.0);
    assert_eq!(weak.default_value.as_deref(), Some("false"));
    assert_eq!(serde_json::to_value(weak).unwrap()["type"], "TYPE_BOOL");
    assert_eq!(
        serde_json::to_value(weak).unwrap()["options"]["deprecated"],
        true
    );

    let edition = &fdp.enum_type[0];
    assert_eq!(edition.value.len(), 14);
    assert_eq!(edition.value[0].name, "EDITION_UNKNOWN");
    assert_eq!(edition.value[1].number, 900.0);
    assert_eq!(edition.value[2].name, "EDITION_PROTO2");
}
