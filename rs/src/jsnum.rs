/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! `Number(string)` as ECMA-262 defines it (7.1.4.1, StringToNumber),
//! for the one place the canonical walk calls it: reading a numeric
//! constant or a field number out of a token's text.
//!
//! The WRITING direction is here too, in [`js_number_to_string`]: the
//! descriptor's JSON has to spell a number the way `JSON.stringify`
//! spells it, and Rust's own `f64` formatting does not.
//!
//! Three details the obvious reading gets wrong, each of which this
//! reproduces:
//!
//! - A NonDecimalIntegerLiteral takes NO sign. `Number("-0x10")` is
//!   `NaN`, not -16, and the canonical walk therefore keeps `-0x10` as
//!   the literal text.
//! - A numeric separator is a SOURCE literal feature and not part of
//!   StringToNumber, so `Number("1_0")` is `NaN` even though the tabnas
//!   lexer happily tokenises `1_0`.
//! - JavaScript's whitespace is `\p{White_Space}` minus U+0085 plus
//!   U+FEFF, which is neither `char::is_whitespace` nor `str::trim`.

/// Whether `ch` is StrWhiteSpace: ECMA-262 WhiteSpace plus
/// LineTerminator.
///
/// U+0085 is in Unicode's `White_Space` property and is NOT whitespace
/// to JavaScript; U+FEFF is not in the property and IS whitespace to
/// JavaScript. `char::is_whitespace` gets both the wrong way round.
///
/// The set, spelt out: TAB, LF, VT, FF, CR, SP, NBSP, U+1680, the
/// U+2000 to U+200A run, LS, PS, U+202F, U+205F, U+3000 and ZWNBSP.
pub(crate) fn is_js_whitespace(ch: char) -> bool {
    // The one run, kept out of the alternation below because rustfmt
    // re-indents a range pattern inside `matches!` into something that
    // reads as a nesting it is not.
    if ('\u{2000}'..='\u{200A}').contains(&ch) {
        return true;
    }
    matches!(
        ch,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// `Number(text)`, or `None` where it is `NaN`.
///
/// `None` rather than `Some(f64::NAN)` so a caller can tell "not a
/// number" from a number, which is exactly the test the canonical walk
/// makes with `Number.isNaN`.
pub(crate) fn js_number(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(is_js_whitespace);
    if trimmed.is_empty() {
        // StringToNumber of an empty string is +0.
        return Some(0.0);
    }

    // NonDecimalIntegerLiteral: no sign, and the base decides the digits.
    if let Some(digits) = strip_prefix_ci(trimmed, "0x") {
        return radix_power_of_two(digits, 16, 4);
    }
    if let Some(digits) = strip_prefix_ci(trimmed, "0o") {
        return radix_power_of_two(digits, 8, 3);
    }
    if let Some(digits) = strip_prefix_ci(trimmed, "0b") {
        return radix_power_of_two(digits, 2, 1);
    }

    let (negative, body) = match trimmed.as_bytes()[0] {
        b'-' => (true, &trimmed[1..]),
        b'+' => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if "Infinity" == body {
        return Some(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if !is_str_decimal(body) {
        return None;
    }
    // The shape is already checked, so Rust's own reader can only read
    // it as the same value: both are correctly rounded, and the spellings
    // Rust accepts that JavaScript does not (`inf`, `NaN`, a separator)
    // are refused above.
    let value: f64 = body.parse().ok()?;
    Some(if negative { -value } else { value })
}

/// `text` without a leading `prefix`, matched case-insensitively over
/// ASCII. `str::to_lowercase` is Unicode-aware and would also fold, for
/// example, U+212A KELVIN SIGN onto `k`.
fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        Some(&text[prefix.len()..])
    } else {
        None
    }
}

/// Whether `body` is a StrUnsignedDecimalLiteral without `Infinity`:
/// `digits [ "." digits? ] [ exponent ]` or `"." digits [ exponent ]`.
fn is_str_decimal(body: &str) -> bool {
    let bytes = body.as_bytes();
    let mut at = 0;
    let integer = take_digits(bytes, &mut at);
    let mut fraction = false;
    if at < bytes.len() && b'.' == bytes[at] {
        at += 1;
        fraction = take_digits(bytes, &mut at);
    }
    if !integer && !fraction {
        return false;
    }
    if at < bytes.len() && (b'e' == bytes[at] || b'E' == bytes[at]) {
        at += 1;
        if at < bytes.len() && (b'+' == bytes[at] || b'-' == bytes[at]) {
            at += 1;
        }
        if !take_digits(bytes, &mut at) {
            return false;
        }
    }
    at == bytes.len()
}

/// Consume a run of ASCII digits, reporting whether there was one.
/// `u8::is_ascii_digit` and not `char::is_numeric`, which would also
/// accept every other decimal digit Unicode has.
fn take_digits(bytes: &[u8], at: &mut usize) -> bool {
    let start = *at;
    while *at < bytes.len() && bytes[*at].is_ascii_digit() {
        *at += 1;
    }
    start < *at
}

/// A binary, octal or hexadecimal integer literal, read exactly.
///
/// Every one of the three bases is a power of two, so the literal's
/// digits ARE the value's bits: collect the significant ones, keep a
/// round bit and a sticky bit for everything past the 53rd, and round to
/// nearest with ties to even, as a correctly rounded reader must. An
/// accumulate-and-multiply loop in `f64` would round at every digit
/// instead, and a `u128` accumulator would silently wrap.
fn radix_power_of_two(digits: &str, radix: u32, bits_per_digit: u32) -> Option<f64> {
    if digits.is_empty() {
        return None;
    }
    let mut significant: u64 = 0;
    let mut collected: u32 = 0;
    let mut dropped: i32 = 0;
    let mut sticky = false;
    let mut started = false;

    for ch in digits.chars() {
        // `char::to_digit` is ASCII-only for these radices, which is what
        // JavaScript's reader accepts.
        let digit = ch.to_digit(radix)?;
        for shift in (0..bits_per_digit).rev() {
            let bit = (digit >> shift) & 1;
            if !started {
                if 0 == bit {
                    continue;
                }
                started = true;
            }
            if collected < 64 {
                significant = (significant << 1) | u64::from(bit);
                collected += 1;
            } else {
                sticky |= 1 == bit;
                dropped += 1;
            }
        }
    }

    if !started {
        return Some(0.0);
    }
    if collected <= 53 {
        // Exact: at most 53 significant bits, and nothing was dropped.
        return Some(significant as f64);
    }

    let excess = collected - 53;
    let round_bit = (significant >> (excess - 1)) & 1;
    let low_mask = (1u64 << (excess - 1)) - 1;
    sticky |= 0 != (significant & low_mask);
    let mut mantissa = significant >> excess;
    let mut exponent = dropped + excess as i32;
    if 1 == round_bit && (sticky || 1 == (mantissa & 1)) {
        mantissa += 1;
        if mantissa == (1u64 << 53) {
            mantissa >>= 1;
            exponent += 1;
        }
    }
    Some(mantissa as f64 * (2f64).powi(exponent))
}

/// JavaScript's `Number::toString` (ECMA-262 6.1.6.1.20), which is what
/// `JSON.stringify` spells a number with, and so what the canonical
/// descriptor's JSON carries.
///
/// COPIED, not rewritten: this is `js_number_to_string` from
/// `../../csv/rs/src/lib.rs`, which is the fleet's one implementation of
/// the algorithm. Rust's own `f64` formatting differs from it in three
/// ways that reach a descriptor. It keeps the sign of negative zero
/// (`-0`, where JavaScript says `0`). It never switches to exponent form,
/// where JavaScript does so at `1e21` and at `1e-7`. And
/// `format!("{n:.0}")` prints a large integral float's exact binary value
/// (`123456789012345683968`) rather than its shortest round-tripping
/// digits (`123456789012345680000`).
///
/// `rs/tests/jsnum_test.rs` grades it against node over more than 55,000
/// doubles.
pub(crate) fn js_number_to_string(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    // Catches -0.0 as well: JavaScript spells both zeros "0".
    if number == 0.0 {
        return "0".to_string();
    }
    if number < 0.0 {
        return format!("-{}", js_number_to_string(-number));
    }
    if number.is_infinite() {
        return "Infinity".to_string();
    }

    // The specification wants the shortest digit string `s` that round-trips
    // (length `k`), and `n`, the position of the decimal point relative to
    // it. Rust's `{:e}` yields digits of exactly that shortest length.
    let shortest = format!("{number:e}");
    let shortest_k = shortest
        .split_once('e')
        .map(|(mantissa, _)| mantissa.chars().filter(char::is_ascii_digit).count())
        .expect("a finite f64 always formats with an exponent");

    // Re-render to that same length to settle a tie. Where two digit
    // strings of length `k` are equally close to `number`, the
    // specification takes the one ending in an even digit; Rust's shortest
    // form does not, but its exactly-rounded fixed-precision form does.
    let exponential = format!("{:.*e}", shortest_k - 1, number);
    let (mantissa, exponent) = exponential
        .split_once('e')
        .expect("a finite f64 always formats with an exponent");
    // Rounding can leave trailing zeros (and, on a carry, one digit too
    // many); dropping them keeps `s` shortest, which is what `k` means.
    let digits = mantissa
        .chars()
        .filter(|digit| *digit != '.')
        .collect::<String>();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let k = digits.len() as i32;
    let n = exponent
        .parse::<i32>()
        .expect("a formatted exponent is an integer")
        + 1;

    // The four cases of the specification, in its order. The range bounds
    // are `k <= n <= 21`, `0 < n <= 21` and `-6 < n <= 0`.
    if (k..=21).contains(&n) {
        // Integral, with n - k trailing zeros to restore.
        let mut text = digits.to_string();
        text.push_str(&"0".repeat((n - k) as usize));
        text
    } else if (1..=21).contains(&n) {
        let point = n as usize;
        format!("{}.{}", &digits[..point], &digits[point..])
    } else if (-5..=0).contains(&n) {
        format!("0.{}{}", "0".repeat(-n as usize), digits)
    } else {
        // Exponent form. `n - 1` is never 0 here, so the sign is never "+0".
        let sign = if n - 1 < 0 { '-' } else { '+' };
        let power = (n - 1).abs();
        if k == 1 {
            format!("{digits}e{sign}{power}")
        } else {
            format!("{}.{}e{sign}{power}", &digits[..1], &digits[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_javascript_reads() {
        assert_eq!(js_number("1"), Some(1.0));
        assert_eq!(js_number("010"), Some(10.0));
        assert_eq!(js_number("1e2"), Some(100.0));
        assert_eq!(js_number(".5"), Some(0.5));
        assert_eq!(js_number("1."), Some(1.0));
        assert_eq!(js_number("1.e2"), Some(100.0));
        assert_eq!(js_number("0x10"), Some(16.0));
        assert_eq!(js_number("0X7FFFFFFF"), Some(2147483647.0));
        assert_eq!(js_number("0o17"), Some(15.0));
        assert_eq!(js_number("0b101"), Some(5.0));
        assert_eq!(js_number("1e400"), Some(f64::INFINITY));
        assert_eq!(js_number(""), Some(0.0));
        assert_eq!(js_number("  12  "), Some(12.0));
    }

    #[test]
    fn refuses_what_javascript_refuses() {
        // A sign is not part of a NonDecimalIntegerLiteral.
        assert_eq!(js_number("-0x10"), None);
        assert_eq!(js_number("+0b1"), None);
        // Numeric separators belong to source literals, not to
        // StringToNumber.
        assert_eq!(js_number("1_0"), None);
        assert_eq!(js_number("1_000"), None);
        // Rust's own reader accepts these; JavaScript's does not.
        assert_eq!(js_number("inf"), None);
        assert_eq!(js_number("NaN"), None);
        assert_eq!(js_number("0o8"), None);
        assert_eq!(js_number("0b12"), None);
        assert_eq!(js_number("1e"), None);
        assert_eq!(js_number("."), None);
    }

    #[test]
    fn rounds_a_long_hexadecimal_literal_the_way_one_read_of_it_would() {
        // 2^53 + 1 is not representable; nearest-even takes 2^53.
        assert_eq!(js_number("0x20000000000001"), Some(9007199254740992.0));
        // 2^53 + 3 rounds up to 2^53 + 4.
        assert_eq!(js_number("0x20000000000003"), Some(9007199254740996.0));
        // Well past u128, and still a finite double.
        assert_eq!(
            js_number(&format!("0x1{}", "0".repeat(60))),
            Some((2f64).powi(240))
        );
        // Past the largest double.
        assert_eq!(
            js_number(&format!("0x1{}", "0".repeat(300))),
            Some(f64::INFINITY)
        );
    }

    #[test]
    fn javascript_whitespace_is_not_unicode_whitespace() {
        // U+0085 is Unicode White_Space and is not JavaScript's.
        assert!(!is_js_whitespace('\u{0085}'));
        assert_eq!(js_number("\u{0085}1"), None);
        // U+FEFF is not Unicode White_Space and is JavaScript's.
        assert!(is_js_whitespace('\u{FEFF}'));
        assert_eq!(js_number("\u{FEFF}1\u{FEFF}"), Some(1.0));
    }
}
