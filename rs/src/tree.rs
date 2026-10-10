/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! The descriptor as a tree: the plain value the canonical `parse`
//! returns, member for member and in its member order.
//!
//! The canonical walk (`ts/src/build-descriptor.ts`) builds the
//! descriptor as a JavaScript object, and an object lists its members in
//! the order they were first assigned. [`FileDescriptorProto`] serializes
//! in the order its fields are declared instead, so its JSON holds the
//! same members in a different order: `syntax` after `options` where the
//! canonical object has it before, `proto3Optional` after `typeName`
//! where the canonical has it straight after `label`. A host that walks
//! the value, as a translation does when it streams a tree's events,
//! sees the order, so it reads this tree instead.
//!
//! Every member's place is the one the canonical walk gives it. Most are
//! fixed by the walk's own code; the statement-ordered ones are read from
//! each container's [`MemberOrder`].

use indexmap::IndexMap;
use tabnas::Value;

use crate::descriptor::{
    DescriptorProto, DescriptorRange, EnumDescriptorProto, EnumValueDescriptorProto,
    FieldDescriptorProto, FileDescriptorProto, MemberOrder, MethodDescriptorProto,
    OneofDescriptorProto, OptionValue, Options, ServiceDescriptorProto,
};

/// The descriptor as the tree the canonical `parse` returns: an object
/// for each message, an array for each list, and every member named and
/// ordered as the canonical object has it.
///
/// This is what a host that walks the value reads, rather than
/// [`FileDescriptorProto`]'s own serialization, which holds the same
/// members in the order the struct declares them. Numbers are
/// [`Value::Number`], so a field number JavaScript's `Number()` cannot
/// read stays `NaN`, as it does in the canonical object; a JSON writer
/// spells it `null`.
///
/// The members a statement places, such as a file's `package` and
/// `options`, follow each container's [`MemberOrder`], which the walk
/// records. A descriptor built by hand carries no record, and those
/// members then come in the order [`MemberOrder`] documents.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let file = tabnas_proto::parse("option java_package = \"x\";\npackage p;", None)?;
///     let tree = tabnas_proto::descriptor_value(&file);
///     let tabnas::Value::Object(members) = tree else { panic!("an object") };
///     let names: Vec<&str> = members.keys().map(String::as_str).collect();
///     assert_eq!(
///         names,
///         [
///             "dependency", "publicDependency", "weakDependency", "messageType", "enumType",
///             "service", "extension", "syntax", "options", "package",
///         ]
///     );
///     Ok(())
/// }
/// ```
pub fn descriptor_value(file: &FileDescriptorProto) -> Value {
    let mut out = Members::new();
    out.put("dependency", list(&file.dependency, |name| string(name)));
    out.put(
        "publicDependency",
        list(&file.public_dependency, |at| index(*at)),
    );
    out.put(
        "weakDependency",
        list(&file.weak_dependency, |at| index(*at)),
    );
    out.put("messageType", list(&file.message_type, message_value));
    out.put("enumType", list(&file.enum_type, enum_value));
    out.put("service", list(&file.service, service_value));
    out.put("extension", list(&file.extension, field_value));
    // An edition file assigns `edition` and then `syntax`, a syntax file
    // `syntax` alone, both before any statement is read.
    out.put_some("edition", file.edition.as_deref().map(string));
    out.put_some("syntax", file.syntax.as_deref().map(string));
    out.put_ordered(
        &file.member_order,
        [
            ("package", file.package.as_deref().map(string)),
            (
                "optionDependency",
                file.option_dependency
                    .as_ref()
                    .map(|targets| list(targets, |target| string(target))),
            ),
            ("options", file.options.as_ref().map(options_value)),
        ],
    );
    // The walk never sets `name`; a caller who does sets it after the
    // walk, which places it last.
    out.put_some("name", file.name.as_deref().map(string));
    out.done()
}

/// An object under construction, its members in the order put.
struct Members(IndexMap<String, Value>);

impl Members {
    fn new() -> Self {
        Members(IndexMap::new())
    }

    fn put(&mut self, name: &str, value: Value) {
        self.0.insert(name.to_string(), value);
    }

    fn put_some(&mut self, name: &str, value: Option<Value>) {
        if let Some(value) = value {
            self.put(name, value);
        }
    }

    /// The statement-ordered members: those `order` recorded, in its
    /// order, then the rest in the order given.
    fn put_ordered<const N: usize>(
        &mut self,
        order: &MemberOrder,
        members: [(&'static str, Option<Value>); N],
    ) {
        let mut pending: Vec<(&'static str, Option<Value>)> = members.into_iter().collect();
        for name in order.members() {
            if let Some(at) = pending.iter().position(|(member, _)| member == name) {
                let (member, value) = pending.remove(at);
                self.put_some(member, value);
            }
        }
        for (member, value) in pending {
            self.put_some(member, value);
        }
    }

    fn done(self) -> Value {
        Value::object(self.0)
    }
}

fn string(text: &str) -> Value {
    Value::String(text.to_string())
}

fn index(at: usize) -> Value {
    Value::Number(at as f64)
}

fn list<T>(items: &[T], each: impl Fn(&T) -> Value) -> Value {
    Value::array(items.iter().map(each).collect())
}

fn options_value(options: &Options) -> Value {
    Value::object(
        options
            .iter()
            .map(|(name, value)| (name.clone(), option_value(value)))
            .collect(),
    )
}

fn option_value(value: &OptionValue) -> Value {
    match value {
        OptionValue::Bool(flag) => Value::Bool(*flag),
        OptionValue::Number(number) => Value::Number(*number),
        OptionValue::Str(text) => string(text),
    }
}

/// `{ name, field, nestedType, enumType, oneofDecl, extension }`, then
/// `options`, which the walk reads in a pass of its own before the other
/// statements, then the statement-ordered ranges and names, then the
/// `visibility` an edition-2024 `export` or `local` adds last.
fn message_value(message: &DescriptorProto) -> Value {
    let mut out = Members::new();
    out.put("name", string(&message.name));
    out.put("field", list(&message.field, field_value));
    out.put("nestedType", list(&message.nested_type, message_value));
    out.put("enumType", list(&message.enum_type, enum_value));
    out.put("oneofDecl", list(&message.oneof_decl, oneof_value));
    out.put("extension", list(&message.extension, field_value));
    out.put_some("options", message.options.as_ref().map(options_value));
    out.put_ordered(
        &message.member_order,
        [
            (
                "extensionRange",
                message
                    .extension_range
                    .as_ref()
                    .map(|ranges| list(ranges, range_value)),
            ),
            (
                "reservedRange",
                message
                    .reserved_range
                    .as_ref()
                    .map(|ranges| list(ranges, range_value)),
            ),
            (
                "reservedName",
                message
                    .reserved_name
                    .as_ref()
                    .map(|names| list(names, |name| string(name))),
            ),
        ],
    );
    out.put_some(
        "visibility",
        message
            .visibility
            .map(|visibility| string(visibility.as_str())),
    );
    out.done()
}

/// `{ name, value }`, then the statement-ordered ranges, names and
/// options, then `visibility`.
fn enum_value(enumeration: &EnumDescriptorProto) -> Value {
    let mut out = Members::new();
    out.put("name", string(&enumeration.name));
    out.put("value", list(&enumeration.value, enum_member_value));
    out.put_ordered(
        &enumeration.member_order,
        [
            (
                "reservedRange",
                enumeration
                    .reserved_range
                    .as_ref()
                    .map(|ranges| list(ranges, range_value)),
            ),
            (
                "reservedName",
                enumeration
                    .reserved_name
                    .as_ref()
                    .map(|names| list(names, |name| string(name))),
            ),
            ("options", enumeration.options.as_ref().map(options_value)),
        ],
    );
    out.put_some(
        "visibility",
        enumeration
            .visibility
            .map(|visibility| string(visibility.as_str())),
    );
    out.done()
}

fn enum_member_value(member: &EnumValueDescriptorProto) -> Value {
    let mut out = Members::new();
    out.put("name", string(&member.name));
    out.put("number", Value::Number(member.number));
    out.put_some("options", member.options.as_ref().map(options_value));
    out.done()
}

/// The canonical field: `{ name, number, label, proto3Optional?, type? or
/// typeName? }` as one literal, then the pseudo-options and options the
/// field's own list sets, then `extendee` for an `extend` member, and
/// last the `oneofIndex` a oneof, declared or synthesised, assigns once
/// the field is built.
fn field_value(field: &FieldDescriptorProto) -> Value {
    let mut out = Members::new();
    out.put("name", string(&field.name));
    out.put("number", Value::Number(field.number));
    out.put_some("label", field.label.map(|label| string(label.as_str())));
    if field.proto3_optional {
        out.put("proto3Optional", Value::Bool(true));
    }
    out.put_some(
        "type",
        field.r#type.map(|field_type| string(field_type.as_str())),
    );
    out.put_some("typeName", field.type_name.as_deref().map(string));
    out.put_some("jsonName", field.json_name.as_deref().map(string));
    out.put_some("defaultValue", field.default_value.as_deref().map(string));
    out.put_some("options", field.options.as_ref().map(options_value));
    out.put_some("extendee", field.extendee.as_deref().map(string));
    out.put_some("oneofIndex", field.oneof_index.map(index));
    out.done()
}

fn range_value(range: &DescriptorRange) -> Value {
    let mut out = Members::new();
    out.put("start", Value::Number(range.start));
    out.put("end", Value::Number(range.end));
    out.put_some("options", range.options.as_ref().map(options_value));
    out.done()
}

fn oneof_value(oneof: &OneofDescriptorProto) -> Value {
    let mut out = Members::new();
    out.put("name", string(&oneof.name));
    out.put_some("options", oneof.options.as_ref().map(options_value));
    out.done()
}

fn service_value(service: &ServiceDescriptorProto) -> Value {
    let mut out = Members::new();
    out.put("name", string(&service.name));
    out.put("method", list(&service.method, method_value));
    out.put_some("options", service.options.as_ref().map(options_value));
    out.done()
}

fn method_value(method: &MethodDescriptorProto) -> Value {
    let mut out = Members::new();
    out.put("name", string(&method.name));
    out.put("inputType", string(&method.input_type));
    out.put("outputType", string(&method.output_type));
    if method.client_streaming {
        out.put("clientStreaming", Value::Bool(true));
    }
    if method.server_streaming {
        out.put("serverStreaming", Value::Bool(true));
    }
    out.put_some("options", method.options.as_ref().map(options_value));
    out.done()
}
