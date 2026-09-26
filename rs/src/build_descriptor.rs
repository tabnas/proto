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

use tabnas::Value;

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
        return OptionValue::Str(unquote(src).to_string());
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
fn read_field_options(opts: Option<&Value>) -> PseudoOptions {
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
            out.json_name = Some(unquote(nsrc(constant)).to_string());
            continue;
        }
        if "default" == name {
            out.default_value = Some(unquote(nsrc(constant)).to_string());
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
    read_field_options(opts).options
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
    let pseudo = read_field_options(opts);
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
/// statement text; whole-word tokens make that unambiguous.
///
/// The canonical pattern is
/// `/"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)'|([A-Za-z_][A-Za-z0-9_]*)/g`,
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
        if let Some((text, next)) = scan_quoted(body, at, b'"') {
            out.push(text.to_string());
            at = next;
            continue;
        }
        if let Some((text, next)) = scan_quoted(body, at, b'\'') {
            out.push(text.to_string());
            at = next;
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

/// The contents of the first two parenthesised spans of a statement's
/// text. No identifier or type holds a parenthesis, so for an rpc these
/// are its input and its output, whatever the names in them are spelled.
/// Mirrors the TypeScript `parenthesised`.
fn parenthesised(src: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut at = 0;
    while out.len() < 2 {
        let Some(open) = src[at..].find('(').map(|i| i + at) else {
            break;
        };
        let Some(close) = src[open + 1..].find(')').map(|i| i + open + 1) else {
            break;
        };
        out.push(&src[open + 1..close]);
        at = close + 1;
    }
    out
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
    // `stream` is a bare terminal, so it never becomes a node, and the name
    // and both types are identifiers that may themselves be spelled
    // `stream` (`rpc stream (stream stream)`), so no search for a child's
    // text can say which copy is the modifier. The parentheses can: a name
    // or a type never holds one, so the first two parenthesised spans are
    // the input and the output, each the type's own text with or without
    // `stream` ahead of it. A type that merely begins with those letters
    // (`rpc M (streaming.Request)`) is its own text, and is not streaming.
    let spans = parenthesised(nsrc(element));
    if spans
        .first()
        .is_some_and(|span| *span == format!("stream{}", method.input_type))
    {
        method.client_streaming = true;
    }
    if spans
        .get(1)
        .is_some_and(|span| *span == format!("stream{}", method.output_type))
    {
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
            if let Some(node) = child(def, "strLit") {
                let target = unquote(nsrc(node)).to_string();
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

/// The nesting depth of a `.proto` source, counted in braces, skipping
/// the string literals and the comments the tabnas lexer skips.
///
/// `parse` uses it to refuse a runaway document BEFORE the engine builds
/// a tree that deep, because a `tabnas::Value` drops recursively and a
/// Rust stack overflow aborts the process rather than unwinding.
///
/// Over-counting is safe and under-counting is not, so an unterminated
/// string or comment counts every brace it contains: the engine will
/// reject that source anyway.
pub(crate) fn brace_depth(src: &str) -> usize {
    let bytes = src.as_bytes();
    let mut at = 0;
    let mut depth: usize = 0;
    let mut deepest: usize = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
                at += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                at += 1;
            }
            quote @ (b'"' | b'\'') => {
                at += 1;
                while at < bytes.len() && bytes[at] != quote {
                    at += if b'\\' == bytes[at] { 2 } else { 1 };
                }
                at += 1;
            }
            b'#' => {
                // The shared tabnas lexer reads `#` as a line comment;
                // `.proto` does not, which the leniency corpus records.
                while at < bytes.len() && b'\n' != bytes[at] {
                    at += 1;
                }
            }
            b'/' if Some(&b'/') == bytes.get(at + 1) => {
                while at < bytes.len() && b'\n' != bytes[at] {
                    at += 1;
                }
            }
            b'/' if Some(&b'*') == bytes.get(at + 1) => {
                at += 2;
                while at < bytes.len() && !(b'*' == bytes[at] && Some(&b'/') == bytes.get(at + 1)) {
                    at += 1;
                }
                at += 2;
            }
            _ => at += 1,
        }
    }
    deepest
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

    #[test]
    fn brace_depth_skips_strings_and_comments() {
        assert_eq!(brace_depth("message M { message N { } }"), 2);
        assert_eq!(brace_depth("option a = \"{{{{\";"), 0);
        assert_eq!(brace_depth("// {{{{\nmessage M {}"), 1);
        assert_eq!(brace_depth("/* {{{{ */ message M {}"), 1);
        assert_eq!(brace_depth("# {{{{\nmessage M {}"), 1);
        assert_eq!(brace_depth("}}}}"), 0);
    }
}
