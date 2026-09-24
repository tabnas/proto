// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! An aggregate option value: `option (foo) = { a: 1 };`.
//!
//! Port of `ts/src/aggregate.ts`, which explains protoc's rule in full.
//! In short: protoc records the source text between the braces as the
//! option's `aggregate_value`, every token, space and newline as written,
//! with each run of comments replaced by the newlines and spaces that keep
//! the next token on its line and column. A comment directly ahead of the
//! closing brace leaves nothing. Columns are protoc's: a tab moves to the
//! next multiple of 8, and every other byte of UTF-8 is one column.

use tabnas::{Context, Rule, Value};

const TAB_WIDTH: usize = 8;

/// Line and column, kept protoc's way while a scan moves through bytes.
struct Position {
    line: usize,
    col: usize,
}

impl Position {
    /// Move past `bytes[at]`, returning the index after it.
    ///
    /// Stepping a byte at a time is what makes a multi-byte character
    /// count once per byte, as protoc's tokenizer counts it.
    fn step(&mut self, bytes: &[u8], at: usize) -> usize {
        match bytes[at] {
            b'\n' => {
                self.line += 1;
                self.col = 0;
            }
            b'\t' => self.col += TAB_WIDTH - self.col % TAB_WIDTH,
            _ => self.col += 1,
        }
        at + 1
    }
}

/// The text protoc 36 records for the aggregate whose braces are at
/// `src[open]` and `src[close]`, byte offsets.
pub(crate) fn aggregate_text(src: &str, open: usize, close: usize) -> String {
    let bytes = src.as_bytes();
    let mut pos = Position { line: 0, col: 0 };

    // The column after the opening brace depends on everything ahead of it
    // on its line, because that decides where a later tab stops.
    let mut at = src[..open].rfind('\n').map_or(0, |newline| newline + 1);
    while at <= open {
        at = pos.step(bytes, at);
    }
    pos.line = 0;

    let is_comment_start =
        |at: usize| b'/' == bytes[at] && matches!(bytes.get(at + 1), Some(b'/') | Some(b'*'));

    let mut out = String::new();
    let mut from = open + 1;
    let mut at = from;
    while at < close {
        let byte = bytes[at];
        if is_comment_start(at) {
            out.push_str(&src[from..at]);
            let (gap_line, gap_col) = (pos.line, pos.col);
            // Comments with nothing between them form one gap: protoc pads
            // from the token before the first to the token after the last.
            while at < close && is_comment_start(at) {
                if b'/' == bytes[at + 1] {
                    while at < close && b'\n' != bytes[at] {
                        at = pos.step(bytes, at);
                    }
                    if at < close {
                        at = pos.step(bytes, at);
                    }
                } else {
                    at = pos.step(bytes, at);
                    at = pos.step(bytes, at);
                    while at < close && !(b'*' == bytes[at] && Some(&b'/') == bytes.get(at + 1)) {
                        at = pos.step(bytes, at);
                    }
                    if at < close {
                        at = pos.step(bytes, at);
                        at = pos.step(bytes, at);
                    }
                }
            }
            from = at;
            if close <= at {
                break;
            }
            if gap_line < pos.line {
                out.push_str(&"\n".repeat(pos.line - gap_line));
                out.push_str(&" ".repeat(pos.col));
            } else if gap_col < pos.col {
                out.push_str(&" ".repeat(pos.col - gap_col));
            }
            continue;
        }
        if b'"' == byte || b'\'' == byte {
            // A string's text is copied as written; a `//` inside one is not
            // a comment. protoc ends an unterminated string at the newline.
            at = pos.step(bytes, at);
            while at < close && byte != bytes[at] && b'\n' != bytes[at] {
                if b'\\' == bytes[at] && at + 1 < close && b'\n' != bytes[at + 1] {
                    at = pos.step(bytes, at);
                }
                at = pos.step(bytes, at);
            }
            if at < close && byte == bytes[at] {
                at = pos.step(bytes, at);
            }
            continue;
        }
        at = pos.step(bytes, at);
    }
    if from < close {
        out.push_str(&src[from..close]);
    }
    out
}

/// The after-close action [`crate::proto`] installs on the grammar's
/// `constant` rule: on an aggregate value's CST node it sets `aggregate`
/// to the text protoc records, read from the source between the value's
/// braces.
///
/// The node's `src` stays the tokens run together, as on every node. The
/// opening brace is the rule's first token and the closing brace the last
/// token consumed; a node where either is not a brace is left without
/// `aggregate`. Port of `recordAggregate` in `ts/src/aggregate.ts`.
pub(crate) fn record_aggregate(rule: &mut Rule, ctx: &mut Context) {
    let is_aggregate = matches!(
        &*rule.node.borrow(),
        Value::Object(entries)
            if matches!(entries.get("rule"), Some(Value::String(name)) if "constant" == name)
                && matches!(entries.get("src"), Some(Value::String(src)) if src.starts_with('{'))
    );
    if !is_aggregate {
        return;
    }
    let (Some(open), Some(close)) = (rule.o.first(), ctx.v1()) else {
        return;
    };
    let (open, close) = (open.site.si, close.site.si);
    let src = ctx.source.as_str();
    if !(open < close && close < src.len()) {
        return;
    }
    if b'{' != src.as_bytes()[open] || b'}' != src.as_bytes()[close] {
        return;
    }
    let text = aggregate_text(src, open, close);
    if let Some(node) = rule.node.borrow_mut().as_object_mut() {
        node.insert("aggregate".to_string(), Value::String(text));
    }
}
