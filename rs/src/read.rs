// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! What the parser reads as the lexer gives it its tokens, a token at a
//! time, so that nothing waits for the document's end: the nesting cap,
//! and, in a parse from a reader, the text of an aggregate option value.
//! Port of `ts/src/read.ts`, which explains both in full.
//!
//! The cap: [`read_token`] counts the braces among the tokens the lexer
//! hands the parser, ignored ones and the brace a word or a number is cut
//! with included, and turns the brace that opens a level past
//! [`MAX_NESTING_DEPTH`] into a bad token, which the parser refuses at
//! once, at that brace, with the code `proto_too_deep`. No tree deeper
//! than the cap is built, from a text or from a reader, so none drops
//! deep enough to run out of stack. [`crate::preflight`] is the same cap
//! counted over a whole text, which [`crate::parse`] and its kin still
//! make first.
//!
//! The aggregate's text: a parse from a reader holds a window of the
//! source, never the whole of it, so [`read_token`] keeps protoc's column
//! as the tokens go by, keeps the tokens' text from the brace that opens
//! an option's aggregate value (the first after an `=`) to the brace that
//! closes it, and gives each closing brace the text protoc records for the
//! value it closes, which `record_aggregate` reads.

use std::sync::Arc;

use tabnas::{Context, LexMatcher, Lexer, Rule, Token, Value, TIN_BD, TIN_CM, TIN_LN, TIN_SP};

use crate::aggregate::{interior_text, Position, AGGREGATE, AGGREGATE_OPEN};
use crate::MAX_NESTING_DEPTH;

/// The code the parser refuses a document nesting past the cap with.
pub(crate) const TOO_DEEP: &str = "proto_too_deep";

/// Its message, which [`crate::proto`] declares.
pub(crate) fn too_deep_message() -> String {
    format!("proto: document nests deeper than the {MAX_NESTING_DEPTH} levels this parser accepts")
}

/// Its hint, which [`crate::proto`] declares.
pub(crate) fn too_deep_hint() -> String {
    format!(
        "A .proto document nests at most {MAX_NESTING_DEPTH} levels of braces, counting every \
         block and every aggregate option value, the recursion budget protoc carries too. \
         This brace opens one level more."
    )
}

/// The member of a parse's `Context::u` that holds what [`read_token`] has
/// read: where the token after the last one read starts (a byte offset: a
/// token the lexer gives again is not read again), the braces open there,
/// protoc's line and column there (kept in a parse from a reader), and
/// whether the last token read that is not space, a line break or a
/// comment was `=`.
const READING: &str = "proto$read";

/// The member that holds the text from the outermost open aggregate's
/// opening brace on, in a parse from a reader.
const TEXT: &str = "proto$text";

/// The member that holds the aggregates open, outermost first, each where
/// its opening brace is, where its text starts in the text, and protoc's
/// column after its opening brace.
const OPEN: &str = "proto$open";

/// What [`read_token`] has read of one parse, as `Context::u` holds it.
struct Reading {
    next: usize,
    depth: usize,
    at: Position,
    eq: bool,
}

impl Reading {
    fn load(context: &Context) -> Self {
        let number = |index: usize| match context.u.get(READING) {
            Some(Value::Array(items)) => match items.get(index) {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            },
            _ => 0,
        };
        let eq = matches!(
            context.u.get(READING),
            Some(Value::Array(items)) if matches!(items.get(4), Some(Value::Bool(true)))
        );
        Reading {
            next: number(0),
            depth: number(1),
            at: Position {
                line: number(2),
                col: number(3),
            },
            eq,
        }
    }

    fn store(&self, context: &mut Context) {
        let items = [
            Value::Number(self.next as f64),
            Value::Number(self.depth as f64),
            Value::Number(self.at.line as f64),
            Value::Number(self.at.col as f64),
            Value::Bool(self.eq),
        ];
        match context.u.get_mut(READING) {
            Some(Value::Array(held)) => {
                let held = Arc::make_mut(held);
                held.clear();
                held.extend(items);
            }
            _ => {
                context
                    .u
                    .insert(READING.to_string(), Value::Array(Arc::new(items.to_vec())));
            }
        }
    }

    /// Move protoc's place past `src`.
    fn advance(&mut self, src: &str) {
        let bytes = src.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            at = self.at.step(bytes, at);
        }
    }
}

/// The lex subscriber [`crate::proto`] installs.
pub(crate) fn read_token(token: &mut Token, _rule: &mut Rule, context: &mut Context) {
    let mut reading = Reading::load(context);
    if token.site.si < reading.next {
        return;
    }
    reading.next = token.site.si + token.len;
    let (open, close) = (token.src.as_str() == "{", token.src.as_str() == "}");
    if open {
        if MAX_NESTING_DEPTH <= reading.depth {
            token.tin = TIN_BD;
            token.name = "#BD".into();
            token.why = TOO_DEEP.into();
            token.err = TOO_DEEP.into();
            reading.store(context);
            return;
        }
        reading.depth += 1;
    } else if close {
        reading.depth = reading.depth.saturating_sub(1);
    }
    if context.source.is_empty() {
        read_aggregate(token, context, &mut reading, open, close);
    }
    reading.store(context);
}

/// In a parse from a reader: protoc's column, and the text of the
/// aggregates open, which a closing brace takes away in its use data.
fn read_aggregate(
    token: &mut Token,
    context: &mut Context,
    reading: &mut Reading,
    open: bool,
    close: bool,
) {
    let inside = matches!(context.u.get(OPEN), Some(Value::Array(frames)) if !frames.is_empty());
    if open && (reading.eq || inside) {
        if inside {
            append(context, token.src.as_str());
        }
        reading.advance(token.src.as_str());
        let from = match context.u.get(TEXT) {
            Some(Value::String(text)) => text.len(),
            _ => 0,
        };
        let frame = Value::Array(Arc::new(vec![
            Value::Number(token.site.si as f64),
            Value::Number(from as f64),
            Value::Number(reading.at.col as f64),
        ]));
        match context.u.get_mut(OPEN) {
            Some(Value::Array(frames)) => Arc::make_mut(frames).push(frame),
            _ => {
                context
                    .u
                    .insert(OPEN.to_string(), Value::Array(Arc::new(vec![frame])));
            }
        }
    } else if close && inside {
        let frame = match context.u.get_mut(OPEN) {
            Some(Value::Array(frames)) => Arc::make_mut(frames).pop(),
            _ => None,
        };
        let field = |index: usize| match &frame {
            Some(Value::Array(items)) => match items.get(index) {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            },
            _ => 0,
        };
        let value = match context.u.get(TEXT) {
            Some(Value::String(text)) => interior_text(text, field(1), text.len(), field(2)),
            _ => String::new(),
        };
        let data = token.use_data_mut();
        data.insert(AGGREGATE.to_string(), Value::String(value));
        data.insert(AGGREGATE_OPEN.to_string(), Value::Number(field(0) as f64));
        if matches!(context.u.get(OPEN), Some(Value::Array(frames)) if !frames.is_empty()) {
            append(context, token.src.as_str());
        } else {
            context.u.shift_remove(OPEN);
            context.u.shift_remove(TEXT);
        }
        reading.advance(token.src.as_str());
    } else {
        if inside {
            append(context, token.src.as_str());
        }
        reading.advance(token.src.as_str());
    }
    if TIN_SP != token.tin && TIN_LN != token.tin && TIN_CM != token.tin {
        reading.eq = token.src.as_str() == "=";
    }
}

/// Add `src` to the text of the aggregates open.
fn append(context: &mut Context, src: &str) {
    match context.u.get_mut(TEXT) {
        Some(Value::String(text)) => text.push_str(src),
        _ => {
            context
                .u
                .insert(TEXT.to_string(), Value::String(src.to_string()));
        }
    }
}

/// Whether the window decides where the word at its start ends: `Ok` when
/// a character that is no word's lies in it, or the window holds no word,
/// and otherwise `Err` with the characters the window holds.
fn word_reach(window: &str) -> Result<(), usize> {
    let mut read = 0;
    for ch in window.chars() {
        if !(ch.is_ascii_alphanumeric() || '_' == ch) {
            return Ok(());
        }
        read += 1;
    }
    Err(read)
}

/// The lexer matcher [`crate::make`]'s parser carries with `lex.window`.
/// In a parse from a reader it reads the window on, doubling, until the
/// word at the cursor ends inside it, or the source ends, so that a
/// keyword's `\b` sees the character after the word as the whole text
/// shows it. It cuts no token, and in a parse of a whole text it returns
/// at once.
pub(crate) fn reach_matcher() -> LexMatcher {
    LexMatcher {
        name: "proto-reach".to_string(),
        order: 900_000.0,
        matcher: None,
        imperative: Some(Arc::new(
            |lexer: &mut Lexer<'_>, _rule: &mut Rule, _context: &mut Context| {
                if !lexer.windowed() {
                    return None;
                }
                let mut ahead = 1;
                loop {
                    let held = lexer.fill(ahead);
                    match word_reach(lexer.window()) {
                        Ok(()) => return None,
                        Err(_) if !held => return None,
                        Err(read) => ahead = (read + 1).max(2 * ahead),
                    }
                }
            },
        )),
        factory: None,
    }
}
