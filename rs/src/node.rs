/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! CST node accessors.
//!
//! The engine hands back a `{rule, src, kids}` tree as a
//! [`tabnas::Value`]. A node that is not a user rule carries no `rule`
//! key at all, which is what the canonical `R(n)` filter tests for.

use tabnas::Value;

/// A node's rule name, or `""` for a node that is not a user rule.
pub fn nrule(node: &Value) -> &str {
    match node {
        Value::Object(entries) => match entries.get("rule") {
            Some(Value::String(rule)) => rule,
            _ => "",
        },
        _ => "",
    }
}

/// A node's accepted source text. The lexer skips whitespace and
/// comments, so this is the node's tokens run together.
pub fn nsrc(node: &Value) -> &str {
    match node {
        Value::Object(entries) => match entries.get("src") {
            Some(Value::String(src)) => src,
            _ => "",
        },
        _ => "",
    }
}

/// The children that are real rule nodes; terminals fold into `src`.
///
/// The canonical `R(n)`. An empty `.proto` is a legal (if useless) file
/// and parses to no node at all, so a missing node yields no children
/// rather than failing at the root.
pub fn child_rules(node: &Value) -> Vec<&Value> {
    let kids = match node {
        Value::Object(entries) => entries.get("kids"),
        _ => None,
    };
    match kids {
        Some(Value::Array(items)) => items.iter().filter(|kid| !nrule(kid).is_empty()).collect(),
        _ => Vec::new(),
    }
}

/// The keyword or keywords consumed before this node's first child: the
/// part of `src` ahead of the first child's `src`.
///
/// For `message Foo {...}` the first child is `Foo`, so this is
/// `message`; for an unlabelled field it is `""`.
pub fn kw(node: &Value) -> &str {
    let kids = child_rules(node);
    let src = nsrc(node);
    let Some(first) = kids.first() else {
        return src;
    };
    // `indexOf` returning -1 and returning 0 are the same answer here,
    // as they are in the canonical `i <= 0 ? '' : ...`.
    match src.find(nsrc(first)) {
        None | Some(0) => "",
        Some(at) => &src[..at],
    }
}

/// The first rule child with this rule name.
pub fn child<'a>(node: &'a Value, rule: &str) -> Option<&'a Value> {
    child_rules(node).into_iter().find(|kid| nrule(kid) == rule)
}

/// Every rule child with this rule name, in source order.
pub fn children<'a>(node: &'a Value, rule: &str) -> Vec<&'a Value> {
    child_rules(node)
        .into_iter()
        .filter(|kid| nrule(kid) == rule)
        .collect()
}

/// A node's `src`, or `""` when there is no node.
pub fn src_or(node: Option<&Value>) -> &str {
    node.map_or("", nsrc)
}
