/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! Walk the `{rule, src, kids}` CST into the source-ordered tree, the
//! schema `proto-ast` (alchemy's `docs/streaming-plan.md`, section 13.1):
//! the file's declarations in the order the file writes them, each node
//! an object whose first member, `type`, names it and whose other members
//! follow in the order the statement spells them.
//!
//! A scalar is the text the source spells, its tokens run together as a
//! CST node's `src` keeps them: a name, a type, a number with its sign,
//! an option's name, and a string literal with its quotes and escapes as
//! written. An aggregate option value is its fields, `[{name, value}]`,
//! each value text or an aggregate again. An empty statement declares
//! nothing and is left out.
//!
//! The transducer part (`alchemy/transduce.alc`) makes the same tree's
//! events from the parse's rule passes; this walk is the tree a host
//! reads whole, and the one the part is held to. It finds each statement
//! the way the descriptor's walk does, by the keyword ahead of the node's
//! first child ([`kw`]), since the ABNF compiler inlines a leading rule
//! into the dispatch node that holds it.
//!
//! Rust port of `ts/src/build-tree.ts`. Like the descriptor's walk, it
//! refuses a CST nesting past [`MAX_NESTING_DEPTH`] rather than run out
//! of stack, which would abort the process.

use indexmap::IndexMap;
use tabnas::Value;

use crate::build_descriptor::{
    is_group, is_option_stmt, option_name_of, parenthesised, type_text_of, MAX_NESTING_DEPTH,
};
use crate::error::ProtoError;
use crate::node::{child, child_rules, children, gaps, gaps_before, kw, nrule, nsrc, src_or};

/// A node's members, in the order they are put.
struct Node(IndexMap<String, Value>);

impl Node {
    fn of(kind: &str) -> Self {
        let mut node = Node(IndexMap::new());
        node.put("type", text(kind));
        node
    }

    fn new() -> Self {
        Node(IndexMap::new())
    }

    fn put(&mut self, name: &str, value: Value) {
        self.0.insert(name.to_string(), value);
    }

    fn value(self) -> Value {
        Value::object(self.0)
    }
}

fn text(s: &str) -> Value {
    Value::String(s.to_string())
}

fn array(items: Vec<Value>) -> Value {
    Value::Array(items.into())
}

/// The walk's depth, held to [`MAX_NESTING_DEPTH`].
struct Walk {
    depth: usize,
    too_deep: bool,
}

impl Walk {
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

/// A constant's value: its text, or an aggregate's fields.
fn constant(node: Option<&Value>, walk: &mut Walk) -> Value {
    let Some(node) = node else {
        return text("");
    };
    if !nsrc(node).starts_with('{') {
        return text(nsrc(node));
    }
    if !walk.enter() {
        return array(Vec::new());
    }
    let fields = children(node, "messageValueEntry")
        .into_iter()
        .map(|entry| aggregate_field(entry, walk))
        .collect();
    walk.leave();
    array(fields)
}

/// `messageValueEntry = ident [":"] (strLit 1*strLit / constant) [","/";"]`.
/// The leading `ident` is inlined, so the name is the text ahead of the
/// first child, less the colon; adjacent string literals are their texts
/// run together, as the entry's `src` keeps them.
fn aggregate_field(entry: &Value, walk: &mut Walk) -> Value {
    let kids = child_rules(entry);
    let gap = gaps(entry).first().copied().unwrap_or("");
    let name = gap.strip_suffix(':').unwrap_or(gap);
    let value = if 1 == kids.len() && "constant" == nrule(kids[0]) {
        constant(Some(kids[0]), walk)
    } else {
        text(&kids.iter().map(|kid| nsrc(kid)).collect::<String>())
    };
    let mut out = Node::new();
    out.put("name", text(name));
    out.put("value", value);
    out.value()
}

/// `option name = constant;`, as a statement.
fn option(element: &Value, walk: &mut Walk) -> Value {
    let value = child(element, "constant");
    let mut out = Node::of("option");
    out.put("name", text(option_name_of(element, value)));
    out.put("value", constant(value, walk));
    out.value()
}

/// `[name = constant, ...]`: a field's, an enum value's or an extension
/// range's options.
fn options(list: &Value, walk: &mut Walk) -> Value {
    array(
        children(list, "fieldOption")
            .into_iter()
            .map(|option| {
                let value = child(option, "constant");
                let mut out = Node::new();
                out.put("name", text(option_name_of(option, value)));
                out.put("value", constant(value, walk));
                out.value()
            })
            .collect(),
    )
}

/// The statement's `[options]`, where it writes them.
fn with_options(mut out: Node, element: &Value, walk: &mut Walk) -> Value {
    if let Some(list) = child(element, "fieldOptions") {
        out.put("options", options(list, walk));
    }
    out.value()
}

/// `[label] fieldType ident "=" fieldNumber [fieldOptions] ";"`.
fn field(node: &Value, walk: &mut Walk) -> Value {
    let mut out = Node::of("field");
    if let Some(label) = child(node, "label") {
        out.put("label", text(nsrc(label)));
    }
    out.put("fieldType", text(&type_text_of(node)));
    out.put("name", text(src_or(child(node, "ident"))));
    out.put("number", text(src_or(child(node, "fieldNumber"))));
    with_options(out, node, walk)
}

/// `[label] "group" ident "=" fieldNumber messageBody`.
fn group(node: &Value, walk: &mut Walk) -> Value {
    let mut out = Node::of("group");
    if let Some(label) = child(node, "label") {
        out.put("label", text(nsrc(label)));
    }
    out.put("name", text(src_or(child(node, "ident"))));
    out.put("number", text(src_or(child(node, "fieldNumber"))));
    out.put("body", message_body(child(node, "messageBody"), walk));
    out.value()
}

/// `"map" "<" fieldType "," fieldType ">" ident "=" fieldNumber
/// [fieldOptions] ";"`.
fn map(node: &Value, walk: &mut Walk) -> Value {
    let types = children(node, "fieldType");
    let mut out = Node::of("map");
    out.put("keyType", text(types.first().map_or("", |t| nsrc(t))));
    out.put("valueType", text(types.get(1).map_or("", |t| nsrc(t))));
    out.put("name", text(src_or(child(node, "ident"))));
    out.put("number", text(src_or(child(node, "fieldNumber"))));
    with_options(out, node, walk)
}

/// The ranges of a `reserved` or `extensions` statement, each `from` and,
/// where the range writes one, `to`. The leading `range` is inlined into
/// `ranges`, so they are read from its text, which no number or `max`
/// spells with a `t`.
fn ranges(node: Option<&Value>) -> Value {
    let Some(node) = node else {
        return array(Vec::new());
    };
    array(
        nsrc(node)
            .split(',')
            .map(|part| {
                let mut out = Node::new();
                match part.find("to") {
                    Some(at) => {
                        out.put("from", text(&part[..at]));
                        out.put("to", text(&part[at + 2..]));
                    }
                    None => out.put("from", text(part)),
                }
                out.value()
            })
            .collect(),
    )
}

/// The names of a `reserved` statement, each as written: a string literal
/// with its quotes, or an identifier. They are read from the statement's
/// text, cut at the commas outside a literal.
fn reserved_names(node: &Value) -> Value {
    let src = src_or(child(node, "fieldNames")).as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while at < src.len() {
        let c = src[at];
        if b'"' == c || b'\'' == c || b'`' == c {
            at += 1;
            while at < src.len() && src[at] != c {
                if b'\\' == src[at] {
                    at += 1;
                }
                at += 1;
            }
        } else if b',' == c {
            out.push(text(&String::from_utf8_lossy(&src[from..at])));
            from = at + 1;
        }
        at += 1;
    }
    out.push(text(&String::from_utf8_lossy(&src[from.min(src.len())..])));
    array(out)
}

fn reserved(node: &Value) -> Value {
    let mut out = Node::of("reserved");
    match child(node, "ranges") {
        Some(list) => out.put("ranges", ranges(Some(list))),
        None => out.put("names", reserved_names(node)),
    }
    out.value()
}

/// The edition-2024 `export` or `local` modifier keeps the message or the
/// enum as a child node, which the walk reads with the modifier ahead of
/// it.
fn visible(element: &Value, keyword: &str, walk: &mut Walk) -> Option<Value> {
    let visibility = if keyword.starts_with("export") {
        "export"
    } else {
        "local"
    };
    if let Some(message) = child(element, "message") {
        return Some(message_node(message, Some(visibility), walk));
    }
    child(element, "enumDef").map(|node| enum_node(node, Some(visibility), walk))
}

fn message_node(node: &Value, visibility: Option<&str>, walk: &mut Walk) -> Value {
    let mut out = Node::of("message");
    if let Some(visibility) = visibility {
        out.put("visibility", text(visibility));
    }
    out.put("name", text(src_or(child(node, "ident"))));
    out.put("body", message_body(child(node, "messageBody"), walk));
    out.value()
}

fn message_body(body: Option<&Value>, walk: &mut Walk) -> Value {
    let Some(body) = body else {
        return array(Vec::new());
    };
    if !walk.enter() {
        return array(Vec::new());
    }
    let out = children(body, "messageElement")
        .into_iter()
        .filter_map(|element| message_element(element, walk))
        .collect();
    walk.leave();
    array(out)
}

fn message_element(element: &Value, walk: &mut Walk) -> Option<Value> {
    let keyword = kw(element);
    if is_option_stmt(element) {
        return Some(option(element, walk));
    }
    if keyword.starts_with("map<") {
        return Some(map(element, walk));
    }
    if keyword.starts_with("oneof") {
        return Some(oneof(element, walk));
    }
    if keyword.starts_with("export") || keyword.starts_with("local") {
        return visible(element, keyword, walk);
    }
    if is_group(element) {
        return Some(group(element, walk));
    }
    if keyword.starts_with("message") {
        return Some(message_node(element, None, walk));
    }
    if keyword.starts_with("enum") {
        return Some(enum_node(element, None, walk));
    }
    if keyword.starts_with("reserved") {
        return Some(reserved(element));
    }
    if keyword.starts_with("extensions") {
        let mut out = Node::of("extensions");
        out.put("ranges", ranges(child(element, "ranges")));
        return Some(with_options(out, element, walk));
    }
    if keyword.starts_with("extend") {
        return Some(extend(element, walk));
    }
    if ";" == nsrc(element) {
        return None;
    }
    let first = child_rules(element).into_iter().next()?;
    if "fieldType" == nrule(first) || "label" == nrule(first) {
        return Some(field(element, walk));
    }
    None
}

fn oneof(element: &Value, walk: &mut Walk) -> Value {
    let mut body = Vec::new();
    if walk.enter() {
        for member in children(element, "oneofElement") {
            if is_option_stmt(member) {
                body.push(option(member, walk));
            } else if ";" == nsrc(member) {
                continue;
            } else if is_group(member) {
                body.push(group(member, walk));
            } else {
                body.push(field(member, walk));
            }
        }
        walk.leave();
    }
    let mut out = Node::of("oneof");
    out.put("name", text(src_or(child(element, "ident"))));
    out.put("body", array(body));
    out.value()
}

fn enum_node(node: &Value, visibility: Option<&str>, walk: &mut Walk) -> Value {
    let mut out = Node::of("enum");
    if let Some(visibility) = visibility {
        out.put("visibility", text(visibility));
    }
    out.put("name", text(src_or(child(node, "ident"))));
    let mut body = Vec::new();
    for element in children(node, "enumElement") {
        // The first child says what the statement is (the descriptor's
        // walk has why): `optionName` an option, `ranges` or `fieldNames` a
        // `reserved`, `fieldNumber` a value.
        let Some(first) = child_rules(element).into_iter().next() else {
            continue;
        };
        match nrule(first) {
            "ranges" | "fieldNames" => body.push(reserved(element)),
            "optionName" => body.push(option(element, walk)),
            "fieldNumber" => {
                // `ident "=" ["-"] fieldNumber`: the name and the sign are
                // in the text ahead of the number node.
                let gap = gaps_before(element, "fieldNumber")
                    .first()
                    .copied()
                    .unwrap_or("");
                let name = gap.find('=').map_or(gap, |at| &gap[..at]);
                let sign = if gap.ends_with('-') { "-" } else { "" };
                let mut value = Node::of("enumValue");
                value.put("name", text(name));
                value.put("number", text(&format!("{sign}{}", nsrc(first))));
                body.push(with_options(value, element, walk));
            }
            _ => {}
        }
    }
    out.put("body", array(body));
    out.value()
}

/// `extend messageType "{" *( field / emptyStmt ) "}"`.
fn extend(node: &Value, walk: &mut Walk) -> Value {
    let body = child_rules(node)
        .into_iter()
        .filter(|kid| "field" == nrule(kid) || "messageElement" == nrule(kid))
        .map(|kid| field(kid, walk))
        .collect();
    let mut out = Node::of("extend");
    out.put("extendee", text(src_or(child(node, "messageType"))));
    out.put("body", array(body));
    out.value()
}

fn service(node: &Value, walk: &mut Walk) -> Value {
    let mut body = Vec::new();
    for element in children(node, "serviceElement") {
        if kw(element).starts_with("rpc") {
            body.push(rpc(element, walk));
        } else if is_option_stmt(element) {
            body.push(option(element, walk));
        }
    }
    let mut out = Node::of("service");
    out.put("name", text(src_or(child(node, "ident"))));
    out.put("body", array(body));
    out.value()
}

/// `rpc ident "(" ["stream"] messageType ")" "returns" "(" ["stream"]
/// messageType ")" ( ";" / "{" *( optionStmt / emptyStmt ) "}" )`. Whether
/// each side streams is read from its parenthesised span, as the
/// descriptor's walk reads it; the block is there where the statement
/// ends in a brace.
fn rpc(element: &Value, walk: &mut Walk) -> Value {
    let types = children(element, "messageType");
    let input_type = types.first().map_or("", |t| nsrc(t));
    let output_type = types.get(1).map_or("", |t| nsrc(t));
    let spans = parenthesised(nsrc(element));
    let mut out = Node::of("rpc");
    out.put("name", text(src_or(child(element, "ident"))));
    if spans
        .first()
        .is_some_and(|span| *span == format!("stream{input_type}"))
    {
        out.put("inputStream", Value::Bool(true));
    }
    out.put("inputType", text(input_type));
    if spans
        .get(1)
        .is_some_and(|span| *span == format!("stream{output_type}"))
    {
        out.put("outputStream", Value::Bool(true));
    }
    out.put("outputType", text(output_type));
    if nsrc(element).ends_with('}') {
        let body = children(element, "optionStmt")
            .into_iter()
            .map(|stmt| option(stmt, walk))
            .collect();
        out.put("body", array(body));
    }
    out.value()
}

fn top_level(def: &Value, walk: &mut Walk) -> Option<Value> {
    let keyword = kw(def);
    if let Some(modifier) = keyword.strip_prefix("import") {
        let mut out = Node::of("import");
        if !modifier.is_empty() {
            out.put("modifier", text(modifier));
        }
        out.put("path", text(src_or(child(def, "strLit"))));
        return Some(out.value());
    }
    if keyword.starts_with("package") {
        let mut out = Node::of("package");
        out.put("name", text(src_or(child(def, "fullIdent"))));
        return Some(out.value());
    }
    if is_option_stmt(def) {
        return Some(option(def, walk));
    }
    if keyword.starts_with("export") || keyword.starts_with("local") {
        return visible(def, keyword, walk);
    }
    if keyword.starts_with("message") {
        return Some(message_node(def, None, walk));
    }
    if keyword.starts_with("enum") {
        return Some(enum_node(def, None, walk));
    }
    if keyword.starts_with("service") {
        return Some(service(def, walk));
    }
    if keyword.starts_with("extend") {
        return Some(extend(def, walk));
    }
    None
}

/// The file's tree: its declarations in source order. An empty file,
/// which parses to no node at all, is a file with an empty body.
pub(crate) fn build_tree(proto: &Value) -> Result<Value, ProtoError> {
    let mut walk = Walk {
        depth: 0,
        too_deep: false,
    };
    let mut body = Vec::new();
    for kid in child_rules(proto) {
        match nrule(kid) {
            "syntaxOrEdition" => {
                let kind = if kw(kid).starts_with("edition") {
                    "edition"
                } else {
                    "syntax"
                };
                let mut out = Node::of(kind);
                out.put("value", text(src_or(child(kid, "strLit"))));
                body.push(out.value());
            }
            "topLevelDef" => {
                if let Some(node) = top_level(kid, &mut walk) {
                    body.push(node);
                }
            }
            _ => {}
        }
    }
    if walk.too_deep {
        return Err(ProtoError::TooDeep(format!(
            "proto: the document nests past the {MAX_NESTING_DEPTH} levels this parser accepts"
        )));
    }
    let mut out = Node::of("file");
    out.put("body", array(body));
    Ok(out.value())
}
