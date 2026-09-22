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
    if nsrc(first).is_empty() {
        return "";
    }
    // Located by [`gaps`], not by a forward search for the child's text.
    // A forward search finds the FIRST copy, which for `message m {}` is
    // the `m` of `message` itself: `kw` came back empty and the statement
    // was dispatched as neither a message nor anything else, so the
    // declaration vanished from the descriptor. `oneof o`, `enum n`,
    // `service e` and `package e` went the same way.
    gaps(node)[0]
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

/// The source text immediately ahead of each rule child, one entry per
/// member of [`child_rules`] and in the same order.
///
/// `src` is the node's tokens run together, so every child's text is a
/// contiguous slice of it, but SEARCHING for that text can land on the
/// wrong copy. In the enum element `A1=1;` the `fieldNumber` node's `1`
/// also occurs inside the name ahead of it, and in `rpc M (stream A)`
/// the `stream` modifier is a bare terminal that never becomes a node.
/// The scan therefore runs from the END: each child is bounded above by
/// the child after it, so the last occurrence below that bound is the
/// child itself. That leaves the text between two children exactly,
/// which is where the grammar's own terminals (`=`, `-`, `(`, `stream`,
/// `returns`) are, and reading one of those is a structural question
/// answered from the tree rather than a pattern matched against the
/// whole statement.
pub fn gaps(node: &Value) -> Vec<&str> {
    let kids = child_rules(node);
    let src = nsrc(node);
    let mut at = vec![0usize; kids.len()];
    let mut hi = src.len();
    for index in (0..kids.len()).rev() {
        let text = nsrc(kids[index]);
        let found = if text.is_empty() || text.len() > hi {
            None
        } else {
            src[..hi].rfind(text)
        };
        at[index] = found.unwrap_or(hi);
        hi = at[index];
    }
    let mut out = Vec::with_capacity(kids.len());
    let mut end = 0usize;
    for (index, kid) in kids.iter().enumerate() {
        let start = at[index].max(end);
        out.push(&src[end..start]);
        end = start + nsrc(kid).len();
    }
    out
}

/// The gaps ahead of the children carrying this rule name, in source
/// order. See [`gaps`].
pub fn gaps_before<'a>(node: &'a Value, rule: &str) -> Vec<&'a str> {
    let kids = child_rules(node);
    gaps(node)
        .into_iter()
        .zip(kids)
        .filter(|(_, kid)| nrule(kid) == rule)
        .map(|(gap, _)| gap)
        .collect()
}

/// A node's `src`, or `""` when there is no node.
pub fn src_or(node: Option<&Value>) -> &str {
    node.map_or("", nsrc)
}
