// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! A string value written as adjacent literals: `option (f) = "a" "b";`.
//!
//! Port of `ts/src/strings.ts`, which explains protoc's reading in full:
//! a string wherever protoc reads one is every literal that follows, each
//! decoded by the tokenizer's `ParseStringAppend` and the bytes run
//! together. A value written as ONE literal keeps the text between its
//! quotes as written, escapes included, as it always has here.

/// The literals a CST `src` holds, in order, or `None` when `src` is not
/// one or more whole literals.
///
/// `src` is the literals' own text run together (the lexer drops the
/// space between them), so each literal ends at the first unescaped copy
/// of the quote it opened with.
fn split_literals(src: &str) -> Option<Vec<&str>> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let quote = bytes[at];
        if b'"' != quote && b'\'' != quote {
            return None;
        }
        let mut end = at + 1;
        while end < bytes.len() && quote != bytes[end] {
            end += if b'\\' == bytes[end] { 2 } else { 1 };
        }
        if bytes.len() <= end {
            return None;
        }
        // Both ends are ASCII quotes, so both are character boundaries.
        out.push(&src[at..=end]);
        at = end + 1;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn is_octal(byte: u8) -> bool {
    (b'0'..=b'7').contains(&byte)
}

/// A digit's value in any base up to 36, as the tokenizer's `DigitValue`
/// reads it.
fn digit_value(byte: u8) -> u32 {
    match byte {
        b'0'..=b'9' => u32::from(byte - b'0'),
        b'A'..=b'Z' => u32::from(byte - b'A') + 10,
        b'a'..=b'z' => u32::from(byte - b'a') + 10,
        _ => 36,
    }
}

/// The letters `TranslateEscape` knows, as the byte each stands for.
fn escape(letter: u8) -> Option<u8> {
    Some(match letter {
        b'a' => 0x07,
        b'b' => 0x08,
        b'f' => 0x0c,
        b'n' => 0x0a,
        b'r' => 0x0d,
        b't' => 0x09,
        b'v' => 0x0b,
        b'\\' | b'?' | b'\'' | b'"' => letter,
        _ => return None,
    })
}

/// Append a code point as the tokenizer's `AppendUTF8` does: a surrogate
/// is encoded like any other code point, and one past U+10FFFF is written
/// out as `\U` and eight lower-case hex digits.
fn append_utf8(cp: u64, out: &mut Vec<u8>) {
    // Each `as u8` keeps the low byte of a value already masked or shifted
    // into range, which is the encoding.
    if cp <= 0x7f {
        out.push(cp as u8);
    } else if cp <= 0x7ff {
        out.extend_from_slice(&[0xc0 | (cp >> 6) as u8, 0x80 | (cp & 0x3f) as u8]);
    } else if cp <= 0xffff {
        out.extend_from_slice(&[
            0xe0 | (cp >> 12) as u8,
            0x80 | ((cp >> 6) & 0x3f) as u8,
            0x80 | (cp & 0x3f) as u8,
        ]);
    } else if cp <= 0x10ffff {
        out.extend_from_slice(&[
            0xf0 | (cp >> 18) as u8,
            0x80 | ((cp >> 12) & 0x3f) as u8,
            0x80 | ((cp >> 6) & 0x3f) as u8,
            0x80 | (cp & 0x3f) as u8,
        ]);
    } else {
        out.extend_from_slice(format!("\\U{cp:08x}").as_bytes());
    }
}

/// `ReadHexDigits`: `count` digits read as hex, or `None` where the text
/// ends first. The tokenizer has already refused a `\u` or `\U` that is
/// not followed by hex digits, so no other check is made, and the value is
/// kept whole, as the canonical runtime keeps it.
fn read_hex(bytes: &[u8], at: usize, count: usize) -> Option<u64> {
    let digits = bytes.get(at..at + count)?;
    Some(
        digits
            .iter()
            .fold(0, |value, &byte| value * 16 + u64::from(digit_value(byte))),
    )
}

/// One literal, quotes included, decoded as `ParseStringAppend` decodes
/// it, appended to `out` as bytes.
fn decode_literal(literal: &str, out: &mut Vec<u8>) {
    let bytes = literal.as_bytes();
    let quote = bytes[0];
    let mut at = 1;
    while at < bytes.len() {
        let byte = bytes[at];
        if b'\\' == byte && at + 1 < bytes.len() {
            at += 1;
            let letter = bytes[at];
            if is_octal(letter) {
                let mut code = digit_value(letter);
                for _ in 0..2 {
                    if at + 1 < bytes.len() && is_octal(bytes[at + 1]) {
                        at += 1;
                        code = code * 8 + digit_value(bytes[at]);
                    }
                }
                out.push((code & 0xff) as u8);
            } else if b'x' == letter || b'X' == letter {
                let mut code = 0;
                for _ in 0..2 {
                    if at + 1 < bytes.len() && bytes[at + 1].is_ascii_hexdigit() {
                        at += 1;
                        code = code * 16 + digit_value(bytes[at]);
                    }
                }
                out.push(code as u8);
            } else if b'u' == letter || b'U' == letter {
                let count = if b'u' == letter { 4 } else { 8 };
                let Some(mut cp) = read_hex(bytes, at + 1, count) else {
                    out.push(letter);
                    at += 1;
                    continue;
                };
                let mut next = at + 1 + count;
                // A head surrogate followed by `\u` and a trail surrogate is
                // one code point; a lone one is emitted as it stands.
                if (0xd800..0xdc00).contains(&cp)
                    && Some(&b'\\') == bytes.get(next)
                    && Some(&b'u') == bytes.get(next + 1)
                {
                    if let Some(trail) = read_hex(bytes, next + 2, 4) {
                        if (0xdc00..0xe000).contains(&trail) {
                            cp = 0x10000 + (((cp - 0xd800) << 10) | (trail - 0xdc00));
                            next += 6;
                        }
                    }
                }
                append_utf8(cp, out);
                at = next;
                continue;
            } else {
                out.push(escape(letter).unwrap_or(b'?'));
            }
        } else if !(quote == byte && at == bytes.len() - 1) {
            // Anything but the closing quote.
            out.push(byte);
        }
        at += 1;
    }
}

/// absl::CEscape, which protoc applies to a `bytes` field's default: the
/// usual C escapes, and every other byte outside printable ASCII as three
/// octal digits.
fn c_escape(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &byte in bytes {
        match byte {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'"' => out.push_str("\\\""),
            b'\'' => out.push_str("\\'"),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\{byte:03o}")),
        }
    }
    out
}

/// The value protoc records for a string written as adjacent literals, or
/// `None` when `src` is a single literal (or not literals at all), which
/// the caller reads as it always has.
///
/// `bytes` selects protoc's reading of a `bytes` field's default, which
/// escapes the result again. Otherwise a value that is not UTF-8 has each
/// ill-formed sequence replaced by U+FFFD, as the canonical runtime's
/// WHATWG decoder replaces it (the maximal subpart rule, which
/// `String::from_utf8_lossy` also follows), and a leading byte order mark
/// is kept: it is part of the value.
pub(crate) fn adjacent_value(src: &str, bytes: bool) -> Option<String> {
    let literals = split_literals(src)?;
    if literals.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for literal in literals {
        decode_literal(literal, &mut out);
    }
    Some(if bytes {
        c_escape(&out)
    } else {
        String::from_utf8_lossy(&out).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_literal_is_left_to_the_caller() {
        assert_eq!(None, adjacent_value("\"\\x41\"", false));
        assert_eq!(None, adjacent_value("abc", false));
        assert_eq!(None, adjacent_value("\"a\" b", false));
    }

    #[test]
    fn adjacent_literals_decode_as_protoc_reads_them() {
        let value = |src: &str| adjacent_value(src, false).unwrap();
        assert_eq!("ab", value("\"a\"'b'"));
        assert_eq!("A\n?", value("\"\\x41\"\"\\n\\q\""));
        assert_eq!("\u{1F600}", value("\"\\ud83d\\ude00\"\"\""));
        assert_eq!("\\U00110000", value("\"\\U00110000\"\"\""));
        assert_eq!("S\u{FFFD}", value("\"\\123\\777\"\"\""));
        assert_eq!("\u{FEFF}x", value("\"\\xef\\xbb\\xbf\"\"x\""));
        assert_eq!("u12", value("\"\\u12\"\"\""));
    }

    #[test]
    fn a_bytes_default_is_escaped_again() {
        assert_eq!(
            "a\\001b\\\"\\\\\\'",
            adjacent_value("\"a\\1\"\"b\\\"\\\\\\'\"", true).unwrap()
        );
    }
}
