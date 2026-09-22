// Rust port of `go/proto_test.go` and `go/export_test.go`, which are
// themselves ports of `ts/test/proto.test.ts` and
// `ts/test/version-detect.test.ts`.
//
// What lives here rather than in `test/spec/*.tsv` is what a fixture row
// cannot express: the shape of the exported helpers, and the assertions
// that read one part of a descriptor rather than comparing the whole of
// it. Everything expressible as source to descriptor belongs in the
// shared fixtures, and is there.

mod common;

use tabnas_proto::{
    build_file, child, child_rules, children, declared_version_src, edition_enum, gaps,
    gaps_before, is_edition, make, nsrc, parse, proto, resolve_version, scalar_type, to_descriptor,
    DescriptorProto, FieldDescriptorProto, FieldLabel, FieldType, FileDescriptorProto, OptionValue,
    ProtoOptions, ProtoVersion, SymbolVisibility, SCALAR_TYPES,
};

fn must_parse(src: &str, options: Option<&ProtoOptions>) -> FileDescriptorProto {
    parse(src, options).unwrap_or_else(|error| panic!("parse error: {error}\nsrc:\n{src}"))
}

fn find_field<'a>(fields: &'a [FieldDescriptorProto], name: &str) -> &'a FieldDescriptorProto {
    fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("no field {name}"))
}

fn find_nested<'a>(messages: &'a [DescriptorProto], name: &str) -> &'a DescriptorProto {
    messages
        .iter()
        .find(|message| message.name == name)
        .unwrap_or_else(|| panic!("no nested type {name}"))
}

const PROTO3_SRC: &str = r#"syntax = "proto3";
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
"#;

#[test]
fn proto3_package_syntax_and_dependencies() {
    let file = must_parse(PROTO3_SRC, None);
    assert_eq!(file.syntax.as_deref(), Some("proto3"));
    assert_eq!(file.package.as_deref(), Some("demo"));
    assert_eq!(
        file.dependency,
        vec!["google/protobuf/timestamp.proto", "other.proto"]
    );
    assert_eq!(file.public_dependency, vec![1]);
    assert!(file.weak_dependency.is_empty());
}

#[test]
fn proto3_fields_carry_type_number_and_label() {
    let file = must_parse(PROTO3_SRC, None);
    let fields = &file.message_type[0].field;
    let got: Vec<(&str, Option<FieldLabel>, Option<FieldType>, f64)> = fields[..3]
        .iter()
        .map(|field| (field.name.as_str(), field.label, field.r#type, field.number))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                "name",
                Some(FieldLabel::Optional),
                Some(FieldType::String),
                1.0
            ),
            (
                "age",
                Some(FieldLabel::Optional),
                Some(FieldType::Int32),
                2.0
            ),
            (
                "tags",
                Some(FieldLabel::Repeated),
                Some(FieldType::String),
                3.0
            ),
        ]
    );
    assert!(find_field(fields, "age").proto3_optional);
}

#[test]
fn proto3_map_entry_and_repeated_message_field() {
    let file = must_parse(PROTO3_SRC, None);
    let message = &file.message_type[0];
    let scores = find_field(&message.field, "scores");
    // A named type is unresolved here, so `type` is left unset (as protoc
    // does before its resolution pass) and only `type_name` is recorded.
    assert_eq!(scores.label, Some(FieldLabel::Repeated));
    assert_eq!(scores.r#type, None);
    assert_eq!(scores.type_name.as_deref(), Some("ScoresEntry"));

    let entry = find_nested(&message.nested_type, "ScoresEntry");
    assert_eq!(
        entry.options.as_ref().and_then(|opts| opts.get("mapEntry")),
        Some(&OptionValue::Bool(true))
    );
    let kv: Vec<(&str, Option<FieldType>)> = entry
        .field
        .iter()
        .map(|field| (field.name.as_str(), field.r#type))
        .collect();
    assert_eq!(
        kv,
        vec![
            ("key", Some(FieldType::String)),
            ("value", Some(FieldType::Int32)),
        ]
    );
}

#[test]
fn proto3_oneof_declarations_and_back_references() {
    let file = must_parse(PROTO3_SRC, None);
    let message = &file.message_type[0];
    // `contact` is declared; `_age` is the synthetic oneof protoc adds
    // for the proto3 explicit `optional int32 age`, appended after it.
    let names: Vec<&str> = message
        .oneof_decl
        .iter()
        .map(|oneof| oneof.name.as_str())
        .collect();
    assert_eq!(names, vec!["contact", "_age"]);
    assert_eq!(find_field(&message.field, "age").oneof_index, Some(1));

    let email = find_field(&message.field, "email");
    assert_eq!(email.oneof_index, Some(0));
    assert_eq!(email.r#type, Some(FieldType::String));
    assert!(
        !email.proto3_optional,
        "an explicit oneof member is not proto3-optional"
    );
}

#[test]
fn proto3_nested_messages_enums_reserved_ranges_and_streaming() {
    let file = must_parse(PROTO3_SRC, None);
    let message = &file.message_type[0];
    find_nested(&message.nested_type, "Address");

    let kind = &message.enum_type[0];
    let values: Vec<(&str, f64)> = kind
        .value
        .iter()
        .map(|value| (value.name.as_str(), value.number))
        .collect();
    assert_eq!(values, vec![("UNKNOWN", 0.0), ("ADMIN", 1.0)]);

    // Enum reserved ranges are INCLUSIVE: `2` is [2,2] and `9 to 11` is
    // [9,11].
    let reserved = file.enum_type[0]
        .reserved_range
        .as_ref()
        .expect("Status has reserved ranges");
    let bounds: Vec<(f64, f64)> = reserved
        .iter()
        .map(|range| (range.start, range.end))
        .collect();
    assert_eq!(bounds, vec![(2.0, 2.0), (9.0, 11.0)]);

    let method = &file.service[0].method[0];
    assert!(method.server_streaming);
    assert!(!method.client_streaming);
    assert_eq!(method.input_type, "Person");
}

const PROTO2_SRC: &str = r#"syntax = "proto2";
message Foo {
  required int32 id = 1;
  optional string name = 2 [default = "x"];
  repeated Bar bars = 3;
  extensions 100 to 199;
  group MyGroup = 4 { optional int32 a = 1; }
}
extend Foo { optional string ext = 100; }
"#;

#[test]
fn proto2_labels_and_field_options() {
    let file = must_parse(PROTO2_SRC, None);
    let fields = &file.message_type[0].field;
    assert_eq!(find_field(fields, "id").label, Some(FieldLabel::Required));

    // `default` is a pseudo-option: protoc lifts it to `default_value`.
    let name = find_field(fields, "name");
    assert_eq!(name.default_value.as_deref(), Some("x"));
    assert_eq!(name.options, None);

    let bars = find_field(fields, "bars");
    assert_eq!(bars.label, Some(FieldLabel::Repeated));
    assert_eq!(bars.r#type, None);
    assert_eq!(bars.type_name.as_deref(), Some("Bar"));
}

#[test]
fn proto2_group_expands_to_a_field_plus_a_nested_message() {
    let file = must_parse(PROTO2_SRC, None);
    let message = &file.message_type[0];
    let group = find_field(&message.field, "mygroup");
    assert_eq!(group.number, 4.0);
    assert_eq!(group.label, Some(FieldLabel::Optional));
    assert_eq!(group.r#type, Some(FieldType::Group));
    assert_eq!(group.type_name.as_deref(), Some("MyGroup"));

    let nested = find_nested(&message.nested_type, "MyGroup");
    assert_eq!(nested.field.len(), 1);
    assert_eq!(nested.field[0].name, "a");
}

#[test]
fn proto2_extension_ranges_and_top_level_extend() {
    let file = must_parse(PROTO2_SRC, None);
    // `end` is exclusive, as in descriptor.proto: `100 to 199` is [100,200).
    let ranges = file.message_type[0]
        .extension_range
        .as_ref()
        .expect("Foo has extension ranges");
    assert_eq!(ranges.len(), 1);
    assert_eq!((ranges[0].start, ranges[0].end), (100.0, 200.0));

    assert_eq!(file.extension.len(), 1);
    assert_eq!(file.extension[0].name, "ext");
    assert_eq!(file.extension[0].number, 100.0);
    assert_eq!(file.extension[0].extendee.as_deref(), Some("Foo"));
}

#[test]
fn edition_2023_records_features_at_file_and_field_level() {
    let file = must_parse(
        "edition = \"2023\";\npackage e;\noption features.field_presence = EXPLICIT;\n\
         message M { int32 a = 1 [features.field_presence = IMPLICIT]; }\n",
        None,
    );
    assert_eq!(file.edition.as_deref(), Some("EDITION_2023"));
    assert_eq!(
        file.options
            .as_ref()
            .and_then(|opts| opts.get("features.field_presence")),
        Some(&OptionValue::Str("EXPLICIT".to_string()))
    );
    let field = &file.message_type[0].field[0];
    assert_eq!(
        field
            .options
            .as_ref()
            .and_then(|opts| opts.get("features.field_presence")),
        Some(&OptionValue::Str("IMPLICIT".to_string()))
    );
}

#[test]
fn edition_2024_import_option_and_symbol_visibility() {
    let file = must_parse(
        "edition = \"2024\";\nimport option \"custom.proto\";\n\
         export message Pub { int32 a = 1; }\nmessage Outer { local enum E { A = 0; } }\n",
        None,
    );
    assert_eq!(file.edition.as_deref(), Some("EDITION_2024"));
    // `import option` is its own dependency list, not a plain import.
    assert!(file.dependency.is_empty());
    assert_eq!(
        file.option_dependency.as_deref(),
        Some(["custom.proto".to_string()].as_slice())
    );
    assert_eq!(file.message_type[0].name, "Pub");
    assert_eq!(
        file.message_type[0].visibility,
        Some(SymbolVisibility::Export)
    );
    let inner = &file.message_type[1].enum_type[0];
    assert_eq!(inner.name, "E");
    assert_eq!(inner.visibility, Some(SymbolVisibility::Local));
}

#[test]
fn whitespace_and_comments_do_not_change_the_descriptor() {
    let pretty = must_parse(
        "syntax = \"proto3\";\n\n// header\nmessage  M  {\n  int32   a   =   1 ;   // trailing\n\
         /* block\n     comment */\n  repeated  string  b  =  2 ;\n}\n",
        None,
    );
    let mini = must_parse(
        "syntax=\"proto3\";message M{int32 a=1;repeated string b=2;}",
        None,
    );
    assert_eq!(pretty.message_type, mini.message_type);
}

// ---- version detection ----------------------------------------------------

#[test]
fn detects_the_version_from_the_declaration() {
    assert_eq!(
        must_parse("syntax = \"proto2\";", None).syntax.as_deref(),
        Some("proto2")
    );
    assert_eq!(
        must_parse("syntax = \"proto3\";", None).syntax.as_deref(),
        Some("proto3")
    );
    // As in protoc, an edition file carries BOTH `syntax: "editions"` and
    // the concrete edition.
    let e23 = must_parse("edition = \"2023\";", None);
    assert_eq!(e23.edition.as_deref(), Some("EDITION_2023"));
    assert_eq!(e23.syntax.as_deref(), Some("editions"));
    let e24 = must_parse("edition = \"2024\";", None);
    assert_eq!(e24.edition.as_deref(), Some("EDITION_2024"));
    assert_eq!(e24.syntax.as_deref(), Some("editions"));
}

#[test]
fn an_explicit_option_fills_in_for_a_missing_declaration() {
    let options = ProtoOptions {
        version: Some(ProtoVersion::Proto3),
        ..ProtoOptions::default()
    };
    assert_eq!(
        must_parse("message M {}", Some(&options)).syntax.as_deref(),
        Some("proto3")
    );
    let options = ProtoOptions {
        version: Some(ProtoVersion::Edition2024),
        ..ProtoOptions::default()
    };
    assert_eq!(
        must_parse("message M {}", Some(&options))
            .edition
            .as_deref(),
        Some("EDITION_2024")
    );
    // Neither: protoc's default.
    assert_eq!(
        must_parse("message M {}", None).syntax.as_deref(),
        Some("proto2")
    );
}

#[test]
fn a_mismatch_is_an_error_unless_reconcile_is_off() {
    let options = ProtoOptions {
        version: Some(ProtoVersion::Proto2),
        reconcile: true,
    };
    let error = parse("syntax = \"proto3\";", Some(&options))
        .expect_err("a declaration disagreeing with the option is refused");
    assert!(
        error.to_string().contains("version mismatch"),
        "got {error}"
    );
    // This package declares no error codes, so the rejection carries none.
    assert_eq!(error.code(), "");

    let options = ProtoOptions {
        version: Some(ProtoVersion::Proto2),
        reconcile: false,
    };
    assert_eq!(
        must_parse("syntax = \"proto3\";", Some(&options))
            .syntax
            .as_deref(),
        Some("proto3")
    );
}

#[test]
fn an_unknown_declaration_value_is_refused_by_name() {
    let error = parse("syntax = \"proto9\";", None).expect_err("proto9 is not a version");
    assert!(
        error
            .to_string()
            .contains("unknown syntax version \"proto9\""),
        "got {error}"
    );
    let error = parse("edition = \"1999\";", None).expect_err("1999 is not an edition");
    assert!(
        error
            .to_string()
            .contains("unknown edition version \"1999\""),
        "got {error}"
    );
}

// ---- the exported helpers -------------------------------------------------

#[test]
fn declared_version_reads_the_declaration_node() {
    for (src, want) in [
        ("syntax=\"proto3\";", ProtoVersion::Proto3),
        ("syntax='proto2';", ProtoVersion::Proto2),
        ("edition=\"2023\";", ProtoVersion::Edition2023),
        ("edition=\"2024\";", ProtoVersion::Edition2024),
    ] {
        assert_eq!(declared_version_src(src).unwrap(), Some(want), "for {src}");
    }
    // No declaration at all, and a node with no `src`.
    assert_eq!(declared_version_src("message M{}").unwrap(), None);
    assert_eq!(declared_version_src("").unwrap(), None);
    // An unrecognised value is an error, not a silent None.
    assert!(declared_version_src("syntax=\"proto9\";").is_err());
}

#[test]
fn resolve_version_reconciles_declaration_and_option() {
    let p2 = ProtoVersion::Proto2;
    let p3 = ProtoVersion::Proto3;
    assert_eq!(resolve_version(Some(p3), Some(p3), true).unwrap(), p3);
    assert!(resolve_version(Some(p3), Some(p2), true).is_err());
    assert_eq!(resolve_version(Some(p3), Some(p2), false).unwrap(), p3);
    assert_eq!(
        resolve_version(Some(ProtoVersion::Edition2023), None, true).unwrap(),
        ProtoVersion::Edition2023
    );
    assert_eq!(resolve_version(None, Some(p3), true).unwrap(), p3);
    assert_eq!(resolve_version(None, None, true).unwrap(), p2);
}

#[test]
fn is_edition_and_edition_enum() {
    for version in [ProtoVersion::Edition2023, ProtoVersion::Edition2024] {
        assert!(is_edition(version), "{version}");
    }
    for version in [ProtoVersion::Proto2, ProtoVersion::Proto3] {
        assert!(!is_edition(version), "{version}");
    }
    assert_eq!(edition_enum(ProtoVersion::Edition2023), "EDITION_2023");
    assert_eq!(edition_enum(ProtoVersion::Edition2024), "EDITION_2024");
}

#[test]
fn the_scalar_table_holds_the_scalars_and_nothing_else() {
    assert_eq!(SCALAR_TYPES.len(), 15);
    for (name, want) in [
        ("double", FieldType::Double),
        ("int32", FieldType::Int32),
        ("string", FieldType::String),
        ("bytes", FieldType::Bytes),
    ] {
        assert_eq!(scalar_type(name), Some(want), "for {name}");
    }
    // `group` is syntactically known but is not a scalar.
    assert_eq!(scalar_type("group"), None);
    // And a name that reaches JavaScript's Object.prototype is not one
    // either, which is the whole of the TypeScript defect `DIVERGENCE.md`
    // records.
    assert_eq!(scalar_type("__proto__"), None);
    assert_eq!(scalar_type("constructor"), None);
}

#[test]
fn build_file_takes_a_cst_and_an_already_resolved_version() {
    let parser = make();
    let cst = parser
        .parse("syntax = \"proto3\";\npackage demo;\nmessage Person { string name = 1; }\n")
        .expect("parse");

    let file = build_file(&cst, ProtoVersion::Proto3).expect("build");
    assert_eq!(file.syntax.as_deref(), Some("proto3"));
    assert_eq!(file.edition, None);
    assert_eq!(file.package.as_deref(), Some("demo"));
    assert_eq!(file.message_type.len(), 1);
    assert_eq!(file.message_type[0].name, "Person");
    assert_eq!(
        find_field(&file.message_type[0].field, "name").r#type,
        Some(FieldType::String)
    );

    // The version is the caller's here: nothing re-reads the declaration.
    let as_edition = build_file(&cst, ProtoVersion::Edition2023).expect("build");
    assert_eq!(as_edition.edition.as_deref(), Some("EDITION_2023"));
    assert_eq!(as_edition.syntax.as_deref(), Some("editions"));
}

#[test]
fn to_descriptor_resolves_the_version_from_the_cst() {
    let parser = make();
    let cst = parser.parse("edition = \"2024\";").expect("parse");
    let file = to_descriptor(&cst, None).expect("descriptor");
    assert_eq!(file.edition.as_deref(), Some("EDITION_2024"));
}

#[test]
fn the_grammar_text_carries_every_source_file() {
    assert!(!tabnas_proto::GRAMMAR_TEXT.is_empty());
    for rule in [
        "proto          = [ syntaxOrEdition ] *topLevelDef",
        "; ===== edition-2024.abnf =====",
        "symbolVisibility = \"export\" / \"local\"",
    ] {
        assert!(
            tabnas_proto::GRAMMAR_TEXT.contains(rule),
            "GRAMMAR_TEXT is missing {rule:?}"
        );
    }
}

#[test]
fn installing_twice_on_one_instance_is_a_no_op() {
    let mut parser = tabnas_proto::engine();
    proto(&mut parser).expect("first install");
    let rules = parser.rule_names().len();
    proto(&mut parser).expect("second install");
    assert_eq!(parser.rule_names().len(), rules, "the rule set grew");
    let file = to_descriptor(&parser.parse("syntax = \"proto3\";").unwrap(), None).unwrap();
    assert_eq!(file.syntax.as_deref(), Some("proto3"));
}

#[test]
fn the_plugin_installs_the_same_grammar() {
    let mut parser = tabnas_proto::engine();
    parser
        .use_plugin(tabnas_proto::plugin(), None)
        .expect("the plugin installs");
    let file = to_descriptor(&parser.parse("syntax = \"proto2\";").unwrap(), None).unwrap();
    assert_eq!(file.syntax.as_deref(), Some("proto2"));
}

// One instance, many parses: the plugin keeps no per-parse state on the
// instance, which is what lets `parse` share one. A test that only ever
// parsed once could not tell.
#[test]
fn one_instance_parses_many_documents_independently() {
    let parser = make();
    for _ in 0..3 {
        let a = to_descriptor(&parser.parse("syntax = \"proto3\";").unwrap(), None).unwrap();
        assert_eq!(a.syntax.as_deref(), Some("proto3"));
        let b = to_descriptor(
            &parser.parse("message M { optional int32 x = 1; }").unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(b.syntax.as_deref(), Some("proto2"));
        assert_eq!(b.message_type[0].field[0].name, "x");
    }
}

// The shared lexer matches a word keyword without regard to case, so an
// rpc whose NAME is `Stream` collides with the `stream` modifier and the
// whole service is refused. That is not this port's doing: the canonical
// TypeScript refuses the identical source, measured on 2026-09-21, and
// the cause is in the tokeniser rather than in `proto-grammar/`. Pinned
// here so a repair upstream shows up as a green-to-red here rather than
// silently changing what this package accepts.
#[test]
fn an_rpc_named_after_a_keyword_is_refused_in_every_runtime() {
    // The control: the same service with any other name parses.
    let ok = must_parse(
        "service S { rpc A (X) returns (Y); rpc B (X) returns (Y); }",
        None,
    );
    assert_eq!(ok.service[0].method.len(), 2);
    let ok = must_parse("service S { rpc A (X) returns (stream Y); }", None);
    assert!(ok.service[0].method[0].server_streaming);

    for source in [
        "service S { rpc Stream (X) returns (Y); }",
        "service S { rpc A (X) returns (Y); rpc Stream (X) returns (Y); }",
    ] {
        let error = parse(source, None).expect_err("an rpc named Stream is refused");
        assert_eq!(error.code(), "unexpected", "for {source}");
    }
}

// The descriptor serializes the way the canonical runtime's
// `JSON.stringify` does, which is what makes the shared fixtures
// comparable: an integer stays an integer rather than becoming `1.0`, and
// a number JSON cannot hold becomes `null` rather than failing.
#[test]
fn a_field_number_serializes_as_an_integer() {
    let file = must_parse("message M { optional int32 a = 1; }", None);
    let json = serde_json::to_string(&file).expect("a descriptor is JSON");
    assert!(json.contains("\"number\":1,"), "got {json}");

    // `1_0` is a tabnas digit separator the .proto grammar has no use for:
    // the lexer accepts it, `Number("1_0")` is NaN, and NaN has no JSON
    // spelling but `null`.
    let file = must_parse("message M { optional int32 a = 1_0; }", None);
    let json = serde_json::to_string(&file).expect("a descriptor is JSON");
    assert!(json.contains("\"number\":null,"), "got {json}");
}

// The CST accessors are exported, and `gaps` is the one that decides
// where a bare terminal sat: `stream` ahead of a `messageType`, the `-`
// ahead of an enum value's `fieldNumber`. Both are invisible to a search
// over the flattened statement, which is what these replaced.
#[test]
fn gaps_hold_the_terminals_between_two_rule_children() {
    let parser = make();

    let cst = parser
        .parse("service S { rpc M (stream streaming.Request) returns (streaming.Reply); }")
        .expect("the rpc parses");
    let service = child(&cst, "topLevelDef").expect("a service");
    let element = child(service, "serviceElement").expect("an rpc");
    assert_eq!(
        gaps_before(element, "messageType"),
        vec!["(stream", ")returns("],
        "the modifier belongs to the request only",
    );

    let cst = parser
        .parse("enum E { A1 = 1 [(x) = -2]; B = -3; }")
        .expect("the enum parses");
    let enum_def = child(&cst, "topLevelDef").expect("an enum");
    let values = children(enum_def, "enumElement");
    assert_eq!(gaps_before(values[0], "fieldNumber"), vec!["A1="]);
    assert_eq!(gaps_before(values[1], "fieldNumber"), vec!["B=-"]);

    // Every gap and every child's own text, concatenated in order, is the
    // node's whole source: nothing is skipped and nothing is counted
    // twice.
    let rebuilt = gaps(values[0])
        .into_iter()
        .zip(child_rules(values[0]))
        .fold(String::new(), |mut acc, (gap, kid)| {
            acc.push_str(gap);
            acc.push_str(nsrc(kid));
            acc
        });
    assert_eq!(rebuilt, "A1=1[(x)=-2]");
}
