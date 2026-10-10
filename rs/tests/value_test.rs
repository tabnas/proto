// The descriptor as a tree, held to the canonical JSON byte for byte.
//
// Every row of `test/spec/*.tsv` that expects a descriptor carries, in its
// `expected` cell, the canonical `JSON.stringify(parse(input, opts))`
// output exactly: `ts/test/canonical-json.test.ts` holds each cell to it.
// The shared runner (`parity_test.rs`) compares a row after a JSON round
// trip, which ignores member order, so it cannot see the order a host
// walking the value sees. This file can: it writes the tree
// `parse_value` gives the way `JSON.stringify` writes an object and
// compares the TEXT, so a member out of place, a number spelt the Rust
// way or a missing member each fail the row. `go/value_test.go` holds the
// Go port's tree to the same cells.

mod common;

use serde::{Serialize, Serializer};
use tabnas::Value;
use tabnas_proto::{
    descriptor_value, parse, parse_value, DescriptorProto, DescriptorRange, EnumDescriptorProto,
    FileDescriptorProto, OptionValue, Options,
};
use tabnas_support::{is_error_expect, load_spec_dir, SpecOptions};

use common::{row_options, spec_dir};

/// A tree written as `JSON.stringify` writes it: members in order, a
/// number spelt as JavaScript spells it, and a number no JSON can hold as
/// `null`.
struct Canonical<'a>(&'a Value);

impl Serialize for Canonical<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            // `OptionValue::Number` serializes through the crate's own
            // ECMA-262 spelling, which `jsnum_test.rs` grades against node.
            Value::Number(number) => OptionValue::Number(*number).serialize(serializer),
            Value::Array(items) => serializer.collect_seq(items.iter().map(Canonical)),
            Value::Object(members) => serializer.collect_map(
                members
                    .iter()
                    .map(|(name, member)| (name, Canonical(member))),
            ),
            other => other.serialize(serializer),
        }
    }
}

fn canonical(value: &Value) -> String {
    serde_json::to_string(&Canonical(value)).expect("a tree is JSON")
}

/// The member names of an object value, in order.
fn names(value: &Value) -> Vec<String> {
    match value {
        Value::Object(members) => members.keys().cloned().collect(),
        other => panic!("not an object: {other:?}"),
    }
}

/// The value under `name` in an object value.
fn member<'a>(value: &'a Value, name: &str) -> &'a Value {
    match value {
        Value::Object(members) => members
            .get(name)
            .unwrap_or_else(|| panic!("no member {name}")),
        other => panic!("not an object: {other:?}"),
    }
}

/// The first item of an array value.
fn first(value: &Value) -> &Value {
    match value {
        Value::Array(items) => items.first().expect("a non-empty list"),
        other => panic!("not a list: {other:?}"),
    }
}

#[test]
fn every_descriptor_row_is_the_canonical_json_byte_for_byte() {
    let files = load_spec_dir(spec_dir(), &SpecOptions::default()).expect("test/spec loads");
    let mut checked = 0;
    let mut failures = Vec::new();
    for file in &files {
        for row in &file.rows {
            let expected = row.named("expected");
            if is_error_expect(expected) {
                continue;
            }
            checked += 1;
            let options = row_options(row);
            let tree = parse_value(&row.unesc_named("input"), options.as_ref())
                .unwrap_or_else(|error| panic!("{}: {error}", row.location()));
            let got = canonical(&tree);
            if got != expected {
                failures.push(format!(
                    "{}\n  got      {got}\n  expected {expected}",
                    row.location()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} descriptor rows differ from the canonical JSON:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Ratcheted at what is on disk today, so a loader that finds fewer
    // rows cannot pass by measuring less.
    assert_eq!(
        checked, 235,
        "test/spec holds {checked} descriptor rows, not the 235 this suite was measured against"
    );
}

#[test]
fn a_statement_places_its_member_where_the_canonical_object_has_it() {
    let option_first =
        parse_value("option java_package = \"x\";\npackage p;", None).expect("parses");
    let package_first =
        parse_value("package p;\noption java_package = \"x\";", None).expect("parses");
    let tail = |tree: &Value| names(tree)[7..].to_vec();
    assert_eq!(tail(&option_first), ["syntax", "options", "package"]);
    assert_eq!(tail(&package_first), ["syntax", "package", "options"]);

    // The two descriptors hold the same values, so they are equal: the
    // record of the order never decides equality.
    let a = parse("option java_package = \"x\";\npackage p;", None).expect("parses");
    let b = parse("package p;\noption java_package = \"x\";", None).expect("parses");
    assert_eq!(a, b);
    assert_ne!(
        canonical(&descriptor_value(&a)),
        canonical(&descriptor_value(&b))
    );
}

/// A statement that places a member keeps it present when it yields
/// nothing, as the canonical walk's `(msg.extensionRange ||= []).push()`
/// does: `extensions 1_0;` reads no range (`Number("1_0")` is NaN) and
/// still assigns the list. The expected bytes are the TypeScript parse's,
/// which `ts/test/canonical-json.test.ts` pins, and `go/value_test.go`
/// holds Go to the same.
#[test]
fn a_statement_keeps_its_member_when_it_yields_nothing() {
    for (src, want) in [
        (
            "syntax = \"proto2\";\nmessage M { extensions 1_0; }\n",
            r#"{"dependency":[],"publicDependency":[],"weakDependency":[],"messageType":[{"name":"M","field":[],"nestedType":[],"enumType":[],"oneofDecl":[],"extension":[],"extensionRange":[]}],"enumType":[],"service":[],"extension":[],"syntax":"proto2"}"#,
        ),
        (
            "syntax = \"proto2\";\nmessage M { reserved 1_0; option deprecated = true; extensions 1_0; reserved \"a\"; }\n",
            r#"{"dependency":[],"publicDependency":[],"weakDependency":[],"messageType":[{"name":"M","field":[],"nestedType":[],"enumType":[],"oneofDecl":[],"extension":[],"options":{"deprecated":true},"reservedRange":[],"extensionRange":[],"reservedName":["a"]}],"enumType":[],"service":[],"extension":[],"syntax":"proto2"}"#,
        ),
        (
            "syntax = \"proto2\";\nenum E { A = 0; reserved 1_0; option allow_alias = true; }\n",
            r#"{"dependency":[],"publicDependency":[],"weakDependency":[],"messageType":[],"enumType":[{"name":"E","value":[{"name":"A","number":0}],"reservedRange":[],"options":{"allow_alias":true}}],"service":[],"extension":[],"syntax":"proto2"}"#,
        ),
        (
            "syntax = \"proto2\";\nmessage M { reserved 0x10; }\n",
            r#"{"dependency":[],"publicDependency":[],"weakDependency":[],"messageType":[{"name":"M","field":[],"nestedType":[],"enumType":[],"oneofDecl":[],"extension":[],"reservedRange":[]}],"enumType":[],"service":[],"extension":[],"syntax":"proto2"}"#,
        ),
    ] {
        let tree = parse_value(src, None).expect("parses");
        assert_eq!(canonical(&tree), want, "{src}");
    }
}

#[test]
fn a_number_the_canonical_reads_as_nan_stays_nan_in_the_tree() {
    // `Number("1_0")` is NaN: the canonical object holds NaN, and its
    // JSON writes `null`. Go reads 10 here, which DIVERGENCE.md records.
    let tree = parse_value("message M { optional int32 a = 1_0; }", None).expect("parses");
    let field = first(member(first(member(&tree, "messageType")), "field"));
    assert!(matches!(member(field, "number"), Value::Number(number) if number.is_nan()));
    assert!(canonical(field).contains("\"number\":null"));
}

#[test]
fn a_member_set_after_the_parse_follows_the_ones_the_parse_placed() {
    let mut file = parse("package p;", None).expect("parses");
    let mut options = Options::new();
    options.insert(
        "java_package".to_string(),
        OptionValue::Str("x".to_string()),
    );
    file.options = Some(options);
    file.name = Some("p.proto".to_string());
    let tree = descriptor_value(&file);
    assert_eq!(names(&tree)[7..], ["syntax", "package", "options", "name"]);
}

#[test]
fn a_descriptor_built_by_hand_takes_the_documented_order() {
    let mut options = Options::new();
    options.insert("deprecated".to_string(), OptionValue::Bool(true));

    let mut message = DescriptorProto::new("M");
    message.reserved_name = Some(vec!["a".to_string()]);
    message.reserved_range = Some(vec![DescriptorRange::new(1.0, 2.0)]);
    message.extension_range = Some(vec![DescriptorRange::new(100.0, 200.0)]);
    message.options = Some(options.clone());

    let mut enumeration = EnumDescriptorProto::new("E");
    enumeration.options = Some(options.clone());
    enumeration.reserved_name = Some(vec!["B".to_string()]);
    enumeration.reserved_range = Some(vec![DescriptorRange::new(5.0, 5.0)]);

    let file = FileDescriptorProto {
        options: Some(options),
        option_dependency: Some(vec!["o.proto".to_string()]),
        package: Some("p".to_string()),
        syntax: Some("proto3".to_string()),
        message_type: vec![message],
        enum_type: vec![enumeration],
        ..FileDescriptorProto::default()
    };
    let tree = descriptor_value(&file);
    assert_eq!(
        names(&tree),
        [
            "dependency",
            "publicDependency",
            "weakDependency",
            "messageType",
            "enumType",
            "service",
            "extension",
            "syntax",
            "package",
            "optionDependency",
            "options",
        ]
    );
    assert_eq!(
        names(first(member(&tree, "messageType")))[6..],
        ["options", "extensionRange", "reservedRange", "reservedName"]
    );
    assert_eq!(
        names(first(member(&tree, "enumType"))),
        ["name", "value", "reservedRange", "reservedName", "options"]
    );
}
