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

use tabnas::{Context, Lexer, Rule, RuleSpec, Token, Value, TIN_TX};

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

// ---- words inside an aggregate --------------------------------------------
//
// Port of the matcher in `ts/src/aggregate.ts`, which explains it in full.
// Text format has no keywords, so inside an aggregate value this matcher,
// which [`crate::proto`] runs ahead of the grammar's own, lexes as an
// identifier (#TX):
//
// - a word the grammar spells as a keyword, always;
// - `true`, `false`, `null`, `export` and `local` where they name a field:
//   followed by `:`, `{` or `<`;
// - `[x.y]` or `[type.googleapis.com/x.Y]` followed by `:`, `{` or `<`, the
//   name between the brackets read as one, as text format reads it.
//
// Every scan below compares ASCII bytes only, and no byte of a multi-byte
// UTF-8 character is ASCII, so scanning bytes finds what the canonical
// scan of UTF-16 code units finds.

/// Every word the grammar spells as a literal, less `export` and `local`.
/// The `keywords_match_the_grammar` test keeps it in step with the grammar.
const KEYWORDS: [&str; 24] = [
    "syntax",
    "import",
    "weak",
    "public",
    "package",
    "option",
    "message",
    "required",
    "optional",
    "repeated",
    "oneof",
    "map",
    "enum",
    "service",
    "rpc",
    "stream",
    "returns",
    "extend",
    "extensions",
    "reserved",
    "to",
    "max",
    "group",
    "edition",
];

/// The grammar's punctuation, where the lexer ends a word.
const PUNCT: &[u8] = b"{}[]:,;=()<>-.+";

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || b'_' == byte
}

fn is_word_part(byte: u8) -> bool {
    is_word_start(byte) || byte.is_ascii_digit()
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// Does the lexer end a word at `src[at]`: whitespace, the grammar's
/// punctuation, a comment, or the end of the source?
fn ends_word(src: &[u8], at: usize) -> bool {
    let Some(&byte) = src.get(at) else {
        return true;
    };
    if is_space(byte) || PUNCT.contains(&byte) || b'#' == byte {
        return true;
    }
    b'/' == byte && matches!(src.get(at + 1), Some(b'/' | b'*'))
}

/// The index of the first byte at or after `at` that is neither space nor
/// inside a comment, or `src.len()`.
fn skip_space(src: &[u8], mut at: usize) -> usize {
    while at < src.len() {
        let byte = src[at];
        if is_space(byte) {
            at += 1;
        } else if b'#' == byte || (b'/' == byte && Some(&b'/') == src.get(at + 1)) {
            while at < src.len() && b'\n' != src[at] {
                at += 1;
            }
        } else if b'/' == byte && Some(&b'*') == src.get(at + 1) {
            at = src[at + 2..]
                .windows(2)
                .position(|pair| b"*/" == pair)
                .map_or(src.len(), |end| at + 2 + end + 2);
        } else {
            break;
        }
    }
    at
}

/// Does a field name end at `at`: is the next thing a `:`, `{` or `<`?
fn names_field(src: &[u8], at: usize) -> bool {
    matches!(src.get(skip_space(src, at)), Some(b':' | b'{' | b'<'))
}

/// A type name as text format checks one: identifiers joined by dots.
fn is_type_name(name: &str) -> bool {
    name.split('.').all(|part| {
        let bytes = part.as_bytes();
        !bytes.is_empty() && is_word_start(bytes[0]) && bytes[1..].iter().all(|&b| is_word_part(b))
    })
}

/// `[x.y]` or `[type.googleapis.com/x.Y]` at `open`: the index after its
/// `]` and the name with its spaces and comments left out, or `None`.
///
/// The name is checked as text format checks it
/// (`ConsumeAnyTypeUrlOrFullTypeName` in protobuf's text_format.cc): after
/// the last `/`, if there is one, a type name; before it, a prefix that
/// does not start with `/`.
fn bracket_name(src: &[u8], open: usize) -> Option<(usize, String)> {
    let mut at = open + 1;
    let mut name = String::new();
    loop {
        at = skip_space(src, at);
        match src.get(at).copied() {
            Some(byte) if is_word_part(byte) || matches!(byte, b'.' | b'/' | b'-') => {
                name.push(char::from(byte));
                at += 1;
            }
            Some(b']') => {
                let last = name.rfind('/').map_or(0, |slash| slash + 1);
                if !is_type_name(&name[last..]) || name.starts_with('/') {
                    return None;
                }
                return Some((at + 1, format!("[{name}]")));
            }
            _ => return None,
        }
    }
}

fn is_open_brace(token: Option<&Token>) -> bool {
    token.is_some_and(|token| "{" == &*token.src)
}

/// The keep prop that marks a rule inside an aggregate value.
const INSIDE: &str = "protoAggregate";

/// The before-open action [`crate::proto`] installs on every rule the
/// grammar's `constant` rule pushes: a rule whose parent is an aggregate
/// value's `constant`, the one whose first token is its `{`, is marked.
///
/// The engine copies keep props to each rule pushed below a rule and to a
/// rule that replaces one, so each rule inside the value carries the mark
/// and no rule outside does. It is set on the rules the `constant` pushes
/// rather than on the `constant` itself because the engine copies them at
/// the push, which comes before an after-open action runs. Port of
/// `markAggregate` in `ts/src/aggregate.ts`.
pub(crate) fn mark_aggregate(rule: &mut Rule, _context: &mut Context) {
    let under_aggregate = rule
        .parent_rule
        .as_deref()
        .is_some_and(|parent| "constant" == &*parent.name && is_open_brace(parent.o.first()));
    if under_aggregate {
        rule.k_mut().insert(INSIDE.to_string(), Value::Bool(true));
    }
}

/// The rules the grammar's `constant` rule pushes, which
/// [`mark_aggregate`] is installed on.
pub(crate) fn pushed_rules(constant: &RuleSpec) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in constant.open.iter().filter_map(|alt| alt.p.as_ref()) {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
}

/// Is the lexer inside an aggregate value? `lookahead` is the first token
/// the parser holds unconsumed, `context.t.first()`.
///
/// Every rule inside one carries the mark [`mark_aggregate`] sets, bar the
/// value's `constant` itself: that is inside once its first token is the
/// `{`, and while it is still choosing its alternative it peeks the tokens
/// after the brace, with the brace already in the lookahead. Each test is a
/// lookup, so the answer costs the same however deep the rule stack is.
pub(crate) fn in_aggregate(rule: &Rule, lookahead: Option<&Token>) -> bool {
    if matches!(rule.k.get(INSIDE), Some(Value::Bool(true))) {
        return true;
    }
    "constant" == &*rule.name && (is_open_brace(rule.o.first()) || is_open_brace(lookahead))
}

/// The lexer matcher [`crate::proto`] installs: an identifier token, or
/// `None` to let the grammar's own matchers read the text.
pub(crate) fn aggregate_word(
    lexer: &mut Lexer<'_>,
    rule: &mut Rule,
    context: &mut Context,
) -> Option<Token> {
    let rest = lexer.remaining();
    let src = rest.as_bytes();
    let first = *src.first()?;
    let (end, text) = if b'[' == first {
        let (end, name) = bracket_name(src, 0)?;
        if !names_field(src, end) {
            return None;
        }
        (end, name)
    } else if is_word_start(first) {
        let end = src
            .iter()
            .position(|&byte| !is_word_part(byte))
            .unwrap_or(src.len());
        if !ends_word(src, end) {
            return None;
        }
        let word = &rest[..end];
        let lower = word.to_ascii_lowercase();
        let name = matches!(word, "true" | "false" | "null")
            || matches!(lower.as_str(), "export" | "local");
        let keyword = KEYWORDS.contains(&lower.as_str());
        if !(keyword || (name && names_field(src, end))) {
            return None;
        }
        (end, word.to_string())
    } else {
        return None;
    };
    if !in_aggregate(rule, context.t.first()) {
        return None;
    }
    // A bracketed name may cross lines and hold comments, so advance by the
    // characters it spans: the lexer keeps the row and column as it goes.
    let count = rest[..end].chars().count();
    let point = lexer.point();
    if !lexer.advance_chars(count) {
        return None;
    }
    Some(lexer.token("#TX", TIN_TX, Value::String(text.clone()), text, point))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every word the grammar spells as a literal, less `export` and
    /// `local`, which the matcher takes only where they name a field.
    #[test]
    fn keywords_match_the_grammar() {
        let mut words: Vec<String> = Vec::new();
        for line in crate::GRAMMAR_TEXT.lines() {
            // A rule line ends at the first `;` outside a quoted literal.
            let mut body = String::new();
            let mut quoted = false;
            for ch in line.chars() {
                if '"' == ch {
                    quoted = !quoted;
                } else if ';' == ch && !quoted {
                    break;
                }
                body.push(ch);
            }
            // The literals sit between the odd and even quotes.
            for literal in body.split('"').skip(1).step_by(2) {
                let bytes = literal.as_bytes();
                let word = !bytes.is_empty()
                    && is_word_start(bytes[0])
                    && bytes.iter().all(|&byte| is_word_part(byte));
                let lower = literal.to_ascii_lowercase();
                if word && !words.contains(&lower) {
                    words.push(lower);
                }
            }
        }
        words.retain(|word| "export" != word && "local" != word);
        words.sort();
        let mut keywords: Vec<String> = KEYWORDS.iter().map(|word| word.to_string()).collect();
        keywords.sort();
        assert_eq!(keywords, words);
    }

    /// The answer once came from walking up the rule stack, which every
    /// top-level definition and every aggregate entry deepens, so each
    /// keyword cost more than the one before it. Port of the "telling
    /// inside an aggregate from outside" suite in `ts/test/proto.test.ts`.
    #[test]
    fn in_aggregate_asks_only_the_rule_at_hand() {
        let mut brace = Token::no_token();
        brace.src = "{".into();
        let mut constant = Rule::new("constant", Value::Undefined);
        constant.o = std::rc::Rc::new(vec![brace.clone()]);
        // The parent is an aggregate's constant, but only the mark says so.
        let mut unmarked = Rule::new("messageValueEntry", Value::Undefined);
        unmarked.parent_rule = Some(constant.snapshot());
        assert!(!in_aggregate(&unmarked, None), "walked up the rule stack");
        let mut marked = unmarked.clone();
        marked.k_mut().insert(INSIDE.to_string(), Value::Bool(true));
        assert!(in_aggregate(&marked, None));
        assert!(in_aggregate(&constant, None));
        let choosing = Rule::new("constant", Value::Undefined);
        assert!(in_aggregate(&choosing, Some(&brace)));
        assert!(!in_aggregate(&choosing, None));
    }

    #[test]
    fn bracketed_names_are_checked_as_text_format_checks_them() {
        let name = |src: &str| bracket_name(src.as_bytes(), 0).map(|(_, name)| name);
        assert_eq!(Some("[x.y]".to_string()), name("[ x . y ]"));
        assert_eq!(
            Some("[type.googleapis.com/x.Y]".to_string()),
            name("[type.googleapis.com/x.Y]")
        );
        assert_eq!(None, name("[.x.y]"));
        assert_eq!(None, name("[/x.Y]"));
        assert_eq!(None, name("[]"));
        assert_eq!(None, name("[x.y"));
    }
}
