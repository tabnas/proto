/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! Walk the `{rule, src, kids}` CST the proto ABNF grammar produces and
//! assemble a FileDescriptorProto-shaped value.
//!
//! Lexical atoms arrive as whole-word tokens, and abnf's leading-ref
//! inlining means the specific statement rule (message, field, enum and
//! the rest) is folded into the enclosing `topLevelDef` or
//! `messageElement` dispatch node; the statement kind is recovered from
//! the keyword that precedes the node's first child ([`kw`]).
//!
//! Rust port of `ts/src/build-descriptor.ts`.

use tabnas::{Lexer, Value};

use crate::descriptor::{
    scalar_type, DescriptorProto, DescriptorRange, EnumDescriptorProto, EnumValueDescriptorProto,
    FieldDescriptorProto, FieldLabel, FieldType, FileDescriptorProto, MethodDescriptorProto,
    OneofDescriptorProto, OptionValue, Options, ServiceDescriptorProto, SymbolVisibility,
    MAX_ENUM_NUMBER, MAX_FIELD_NUMBER_END, MAX_MESSAGE_SET_END,
};
use crate::detect_version::{edition_enum, is_edition, ProtoVersion};
use crate::error::ProtoError;
use crate::jsnum::{is_js_whitespace, js_number};
use crate::node::{child, child_rules, children, gaps_before, kw, nrule, nsrc, src_or};
use crate::strings::adjacent_value;

/// How deep a document may nest before the walk refuses it.
///
/// Messages, groups and oneofs nest, and so does the walk that reads
/// them. JavaScript answers a runaway nesting with a catchable
/// `RangeError`; Rust answers it by running out of stack, which ABORTS
/// the process and cannot be caught. `parse` therefore refuses a document
/// past this depth before the engine builds a tree that deep, and the
/// walk refuses one it is handed directly.
///
/// The number is measured, not inherited. On the smallest stack a caller
/// is likely to have, the 1 MiB a spawned `std::thread` gets by default,
/// a DEBUG build of this crate parses a document nesting 290 messages
/// deep and ABORTS at 300; on the 2 MiB a released binary's spawned
/// thread gets, it aborts between 600 and 620. The cap is 100, which
/// leaves roughly a threefold margin on the smaller of those, matches the
/// recursion budget protoc's own parser carries, and is more than an
/// order of magnitude past anything a hand-written `.proto` nests. It
/// also bounds the parse TIME, which grows with the square of the
/// nesting depth because each level's `src` repeats every deeper level's.
///
/// `rs/tests/untrusted_test.rs` parses AT the cap and one level under it
/// as well as past it: a cap nobody tests at is a number, not a bound.
pub const MAX_NESTING_DEPTH: usize = 100;

/// The walk's own state: how deep it is, and whether it gave up.
///
/// A flag rather than a `Result` on every arm: the walk stops descending
/// the moment it is too deep, and [`build_file`] turns the flag into one
/// error for the whole document. Refusing silently would truncate the
/// descriptor, which is worse than refusing loudly.
struct Walk {
    depth: usize,
    too_deep: bool,
}

impl Walk {
    fn new() -> Self {
        Walk {
            depth: 0,
            too_deep: false,
        }
    }

    /// Enter one nesting level, reporting whether there was room.
    fn enter(&mut self) -> bool {
        if self.depth >= MAX_NESTING_DEPTH {
            self.too_deep = true;
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }
}

// ---- small helpers --------------------------------------------------------

/// `src.replace(/\s+/g, '')`, with JavaScript's whitespace set rather
/// than Unicode's: see [`is_js_whitespace`].
fn strip_ws(src: &str) -> String {
    src.chars().filter(|ch| !is_js_whitespace(*ch)).collect()
}

/// The canonical `/^["']([\s\S]*)["']$/`: any text between an opening and
/// a closing quote, which need not be the same kind.
fn unquote(text: &str) -> &str {
    let quote = |ch: char| '"' == ch || '\'' == ch;
    let mut forward = text.char_indices();
    let Some((_, first)) = forward.next() else {
        return text;
    };
    if !quote(first) {
        return text;
    }
    let Some((last_at, last)) = text.char_indices().next_back() else {
        return text;
    };
    // A single character cannot be both the opening and the closing
    // quote, which is what the two `["']` of the pattern require.
    if 0 == last_at || !quote(last) {
        return text;
    }
    &text[first.len_utf8()..last_at]
}

/// A string value as this package records it. One literal keeps the text
/// between its quotes as written; adjacent literals (`"a" "b"`) are the one
/// string protoc records for them, decoded and concatenated
/// (`strings.rs`). `bytes` is a `bytes` field's default, which protoc
/// escapes again.
fn string_value(src: &str, bytes: bool) -> String {
    adjacent_value(src, bytes).unwrap_or_else(|| unquote(src).to_string())
}

/// The canonical `/^[-+]?(?:\d|\.\d|0x|0o|0b)/i`, ASCII throughout.
fn numeric_lead(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 0;
    if at < bytes.len() && (b'-' == bytes[at] || b'+' == bytes[at]) {
        at += 1;
    }
    let Some(&first) = bytes.get(at) else {
        return false;
    };
    if first.is_ascii_digit() {
        return true;
    }
    if b'.' == first {
        return bytes.get(at + 1).is_some_and(u8::is_ascii_digit);
    }
    if b'0' == first {
        return matches!(
            bytes.get(at + 1),
            Some(b'x' | b'X' | b'o' | b'O' | b'b' | b'B')
        );
    }
    false
}

/// `Number(src)` as the canonical walk reads a field or enum number:
/// `NaN` where the text is not a number, which the descriptor's JSON then
/// writes as `null`.
fn number_of(node: Option<&Value>) -> f64 {
    match node {
        None => 0.0,
        Some(node) => js_number(nsrc(node)).unwrap_or(f64::NAN),
    }
}

// ---- constants and option values ------------------------------------------

fn constant_value(node: &Value) -> OptionValue {
    // An aggregate (`{ a: 1 }`) is the text between its braces, as protoc
    // records it in `aggregate_value`; see `aggregate.rs`.
    if let Value::Object(entries) = node {
        if let Some(Value::String(text)) = entries.get("aggregate") {
            return OptionValue::Str(text.clone());
        }
    }
    let src = nsrc(node);
    if "true" == src {
        return OptionValue::Bool(true);
    }
    if "false" == src {
        return OptionValue::Bool(false);
    }
    if src.starts_with(['"', '\'']) {
        return OptionValue::Str(string_value(src, false));
    }
    if numeric_lead(src) {
        // `Number(s.replace(/^\+/, ''))`: stripping the sign first is what
        // makes `+0x10` sixteen, where `Number("+0x10")` is `NaN`.
        if let Some(number) = js_number(src.strip_prefix('+').unwrap_or(src)) {
            return OptionValue::Number(number);
        }
    }
    // An identifier (an enum value name, `inf`, `-nan`), a number
    // JavaScript will not read, or an aggregate on a tree that did not come
    // through this plugin's parse, so has no `aggregate`: kept verbatim.
    OptionValue::Str(src.to_string())
}

/// The option name in an `optionName "=" constant` statement.
///
/// `optionStmt` keeps `optionName` as a child node, so use it when
/// present. Inside `fieldOption` abnf inlines the leading
/// `optionNamePart`, so there the name has to be read out of `src`, as
/// everything BEFORE the trailing `"=" constant`. Anchoring at the end
/// matters: `option (file_opt1) = 1;` would otherwise find the value `1`
/// inside the name `file_opt1`.
fn option_name_of<'a>(stmt: &'a Value, value: Option<&Value>) -> &'a str {
    if let Some(named) = child(stmt, "optionName") {
        return nsrc(named);
    }
    let Some(value) = value else {
        return "";
    };
    let src = nsrc(stmt);
    let head = src.strip_suffix(';').unwrap_or(src);
    let tail = format!("={}", nsrc(value));
    if let Some(name) = head.strip_suffix(&tail) {
        return name;
    }
    match head.find(nsrc(value)) {
        None | Some(0) => "",
        Some(at) => {
            let name = &head[..at];
            name.strip_suffix('=').unwrap_or(name)
        }
    }
}

/// A field's option set with `json_name` and `default` split out: protoc
/// lifts those two out of the option set into descriptor fields.
#[derive(Default)]
struct PseudoOptions {
    options: Option<Options>,
    json_name: Option<String>,
    default_value: Option<String>,
}

/// `fieldOptions = "[" fieldOption *( "," fieldOption ) "]"`.
///
/// `bytes` says the field is a `bytes` field, whose default protoc escapes
/// again after reading it (absl::CEscape).
fn read_field_options(opts: Option<&Value>, bytes: bool) -> PseudoOptions {
    let mut out = PseudoOptions::default();
    let Some(opts) = opts else {
        return out;
    };
    let mut map = Options::new();
    for option in child_rules(opts) {
        let constant = child(option, "constant");
        let name = option_name_of(option, constant);
        let (Some(constant), false) = (constant, name.is_empty()) else {
            continue;
        };
        if "json_name" == name {
            out.json_name = Some(string_value(nsrc(constant), false));
            continue;
        }
        if "default" == name {
            out.default_value = Some(string_value(nsrc(constant), bytes));
            continue;
        }
        // The canonical `map[name] = value` writes to a bare object, and
        // assigning to `__proto__` there sets the prototype instead of
        // creating a property: with a primitive value, as every option
        // value here is, it is silently discarded. An option named
        // `__proto__` therefore does NOT reach a field's option set, and
        // this drops it for the same reason rather than by accident. An
        // `option __proto__ = ...;` STATEMENT is a different path and
        // does survive: see `option_from`.
        if "__proto__" == name {
            continue;
        }
        map.insert(name.to_string(), constant_value(constant));
    }
    if !map.is_empty() {
        out.options = Some(map);
    }
    out
}

/// The option map for the places that cannot carry `json_name` or
/// `default`: extension ranges, enum values.
fn plain_options(opts: Option<&Value>) -> Option<Options> {
    read_field_options(opts, false).options
}

/// `optionStmt = "option" optionName "=" constant ";"`.
///
/// The canonical form is a computed-key object literal, which creates an
/// own property for EVERY name, `__proto__` included, so nothing is
/// dropped here.
fn option_from(element: &Value) -> Option<(String, OptionValue)> {
    let constant = child(element, "constant")?;
    let name = option_name_of(element, Some(constant));
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), constant_value(constant)))
}

/// Merge one `option` statement into an option set, as the canonical
/// `{ ...(target || {}), ...optionFrom(el) }` does. A repeated name keeps
/// its first position and takes the later value, which is what a
/// JavaScript object spread does.
fn merge_option(target: &mut Option<Options>, element: &Value) {
    let options = target.get_or_insert_with(Options::new);
    if let Some((name, value)) = option_from(element) {
        options.insert(name, value);
    }
}

/// The subset of an option map whose names are rooted at `features`.
fn features(opts: Option<&Options>) -> Option<Options> {
    let opts = opts?;
    let out: Options = opts
        .iter()
        .filter(|(name, _)| "features" == name.as_str() || name.starts_with("features."))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

// ---- fields ---------------------------------------------------------------

fn field_label(label: Option<&Value>, version: ProtoVersion) -> (FieldLabel, bool) {
    match src_or(label) {
        "required" => (FieldLabel::Required, false),
        "repeated" => (FieldLabel::Repeated, false),
        "optional" => (FieldLabel::Optional, ProtoVersion::Proto3 == version),
        // Implicit label.
        _ => (FieldLabel::Optional, false),
    }
}

fn field_type_name(type_text: &str) -> (Option<FieldType>, Option<String>) {
    // A LEADING DOT makes the reference fully qualified, so `.int32` names
    // a type called `int32` at the root and is never the scalar `int32`.
    // protoc accepts `message int32 {}` and records `.int32` as the field's
    // type name; only an unqualified spelling may reach the scalar table.
    if !type_text.starts_with('.') {
        if let Some(scalar) = scalar_type(type_text) {
            return (Some(scalar), None);
        }
    }
    // A named reference: could be a message OR an enum, and telling them
    // apart needs symbol resolution this parser deliberately does not do.
    // protoc leaves `type` unset here too, and only fills it in once the
    // name resolves, so record `type_name` as written and nothing else.
    (None, Some(type_text.to_string()))
}

/// The field type node.
///
/// Normally a `fieldType` child; but when `fieldType` is a leading ref
/// (an unlabelled oneof member `string x = 1;`) abnf inlines it, so the
/// type surfaces as a bare `messageType` or `fullIdent` child instead.
fn type_node_of(node: &Value) -> Option<&Value> {
    child(node, "fieldType")
        .or_else(|| child(node, "messageType"))
        .or_else(|| child(node, "fullIdent"))
}

fn apply_field_options(field: &mut FieldDescriptorProto, opts: Option<&Value>) {
    let pseudo = read_field_options(opts, Some(FieldType::Bytes) == field.r#type);
    if let Some(json_name) = pseudo.json_name {
        field.json_name = Some(json_name);
    }
    if let Some(default_value) = pseudo.default_value {
        field.default_value = Some(default_value);
    }
    if let Some(options) = pseudo.options {
        field.options = Some(options);
    }
}

fn build_field(node: &Value, version: ProtoVersion) -> FieldDescriptorProto {
    let (label, proto3_optional) = field_label(child(node, "label"), version);
    let (field_type, type_name) = field_type_name(src_or(type_node_of(node)));
    let mut field = FieldDescriptorProto {
        name: src_or(child(node, "ident")).to_string(),
        number: number_of(child(node, "fieldNumber")),
        label: Some(label),
        r#type: field_type,
        type_name,
        proto3_optional,
        ..FieldDescriptorProto::default()
    };
    apply_field_options(&mut field, child(node, "fieldOptions"));
    field
}

/// A group is a field plus an implicit nested message. protoc lowercases
/// the field name and keeps the declared name for the message and for
/// `type_name`:
///
/// ```text
/// optional group TestGroup = 1 { ... }
///   -> field { name: "testgroup", type: TYPE_GROUP, typeName: "TestGroup" }
///   -> nestedType { name: "TestGroup", ... }
/// ```
fn build_group(
    node: &Value,
    version: ProtoVersion,
    into: &mut DescriptorProto,
    walk: &mut Walk,
) -> FieldDescriptorProto {
    let group_name = src_or(child(node, "ident")).to_string();
    let nested = message_from_body(
        group_name.clone(),
        child(node, "messageBody"),
        version,
        walk,
    );
    into.nested_type.push(nested);

    let (label, proto3_optional) = field_label(child(node, "label"), version);
    let mut field = FieldDescriptorProto {
        // `String.prototype.toLowerCase` is Unicode-aware, and so is
        // this: a group name is an identifier token, which the lexer does
        // not restrict to ASCII.
        name: group_name.to_lowercase(),
        number: number_of(child(node, "fieldNumber")),
        label: Some(label),
        r#type: Some(FieldType::Group),
        type_name: Some(group_name),
        proto3_optional,
        ..FieldDescriptorProto::default()
    };
    apply_field_options(&mut field, child(node, "fieldOptions"));
    field
}

/// A group is the one construct with BOTH a field number and a message
/// body; `message` has a body but no number, `map` and a plain field a
/// number but no body.
fn is_group(node: &Value) -> bool {
    child(node, "fieldNumber").is_some() && child(node, "messageBody").is_some()
}

/// protoc's map-entry name: strip `_`, upper-case the letter that follows
/// (and the first letter), then append `Entry`, so `map_field` becomes
/// `MapFieldEntry`.
fn map_entry_name(field_name: &str) -> String {
    let mut out = String::with_capacity(field_name.len() + 5);
    let mut cap_next = true;
    // `for (const ch of name)` iterates code points, as this does.
    for ch in field_name.chars() {
        if '_' == ch {
            cap_next = true;
            continue;
        }
        if cap_next && ch.is_ascii_lowercase() {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push(ch);
        }
        cap_next = false;
    }
    out.push_str("Entry");
    out
}

/// `map<K,V> name = N;` becomes a repeated message field whose type is a
/// synthesised nested `<Name>Entry` message with `mapEntry = true`.
fn build_map_field(node: &Value, into: &mut DescriptorProto) -> FieldDescriptorProto {
    let types = children(node, "fieldType");
    let key_text = types.first().map_or("", |node| nsrc(node));
    let value_text = types.get(1).map_or("", |node| nsrc(node));
    let field_name = src_or(child(node, "ident")).to_string();
    let entry_name = map_entry_name(&field_name);

    let (key_type, key_type_name) = field_type_name(key_text);
    let (value_type, value_type_name) = field_type_name(value_text);
    let mut entry = DescriptorProto::new(entry_name.clone());
    entry.field.push(FieldDescriptorProto {
        name: "key".to_string(),
        number: 1.0,
        label: Some(FieldLabel::Optional),
        r#type: key_type,
        type_name: key_type_name,
        ..FieldDescriptorProto::default()
    });
    entry.field.push(FieldDescriptorProto {
        name: "value".to_string(),
        number: 2.0,
        label: Some(FieldLabel::Optional),
        r#type: value_type,
        type_name: value_type_name,
        ..FieldDescriptorProto::default()
    });
    let mut entry_options = Options::new();
    entry_options.insert("mapEntry".to_string(), OptionValue::Bool(true));
    entry.options = Some(entry_options);

    let mut field = FieldDescriptorProto {
        name: field_name,
        number: number_of(child(node, "fieldNumber")),
        label: Some(FieldLabel::Repeated),
        type_name: Some(entry_name),
        ..FieldDescriptorProto::default()
    };
    apply_field_options(&mut field, child(node, "fieldOptions"));

    // `features` on a map field govern the synthesised entry's key and
    // value fields too, so protoc copies them down. Nothing else is
    // propagated.
    if let Some(feature_options) = features(field.options.as_ref()) {
        entry.field[0].options = Some(feature_options.clone());
        entry.field[1].options = Some(feature_options);
    }
    into.nested_type.push(entry);
    field
}

// ---- enums ----------------------------------------------------------------

fn build_enum(node: &Value) -> EnumDescriptorProto {
    let mut out = EnumDescriptorProto::new(src_or(child(node, "ident")));
    for element in children(node, "enumElement") {
        // An enum body is the one place where the statement kind CANNOT
        // come from the leading keyword: `enumField` inlines its name, so
        // `kw` on `optionX = 1;` is `optionX=` and a prefix test reads it
        // as an option statement. The first child's rule name says what
        // the statement is without guessing: `optionName` for an option,
        // `ranges` or `fieldNames` for a `reserved`, `fieldNumber` for a
        // value.
        let kids = child_rules(element);
        let Some(first) = kids.first() else {
            continue;
        };
        match nrule(first) {
            "ranges" | "fieldNames" => {
                // Enum reserved ranges are INCLUSIVE and span the whole
                // int32 space.
                add_reserved(
                    element,
                    &mut out.reserved_range,
                    &mut out.reserved_name,
                    RangeOpts {
                        exclusive: false,
                        max: MAX_ENUM_NUMBER,
                    },
                );
                continue;
            }
            "optionName" => {
                merge_option(&mut out.options, element);
                continue;
            }
            "fieldNumber" => {}
            _ => continue,
        }
        // `enumField: ident "=" ["-"] fieldNumber`. Both the name and the
        // sign are in the text ahead of the number node, and nowhere else:
        // reading the whole element instead takes an OPTION's minus sign
        // as the value's, so `A = 1 [(x) = -2]` became -1.
        let gap = gaps_before(element, "fieldNumber")
            .first()
            .copied()
            .unwrap_or("");
        let name = strip_from_equals(gap);
        if name.is_empty() {
            continue;
        }
        let value = number_of(Some(first));
        out.value.push(EnumValueDescriptorProto {
            name: name.to_string(),
            number: if gap.ends_with('-') { -value } else { value },
            options: plain_options(child(element, "fieldOptions")),
        });
    }
    out
}

/// The canonical `k.replace(/=.*$/, '')`, which is subtler than it looks:
/// `.` does not match a line terminator and `$` is the end of the whole
/// string, so the match starts at the FIRST `=` with no line terminator
/// after it, and a string with no such `=` is returned unchanged.
fn strip_from_equals(text: &str) -> &str {
    let terminator = |ch: char| matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    for (at, ch) in text.char_indices() {
        if '=' == ch && !text[at..].contains(terminator) {
            return &text[..at];
        }
    }
    text
}

// ---- reserved and extension ranges ----------------------------------------

/// How a range's `end` is written down: message extension and reserved
/// ranges are half-open (end exclusive), enum reserved ranges closed.
#[derive(Clone, Copy)]
struct RangeOpts {
    exclusive: bool,
    max: f64,
}

/// `ranges = range *( "," range )`. The leading `range` is inlined into
/// `ranges.src`, so parse the whitespace-stripped text rather than kids.
fn ranges(node: Option<&Value>, opts: RangeOpts) -> Vec<DescriptorRange> {
    let Some(node) = node else {
        return Vec::new();
    };
    let flat = strip_ws(nsrc(node));
    let mut out = Vec::new();
    for part in flat.split(',') {
        // The canonical `/^(-?\d+)(?:to(-?\d+|max))?$/`.
        let Some((start_text, bound)) = split_range(part) else {
            continue;
        };
        let start = js_number(start_text).unwrap_or(f64::NAN);
        let end = match bound {
            None => {
                if opts.exclusive {
                    start + 1.0
                } else {
                    start
                }
            }
            Some("max") => opts.max,
            Some(text) => {
                let value = js_number(text).unwrap_or(f64::NAN);
                if opts.exclusive {
                    value + 1.0
                } else {
                    value
                }
            }
        };
        out.push(DescriptorRange::new(start, end));
    }
    out
}

/// `^(-?\d+)(?:to(-?\d+|max))?$` as (start, bound).
fn split_range(part: &str) -> Option<(&str, Option<&str>)> {
    let bytes = part.as_bytes();
    let mut at = 0;
    if at < bytes.len() && b'-' == bytes[at] {
        at += 1;
    }
    let digits_from = at;
    while at < bytes.len() && bytes[at].is_ascii_digit() {
        at += 1;
    }
    if digits_from == at {
        return None;
    }
    let start = &part[..at];
    if at == bytes.len() {
        return Some((start, None));
    }
    let rest = part[at..].strip_prefix("to")?;
    if "max" == rest {
        return Some((start, Some("max")));
    }
    let bytes = rest.as_bytes();
    let mut at = 0;
    if at < bytes.len() && b'-' == bytes[at] {
        at += 1;
    }
    let digits_from = at;
    while at < bytes.len() && bytes[at].is_ascii_digit() {
        at += 1;
    }
    if digits_from == at || at != bytes.len() {
        return None;
    }
    Some((start, Some(rest)))
}

/// The reserved-name list.
///
/// Both the leading `strLit` or `ident` and (for a single-item list) the
/// whole list can be inlined into `src`, so read the names out of the
/// statement text; whole-word tokens make that unambiguous. A name is one
/// string, one identifier, or adjacent strings, which protoc reads as the
/// one name they concatenate to (`reserved "a" "b";` is `ab`).
///
/// The canonical pattern is
/// `/((?:"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')+)|([A-Za-z_][A-Za-z0-9_]*)/g`,
/// scanned left to right. `\\.` does not cross a line terminator, and the
/// identifier class is ASCII, so both are written out rather than handed
/// to a Unicode-aware regexp engine.
fn reserved_names(node: &Value) -> Vec<String> {
    let src = nsrc(node);
    let body = src.strip_prefix("reserved").unwrap_or(src);
    let body = body.strip_suffix(';').unwrap_or(body);

    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        // `(?:"..."|'...')+`: as many literals as follow one another. The
        // repetition is greedy and nothing follows it in its alternative,
        // so the longest run is the match.
        let mut end = at;
        while let Some((_, next)) =
            scan_quoted(body, end, b'"').or_else(|| scan_quoted(body, end, b'\''))
        {
            end = next;
        }
        if at < end {
            out.push(string_value(&body[at..end], false));
            at = end;
            continue;
        }
        if let Some((text, next)) = scan_identifier(body, at) {
            out.push(text.to_string());
            at = next;
            continue;
        }
        // No alternative matched here; the next start position is the
        // next character, as a global regexp's own scan advances.
        at += 1;
        while at < bytes.len() && !body.is_char_boundary(at) {
            at += 1;
        }
    }
    out
}

/// `"((?:[^"\\]|\\.)*)"` at `at`, as (inner text, position after the
/// closing quote).
///
/// The greedy run can only end where the next character is the closing
/// quote, so there is no backtracking to reproduce: every character the
/// run consumed is either an escape pair or a character that is neither
/// the quote nor a backslash.
fn scan_quoted(body: &str, at: usize, quote: u8) -> Option<(&str, usize)> {
    let bytes = body.as_bytes();
    if bytes.get(at) != Some(&quote) {
        return None;
    }
    let start = at + 1;
    let mut cursor = start;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte == quote {
            return Some((&body[start..cursor], cursor + 1));
        }
        if b'\\' == byte {
            // `\\.` where `.` excludes the line terminators.
            let rest = &body[cursor + 1..];
            let next = rest.chars().next()?;
            if matches!(next, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                return None;
            }
            cursor += 1 + next.len_utf8();
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && !body.is_char_boundary(cursor) {
            cursor += 1;
        }
    }
    None
}

/// `[A-Za-z_][A-Za-z0-9_]*` at `at`. ASCII, as the canonical class is.
fn scan_identifier(body: &str, at: usize) -> Option<(&str, usize)> {
    let bytes = body.as_bytes();
    let first = *bytes.get(at)?;
    if !(first.is_ascii_alphabetic() || b'_' == first) {
        return None;
    }
    let mut cursor = at + 1;
    while cursor < bytes.len() && (bytes[cursor].is_ascii_alphanumeric() || b'_' == bytes[cursor]) {
        cursor += 1;
    }
    Some((&body[at..cursor], cursor))
}

fn add_reserved(
    node: &Value,
    reserved_range: &mut Option<Vec<DescriptorRange>>,
    reserved_name: &mut Option<Vec<String>>,
    opts: RangeOpts,
) {
    if let Some(node_ranges) = child(node, "ranges") {
        reserved_range
            .get_or_insert_with(Vec::new)
            .extend(ranges(Some(node_ranges), opts));
        return;
    }
    let names = reserved_names(node);
    if !names.is_empty() {
        reserved_name.get_or_insert_with(Vec::new).extend(names);
    }
}

// ---- messages -------------------------------------------------------------

fn build_message(node: &Value, version: ProtoVersion, walk: &mut Walk) -> DescriptorProto {
    // `node` is a dispatch node whose `message` alt was inlined: kids are
    // [ident, messageBody-children...] or [ident] then messageBody.
    message_from_body(
        src_or(child(node, "ident")).to_string(),
        child(node, "messageBody"),
        version,
        walk,
    )
}

fn message_from_body(
    name: String,
    body: Option<&Value>,
    version: ProtoVersion,
    walk: &mut Walk,
) -> DescriptorProto {
    let mut message = DescriptorProto::new(name);
    if !walk.enter() {
        return message;
    }
    let elements = body.map_or_else(Vec::new, |body| children(body, "messageElement"));

    // Options first: `message_set_wire_format` changes what `to max`
    // means in an extension or reserved range, and protoc applies it
    // wherever the option sits in the body.
    for element in &elements {
        if is_option_stmt(element) {
            merge_option(&mut message.options, element);
        }
    }
    let opts = RangeOpts {
        exclusive: true,
        max: if is_message_set(&message) {
            MAX_MESSAGE_SET_END
        } else {
            MAX_FIELD_NUMBER_END
        },
    };

    for element in &elements {
        if !is_option_stmt(element) {
            add_message_element(element, version, &mut message, opts, walk);
        }
    }
    generate_synthetic_oneofs(&mut message);
    walk.leave();
    message
}

fn is_message_set(message: &DescriptorProto) -> bool {
    matches!(
        message
            .options
            .as_ref()
            .and_then(|options| options.get("message_set_wire_format")),
        Some(OptionValue::Bool(true))
    )
}

/// An `option` statement, as opposed to an `optional`-labelled field,
/// whose `kw` is empty because the `label` node starts `src`.
fn is_option_stmt(element: &Value) -> bool {
    kw(element).starts_with("option")
}

/// protoc synthesises a single-field oneof for every proto3 explicit
/// `optional` field, appended after the declared oneofs. The name is the
/// field name prefixed with `_`, then prefixed with `X` until unique.
fn generate_synthetic_oneofs(message: &mut DescriptorProto) {
    let mut names: Vec<String> = Vec::new();
    for field in &message.field {
        names.push(field.name.clone());
    }
    for oneof in &message.oneof_decl {
        names.push(oneof.name.clone());
    }
    for index in 0..message.field.len() {
        if !message.field[index].proto3_optional || message.field[index].oneof_index.is_some() {
            continue;
        }
        let mut oneof_name = format!("_{}", message.field[index].name);
        while names.contains(&oneof_name) {
            oneof_name = format!("X{oneof_name}");
        }
        names.push(oneof_name.clone());
        message.field[index].oneof_index = Some(message.oneof_decl.len());
        message.oneof_decl.push(OneofDescriptorProto {
            name: oneof_name,
            options: None,
        });
    }
}

fn add_message_element(
    element: &Value,
    version: ProtoVersion,
    message: &mut DescriptorProto,
    opts: RangeOpts,
    walk: &mut Walk,
) {
    let keyword = kw(element);
    let first = child_rules(element).first().copied();
    if keyword.starts_with("map<") {
        let field = build_map_field(element, message);
        message.field.push(field);
        return;
    }
    if keyword.starts_with("oneof") {
        add_oneof(element, version, message, walk);
        return;
    }
    if keyword.starts_with("export") || keyword.starts_with("local") {
        // edition 2024 symbol visibility wraps the message or enum as a
        // child node.
        let (messages, enums) = (&mut message.nested_type, &mut message.enum_type);
        add_visible(element, keyword, version, messages, enums, walk);
        return;
    }
    if is_group(element) {
        let field = build_group(element, version, message, walk);
        message.field.push(field);
        return;
    }
    if keyword.starts_with("message") {
        let nested = build_message(element, version, walk);
        message.nested_type.push(nested);
        return;
    }
    if keyword.starts_with("enum") {
        message.enum_type.push(build_enum(element));
        return;
    }
    if keyword.starts_with("reserved") {
        add_reserved(
            element,
            &mut message.reserved_range,
            &mut message.reserved_name,
            opts,
        );
        return;
    }
    if keyword.starts_with("extensions") {
        let extension_options = plain_options(child(element, "fieldOptions"));
        let mut rs = ranges(child(element, "ranges"), opts);
        // A compound `extensions 2, 9 to 11 [(i) = 5];` puts the options
        // on every range, as protoc does.
        if let Some(extension_options) = extension_options {
            for range in &mut rs {
                range.options = Some(extension_options.clone());
            }
        }
        message
            .extension_range
            .get_or_insert_with(Vec::new)
            .extend(rs);
        return;
    }
    if keyword.starts_with("extend") {
        add_extend(element, version, &mut message.extension);
        return;
    }
    if ";" == nsrc(element) {
        return; // emptyStmt
    }
    // No keyword and a fieldType or label lead: a field.
    if let Some(first) = first {
        if "fieldType" == nrule(first) || "label" == nrule(first) {
            message.field.push(build_field(element, version));
        }
    }
}

/// edition 2024 `export` or `local` on a message or enum declaration. The
/// wrapped `message` or `enumDef` stays a child node instead of inlining.
fn add_visible(
    element: &Value,
    keyword: &str,
    version: ProtoVersion,
    messages: &mut Vec<DescriptorProto>,
    enums: &mut Vec<EnumDescriptorProto>,
    walk: &mut Walk,
) {
    let visibility = if keyword.starts_with("export") {
        SymbolVisibility::Export
    } else {
        SymbolVisibility::Local
    };
    if let Some(node) = child(element, "message") {
        let mut built = build_message(node, version, walk);
        built.visibility = Some(visibility);
        messages.push(built);
    } else if let Some(node) = child(element, "enumDef") {
        let mut built = build_enum(node);
        built.visibility = Some(visibility);
        enums.push(built);
    }
}

fn add_oneof(
    element: &Value,
    version: ProtoVersion,
    message: &mut DescriptorProto,
    walk: &mut Walk,
) {
    let name = src_or(child(element, "ident")).to_string();
    let index = message.oneof_decl.len();
    // protoc gives a oneof its own `OneofOptions`, and every other
    // declaration kind here keeps its option statements, so the oneof's
    // are recorded rather than dropped. Held aside rather than written
    // through a borrow of `oneof_decl`: the loop below builds group
    // members, which push onto the same message.
    message.oneof_decl.push(OneofDescriptorProto {
        name,
        options: None,
    });
    let mut decl_options: Option<Options> = None;
    for member in children(element, "oneofElement") {
        if kw(member).starts_with("option") {
            merge_option(&mut decl_options, member);
            continue;
        }
        if ";" == nsrc(member) {
            continue;
        }
        let mut field = if is_group(member) {
            build_group(member, version, message, walk)
        } else {
            build_field(member, version)
        };
        field.oneof_index = Some(index);
        // An explicit oneof member is not proto3-optional.
        field.proto3_optional = false;
        message.field.push(field);
    }
    message.oneof_decl[index].options = decl_options;
}

fn add_extend(element: &Value, version: ProtoVersion, into: &mut Vec<FieldDescriptorProto>) {
    // `extend messageType "{" *field "}"`, whose fields inline as
    // messageElement-like nodes.
    let extendee = child(element, "messageType").map(|node| nsrc(node).to_string());
    for node in child_rules(element) {
        let rule = nrule(node);
        if "field" != rule && "messageElement" != rule {
            continue;
        }
        let mut field = build_field(node, version);
        field.extendee = extendee.clone();
        into.push(field);
    }
}

// ---- services -------------------------------------------------------------

fn build_service(node: &Value) -> ServiceDescriptorProto {
    let mut service = ServiceDescriptorProto {
        name: src_or(child(node, "ident")).to_string(),
        method: Vec::new(),
        options: None,
    };
    for element in children(node, "serviceElement") {
        if kw(element).starts_with("rpc") {
            service.method.push(build_rpc(element));
        } else if is_option_stmt(element) {
            merge_option(&mut service.options, element);
        }
    }
    service
}

/// `rpc ident "(" ["stream"] messageType ")" "returns"
/// "(" ["stream"] messageType ")"`.
fn build_rpc(element: &Value) -> MethodDescriptorProto {
    let ids = children(element, "ident");
    let types = children(element, "messageType");
    let mut method = MethodDescriptorProto {
        name: ids
            .first()
            .map_or(String::new(), |node| nsrc(node).to_string()),
        input_type: types
            .first()
            .map_or(String::new(), |node| nsrc(node).to_string()),
        output_type: types
            .get(1)
            .map_or(String::new(), |node| nsrc(node).to_string()),
        client_streaming: false,
        server_streaming: false,
        options: None,
    };
    // `stream` is a bare terminal, so it never becomes a node, but it sits
    // immediately ahead of the type it modifies, which is exactly what the
    // gap holds. Searching the statement for `(stream` instead marks an
    // ordinary type whose name merely BEGINS with those letters, so
    // `rpc M (streaming.Request)` came back client-streaming.
    let modifiers = gaps_before(element, "messageType");
    if modifiers.first().is_some_and(|gap| gap.ends_with("stream")) {
        method.client_streaming = true;
    }
    if modifiers.get(1).is_some_and(|gap| gap.ends_with("stream")) {
        method.server_streaming = true;
    }
    for option in children(element, "optionStmt") {
        merge_option(&mut method.options, option);
    }
    method
}

// ---- file -----------------------------------------------------------------

/// Turn a parsed `proto` CST root into a `FileDescriptorProto` for an
/// already-resolved version.
///
/// Most callers want [`crate::to_descriptor`] or [`crate::parse`], which
/// resolve the version first.
///
/// Fails only on a document nesting deeper than [`MAX_NESTING_DEPTH`];
/// see that constant for why a Rust port needs a bound the canonical
/// runtime does not.
pub fn build_file(proto: &Value, version: ProtoVersion) -> Result<FileDescriptorProto, ProtoError> {
    let mut walk = Walk::new();
    let mut file = FileDescriptorProto::default();
    if is_edition(version) {
        file.edition = Some(edition_enum(version));
        file.syntax = Some("editions".to_string());
    } else {
        file.syntax = Some(version.as_str().to_string());
    }

    for def in children(proto, "topLevelDef") {
        let keyword = kw(def);
        if keyword.starts_with("package") {
            if let Some(node) = child(def, "fullIdent") {
                file.package = Some(nsrc(node).to_string());
            }
        } else if keyword.starts_with("import") {
            // The file named, from one literal or from adjacent ones.
            let named: String = children(def, "strLit").into_iter().map(nsrc).collect();
            if !named.is_empty() {
                let target = string_value(&named, false);
                // `import option "x";` (edition 2024) is a separate
                // dependency list.
                if keyword.contains("option") {
                    file.option_dependency
                        .get_or_insert_with(Vec::new)
                        .push(target);
                } else {
                    let index = file.dependency.len();
                    file.dependency.push(target);
                    if keyword.contains("public") {
                        file.public_dependency.push(index);
                    }
                    if keyword.contains("weak") {
                        file.weak_dependency.push(index);
                    }
                }
            }
        } else if is_option_stmt(def) {
            merge_option(&mut file.options, def);
        } else if keyword.starts_with("export") || keyword.starts_with("local") {
            let (messages, enums) = (&mut file.message_type, &mut file.enum_type);
            add_visible(def, keyword, version, messages, enums, &mut walk);
        } else if keyword.starts_with("message") {
            let built = build_message(def, version, &mut walk);
            file.message_type.push(built);
        } else if keyword.starts_with("enum") {
            file.enum_type.push(build_enum(def));
        } else if keyword.starts_with("service") {
            file.service.push(build_service(def));
        } else if keyword.starts_with("extend") {
            add_extend(def, version, &mut file.extension);
        }
    }

    if walk.too_deep {
        return Err(ProtoError::TooDeep(format!(
            "proto: document nests deeper than {MAX_NESTING_DEPTH} levels"
        )));
    }
    Ok(file)
}

/// The nesting depth of a `.proto` source: its braces, and the angle
/// brackets that nest a message inside an aggregate value, counted over the
/// tokens the engine's lexer cuts from it.
///
/// `parse` uses it to refuse a runaway document BEFORE the engine builds
/// a tree that deep, because a `tabnas::Value` drops recursively and a
/// Rust stack overflow aborts the process rather than unwinding.
///
/// An aggregate value (`option (f) = { a < b < c: 1 > > };`) starts at a
/// brace whose token before it is `=`, the only place the grammar takes
/// one. Inside it text format writes a message as `{ ... }` or `< ... >`,
/// and both nest, so both count. Outside one an angle bracket belongs to a
/// `map<K, V>` field, which nests nothing, and is not counted.
///
/// The count has to see what the parse sees: a string or a comment hides
/// what it holds, and nothing else does. Until 0.5.1 a byte scan counted
/// on its own, and it lost its place at a backtick string, at a quote
/// inside a word and at a line comment ended by a bare CR. It then refused
/// documents the engine reads without nesting, or passed documents nesting
/// far past the cap. [`scan_depth`] now reads the bytes by the lexer's own
/// rules and answers for nearly every document. Where it meets something
/// those rules do not settle byte by byte, it hands the source to
/// [`lexed_depth`], which asks the engine's lexer and is exact, but costs
/// a large part of what the parse itself costs.
///
/// `options` gives the options of the parser that will read the source,
/// called only when the lexer is needed.
pub(crate) fn nesting_depth(src: &str, options: impl FnOnce() -> tabnas::Options) -> usize {
    scan_depth(src.as_bytes()).unwrap_or_else(|| lexed_depth(src, options()))
}

/// The count [`scan_depth`] and [`lexed_depth`] both keep, one token at a
/// time, leaving out space and comments.
#[derive(Default)]
struct Depth {
    depth: usize,
    deepest: usize,
    /// The depth outside the aggregate value the count is in, if it is in
    /// one.
    aggregate: Option<usize>,
    /// Whether the last token was `=`.
    after_equals: bool,
}

impl Depth {
    /// A token, by its text.
    fn token(&mut self, text: &[u8]) {
        match text {
            b"{" => self.open(),
            b"<" if self.aggregate.is_some() => self.open(),
            b"}" => self.close(),
            b">" if self.aggregate.is_some() => self.close(),
            _ => {}
        }
        self.after_equals = b"=" == text;
    }

    fn open(&mut self) {
        if self.aggregate.is_none() && self.after_equals {
            self.aggregate = Some(self.depth);
        }
        self.depth += 1;
        self.deepest = self.deepest.max(self.depth);
    }

    fn close(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        if Some(self.depth) == self.aggregate {
            self.aggregate = None;
        }
    }
}

/// [`nesting_depth`] read from the bytes by the rules the engine's lexer
/// follows, or `None` where those rules do not settle the reading.
///
/// Space is a space, a tab, a CR or an LF, and nothing else. `#` and `//`
/// open a comment anywhere outside a string, inside a word too, and it
/// ends at the first CR or LF; `/*` opens one that ends at the first `*/`.
/// Each of `{}[]:,;=()<>` is a token of its own, and so is each of `-.+`
/// except inside a number. A `"` or a `'` opens a string where a token
/// starts, and inside a word is part of the word, as every other byte is.
///
/// Where a token starts is the one thing bytes alone do not always say: a
/// word the grammar spells as a keyword ends before a quote (`message"x"`
/// is a keyword and a string), any other word runs on through one, and a
/// number decides for itself where it ends. So a quote opens a string here
/// only at the start of the source, after space or a token from the first
/// set, or where a comment or a string ends, and anywhere else the source
/// goes to the lexer. So does a backtick, a raw control character inside a
/// string, which the lexer refuses, and a string or block comment left
/// open.
///
/// It does not check a string's escapes. Where the lexer refuses one, the
/// parse stops at that string, and a count that goes on past it only
/// counts more than the parse can build.
fn scan_depth(src: &[u8]) -> Option<usize> {
    let mut count = Depth::default();
    let mut at = 0;
    // Whether a quote at `at` opens a string.
    let mut token_start = true;
    while let Some(&byte) = src.get(at) {
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' => {
                at += 1;
                token_start = true;
            }
            b'#' => {
                at = line_end(src, at);
                token_start = true;
            }
            b'/' if Some(&b'/') == src.get(at + 1) => {
                at = line_end(src, at);
                token_start = true;
            }
            b'/' if Some(&b'*') == src.get(at + 1) => {
                let close = src
                    .get(at + 2..)?
                    .windows(2)
                    .position(|pair| b"*/" == pair)?;
                at += 2 + close + 2;
                token_start = true;
            }
            b'"' | b'\'' if token_start => {
                at = string_end(src, at)?;
                count.token(b"\"");
            }
            b'"' | b'\'' | b'`' => return None,
            b'{' | b'}' | b'[' | b']' | b':' | b',' | b';' | b'=' | b'(' | b')' | b'<' | b'>' => {
                count.token(&src[at..=at]);
                at += 1;
                token_start = true;
            }
            _ => {
                count.token(&src[at..=at]);
                at += 1;
                token_start = false;
            }
        }
    }
    Some(count.deepest)
}

/// The index of the first CR or LF at or after `at`, or the end of the
/// source: where the lexer ends a `#` or `//` comment.
fn line_end(src: &[u8], at: usize) -> usize {
    src[at..]
        .iter()
        .position(|&byte| b'\r' == byte || b'\n' == byte)
        .map_or(src.len(), |offset| at + offset)
}

/// The index after the `"` or `'` string that opens at `at`, or `None`
/// where it is left open or holds a raw control character. A backslash
/// takes the byte after it, whatever it is, as the lexer's does.
fn string_end(src: &[u8], at: usize) -> Option<usize> {
    let quote = src[at];
    let mut index = at + 1;
    loop {
        let byte = *src.get(index)?;
        if quote == byte {
            return Some(index + 1);
        }
        if byte < 0x20 {
            return None;
        }
        index += if b'\\' == byte { 2 } else { 1 };
    }
}

/// [`nesting_depth`] over the tokens the engine's lexer cuts with
/// `options`: exact, for what [`scan_depth`] hands over.
///
/// It runs without the plugin's two lexer matchers, which read the rule the
/// parser is in, and a lexer run on its own has none. Neither moves a
/// brace. The word matcher relabels a word, which holds none, and reads a
/// bracketed name, which holds only name characters, space and comments.
/// The string matcher refuses adjacent literals outside an aggregate, and
/// the parse stops there, so the count need not; with no rule to say it is
/// inside one, it would stop the count at a pair inside an aggregate too,
/// and pass the nesting after it.
///
/// The count stops at the first token the lexer refuses: the engine
/// refuses the source there too, and builds nothing past it.
fn lexed_depth(src: &str, mut options: tabnas::Options) -> usize {
    options
        .lex
        .matchers
        .retain(|name, _| crate::AGGREGATE_WORD != name && crate::ADJACENT_STRINGS != name);
    let mut lexer = Lexer::new(src, options);
    let mut count = Depth::default();
    while let Ok(token) = lexer.next_raw_token() {
        match &*token.name {
            "#SP" | "#LN" | "#CM" => {}
            // The end, or a token the lexer refuses and does not step past.
            name if "#ZZ" == name || "#BD" == name || token.src.is_empty() => break,
            _ => count.token(token.src.as_bytes()),
        }
    }
    count.deepest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_needs_two_characters() {
        assert_eq!(unquote("\"abc\""), "abc");
        assert_eq!(unquote("'abc'"), "abc");
        // The pattern does not require the two quotes to match.
        assert_eq!(unquote("\"abc'"), "abc");
        assert_eq!(unquote("\"\""), "");
        assert_eq!(unquote("\""), "\"");
        assert_eq!(unquote("abc"), "abc");
        // A single astral character is one char and two UTF-16 units; it
        // is not a quote either way.
        assert_eq!(unquote("\"\u{1F600}"), "\"\u{1F600}");
        assert_eq!(unquote("\"\u{1F600}\""), "\u{1F600}");
    }

    #[test]
    fn strip_from_equals_stops_at_a_line_terminator() {
        assert_eq!(strip_from_equals("A="), "A");
        assert_eq!(strip_from_equals("A"), "A");
        // `.` does not cross a newline, so the first `=` cannot match and
        // the second one does.
        assert_eq!(strip_from_equals("a=b\nc=d"), "a=b\nc");
    }

    /// The depth the shared parser counts.
    fn nesting_depth(src: &str) -> usize {
        super::nesting_depth(src, || crate::shared().config())
    }

    /// The depth the shared parser's lexer counts: what the scan must
    /// agree with wherever it answers.
    fn lexed(src: &str) -> usize {
        lexed_depth(src, crate::shared().config())
    }

    /// Does the shared parser's lexer read `src` to the end, refusing
    /// nothing on the way?
    fn lexes_clean(src: &str) -> bool {
        let mut options = crate::shared().config();
        options.lex.matchers.clear();
        let mut lexer = Lexer::new(src, options);
        loop {
            match lexer.next_raw_token() {
                Ok(token) if "#ZZ" == &*token.name => return true,
                Ok(token) if "#BD" == &*token.name || token.src.is_empty() => return false,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
    }

    #[test]
    fn nesting_depth_skips_strings_and_comments() {
        assert_eq!(nesting_depth("message M { message N { } }"), 2);
        assert_eq!(nesting_depth("option a = \"{{{{\";"), 0);
        assert_eq!(nesting_depth("// {{{{\nmessage M {}"), 1);
        assert_eq!(nesting_depth("/* {{{{ */ message M {}"), 1);
        assert_eq!(nesting_depth("# {{{{\nmessage M {}"), 1);
        assert_eq!(nesting_depth("}}}}"), 0);
    }

    #[test]
    fn nesting_depth_counts_angle_brackets_inside_an_aggregate_only() {
        // A map field's angle brackets nest nothing.
        assert_eq!(nesting_depth("message M { map<string, int32> m = 1; }"), 1);
        // Inside an aggregate they nest a message, as braces do.
        assert_eq!(nesting_depth("option (f) = { a < b < c: 1 > > };"), 3);
        assert_eq!(
            nesting_depth("option (f) = /* x */\n{ a { b < c: 1 > } };"),
            3
        );
        assert_eq!(nesting_depth("int32 a = 1 [(f) = { a: [< b: 1 >] }];"), 2);
        // The aggregate ends at its own brace, and the map after it is a
        // map again.
        assert_eq!(
            nesting_depth("message M { option (f) = { a < b: 1 > }; map<K, V> m = 1; }"),
            3
        );
        assert_eq!(nesting_depth("option (f) = { a: \"<<<<\" };"), 1);
        // Unclosed, every opening bracket counts.
        assert_eq!(nesting_depth("option (f) = { a < a < a <"), 4);
    }

    #[test]
    fn nesting_depth_reads_the_source_as_the_lexer_does() {
        // A backtick string is a string to the lexer, `<` and all.
        assert_eq!(nesting_depth("option (f) = { a: `<<<<` };"), 1);
        assert_eq!(nesting_depth("option (f) = `{{{{`;"), 0);
        // A quote inside a word is part of the word, so what follows it is
        // not a string: the `<` after it is counted, and a real string
        // later on still hides its own.
        assert_eq!(nesting_depth("message A\"B { option (x) = \"={<<\"; }"), 1);
        assert_eq!(
            nesting_depth("option (f) = { a: x'y b < c: \"'<<\" > };"),
            2
        );
        assert_eq!(
            nesting_depth("message A\"B { message C { message D { } } }"),
            3
        );
        // A word the grammar spells as a keyword ends before a quote.
        assert_eq!(nesting_depth("message\"{{\" M { }"), 1);
        // A line comment ends at a bare CR, as it does at an LF.
        assert_eq!(nesting_depth("// c\rmessage M { message N { } }"), 2);
        assert_eq!(nesting_depth("# c\rmessage M { }"), 1);
        // The count stops where the lexer refuses the source, as the engine
        // does: nothing past an unclosed string is built.
        assert_eq!(nesting_depth("message M { \"abc {{{{"), 1);
        // But not at adjacent literals the parse refuses outside an
        // aggregate: inside one they are text format, and nest nothing.
        assert_eq!(
            nesting_depth("option (f) = { a: \"\\e\" \"x\" b < c < d: 1 > > };"),
            3
        );
    }

    #[test]
    fn the_scan_hands_over_what_the_bytes_do_not_settle() {
        for src in [
            "option (f) = `{`;",
            "message A\"B { }",
            "message\"x\" { }",
            "option (f) = 1\"x\";",
            "option (f) = -\"x\";",
            "option (f) = \"a\tb\";",
            "option (f) = \"abc",
            "/* {",
        ] {
            assert_eq!(None, scan_depth(src.as_bytes()), "{src:?}");
        }
        for src in [
            "option (f) = \"a\\\"{\" '{' \"\"'';",
            "option (f) = { a: [\"<\"] b < c: '>' > };",
            "/* \" */ message M { } // '\r\"x\"",
        ] {
            assert_eq!(Some(lexed(src)), scan_depth(src.as_bytes()), "{src:?}");
        }
    }

    /// Wherever the scan answers, it answers what the lexer does, over
    /// sources built at random from the pieces that decide a reading.
    #[test]
    fn the_scan_agrees_with_the_lexer_wherever_it_answers() {
        const PIECES: [&str; 40] = [
            "{",
            "}",
            "<",
            ">",
            "=",
            "[",
            "]",
            ";",
            ":",
            ",",
            "(",
            ")",
            "-",
            ".",
            "+",
            "\"",
            "'",
            "`",
            "\\",
            "#",
            "//",
            "/*",
            "*/",
            "\r",
            "\n",
            " ",
            "\t",
            "\u{c}",
            "a",
            "message",
            "true",
            "1",
            "0x",
            "\u{e9}",
            "\"{\"",
            "'<'",
            "= {",
            "option (f) = {",
            "a <",
            "\"a\\\"}\"",
        ];
        // xorshift64: the same sources on every run and every machine.
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move |bound: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % bound as u64) as usize
        };
        let (mut answered, total) = (0, 3000);
        for _ in 0..total {
            let pieces = 1 + next(24);
            let src: String = (0..pieces).map(|_| PIECES[next(PIECES.len())]).collect();
            if let Some(depth) = scan_depth(src.as_bytes()) {
                answered += 1;
                // The scan does not check a string's escapes. Where the
                // lexer refuses one, the parse stops there, and the scan's
                // count past it can only add to the lexer's.
                if lexes_clean(&src) {
                    assert_eq!(lexed(&src), depth, "{src:?}");
                } else {
                    assert!(lexed(&src) <= depth, "{src:?}");
                }
            }
        }
        // Enough answered that the agreement means something.
        assert!(
            answered > total / 5,
            "the scan answered {answered} of {total}"
        );
    }
}
