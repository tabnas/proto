// A `.proto` file is untrusted input. Deep nesting, very long source,
// unterminated constructs, empty input, control characters and odd
// Unicode must not panic, hang, overflow the stack or take super-linear
// time. This suite pins each boundary.
//
// The Rust port has one hazard the other two runtimes do not: a stack
// that runs out ABORTS the process rather than raising something
// catchable, and three passes over a `.proto` document recurse. The
// engine's parse loop is iterative, but the descriptor walk is
// recursive, `tabnas::Value`'s default `Drop` is recursive, and so is
// `Value::to_json`. So the crate refuses nesting past a documented cap,
// before anything that deep is built. See `MAX_NESTING_DEPTH`, whose
// doc comment carries the measurements the cap is chosen from.

mod common;

use std::time::Instant;

use tabnas_proto::{make, parse, parse_with, preflight, MAX_NESTING_DEPTH};

/// `message M { message M { ... } }`, nested `depth` levels.
fn nested(depth: usize) -> String {
    format!(
        "syntax = \"proto2\";\n{}{}",
        "message M {".repeat(depth),
        "}".repeat(depth)
    )
}

/// Nesting AT the cap parses. A cap nobody tests at is a number, not a
/// bound: if this is ever off by one, the test that only checks the
/// refusal would still pass.
#[test]
fn nesting_at_the_cap_is_accepted() {
    let file = parse(&nested(MAX_NESTING_DEPTH), None)
        .expect("a document nesting exactly to the cap parses");
    // ... and the whole of it arrives, rather than being truncated at
    // some shallower level.
    let mut message = &file.message_type[0];
    let mut depth = 1;
    while let Some(inner) = message.nested_type.first() {
        message = inner;
        depth += 1;
    }
    assert_eq!(depth, MAX_NESTING_DEPTH);
}

/// One level under, for the same reason.
#[test]
fn nesting_under_the_cap_is_accepted() {
    parse(&nested(MAX_NESTING_DEPTH - 1), None).expect("one level under the cap parses");
}

/// Past the cap the document is REFUSED, by name, rather than aborting
/// the process on a stack that ran out.
#[test]
fn nesting_past_the_cap_is_refused() {
    for depth in [MAX_NESTING_DEPTH + 1, 10 * MAX_NESTING_DEPTH] {
        let error = parse(&nested(depth), None).expect_err("past the cap is refused");
        assert!(
            error.to_string().contains("nests"),
            "at depth {depth}: got {error}, want the nesting refusal"
        );
    }
}

/// Deeply nested and never closed: refused rather than aborting. The
/// brace scan counts opening braces, so an unterminated pile is caught
/// before the engine sees it.
#[test]
fn deep_unclosed_nesting_is_refused() {
    let src = format!("syntax = \"proto2\";\n{}", "message M {".repeat(5000));
    let error = parse(&src, None).expect_err("an unclosed pile of messages is refused");
    assert!(error.to_string().contains("nests"), "got {error}");
}

/// The brace scan does not count a brace inside a string or a comment, so
/// a document that merely MENTIONS braces is not refused for nesting.
#[test]
fn braces_in_strings_and_comments_do_not_count_as_nesting() {
    let noise = "{".repeat(MAX_NESTING_DEPTH * 4);
    for src in [
        format!("syntax = \"proto2\";\noption a = \"{noise}\";\n"),
        format!("syntax = \"proto2\";\n// {noise}\nmessage M {{}}\n"),
        format!("syntax = \"proto2\";\n/* {noise} */\nmessage M {{}}\n"),
    ] {
        let outcome = parse(&src, None);
        if let Err(error) = &outcome {
            assert!(
                !error.to_string().contains("nests"),
                "refused for nesting: {src:.60}"
            );
        }
    }
}

/// Empty and whitespace-only sources are legal `.proto` files, and the
/// leniency corpus records protoc agreeing.
#[test]
fn empty_input_parses_to_an_empty_descriptor() {
    for src in ["", "   ", "\n\n", "// only a comment\n", "\r\n\r\n", ";;;"] {
        let file = parse(src, None).unwrap_or_else(|error| panic!("{src:?}: {error}"));
        assert!(file.message_type.is_empty(), "for {src:?}");
        // protoc's default for a file with no declaration.
        assert_eq!(file.syntax.as_deref(), Some("proto2"), "for {src:?}");
    }
}

/// Every unterminated construct the notation has. Each outcome is a value
/// or an error; aborting, hanging or panicking is not.
#[test]
fn unterminated_constructs_do_not_panic() {
    for src in [
        "syntax = \"proto3\";\nmessage M { int32 a = 1;\n",
        "syntax = \"proto3\";\nmessage M {",
        "syntax = \"proto3",
        "syntax =",
        "syntax",
        "message M { oneof o {",
        "enum E {",
        "service S { rpc R (",
        "extend M {",
        "message M { optional int32 a = 1 [",
        "message M { reserved 1 to",
        "option",
        "option a =",
        "/* unterminated",
        "message M { group G = 1 {",
        "message M { map<string,",
    ] {
        let _ = parse(src, None);
    }
}

/// Control characters and non-ASCII text in every position that takes
/// text: an identifier, a string literal, a comment and an option value.
#[test]
fn control_characters_and_odd_unicode_do_not_panic() {
    for src in [
        "syntax = \"proto3\";\nmessage M { int32 \u{0}a = 1; }",
        "syntax = \"proto3\";\noption a = \"\u{0}\u{1}\u{7f}\";",
        "syntax = \"proto3\";\noption a = \"\u{1f600}\u{202e}\";",
        "syntax = \"proto3\";\n// \u{1f600} \u{0}\nmessage M {}",
        "syntax = \"proto3\";\nmessage M\u{e9} {}",
        "syntax = \"proto3\";\nmessage M { optional int32 \u{e9} = 1; }",
        "syntax = \"proto3\";\nmessage M { reserved \"\u{1f600}\"; }",
        // A lone `\uD83D` escape cannot be written in Rust source, and a
        // `.proto` string keeps its escapes as text anyway: what the
        // parser sees is the six characters of the escape.
        "syntax = \"proto3\";\noption a = \"\\ud83d\";",
        "syntax = \"proto3\";\nmessage M { optional group \u{1f600}G = 1 {} }",
        "\u{feff}syntax = \"proto3\";",
        "syntax = \"proto3\";\noption a = 1\u{85};",
    ] {
        let _ = parse(src, None);
    }
}

/// A truncation that would split a character. Every prefix of a document
/// carrying astral characters is fed in, cut at a CHARACTER boundary,
/// because a Rust `&str` cannot hold half of one; the point is that no
/// prefix reaches a slice at a computed offset that is not a boundary.
#[test]
fn every_prefix_of_a_multibyte_document_is_safe() {
    let src = "syntax = \"proto3\";\nmessage M\u{1f600} { optional \u{e9}T \u{1f600}a = 1 \
               [default = \"\u{4e2d}\u{6587}\"]; }\n";
    for (at, _) in src.char_indices() {
        let _ = parse(&src[..at], None);
    }
    let _ = parse(src, None);
}

/// A very long single token: a name, a string and a number, each far
/// longer than anything real. This is the dimension that IS linear here;
/// `rs/AGENTS.md` records the one that is not, and where it lives.
#[test]
fn very_long_tokens_are_bounded() {
    // Warm the shared instance so the one-off grammar compile is not
    // counted against a token length.
    let _ = parse("syntax = \"proto2\";", None);

    let long_name = "a".repeat(100_000);
    let long_string = "x".repeat(100_000);
    let long_number = "9".repeat(20_000);
    let start = Instant::now();
    let _ = parse(
        &format!("message M {{ optional int32 {long_name} = 1; }}"),
        None,
    );
    let _ = parse(
        &format!("syntax = \"proto3\";\noption a = \"{long_string}\";"),
        None,
    );
    let _ = parse(
        &format!("message M {{ optional int32 a = {long_number}; }}"),
        None,
    );
    let elapsed = start.elapsed().as_secs_f64();
    // A ceiling, not a ratio: an absolute bound catches a hang, where a
    // ratio would quietly pin whatever curve the engine has today.
    assert!(elapsed < 30.0, "three long tokens took {elapsed:.1}s");
}

/// A long reserved-name list is read by scanning `src` rather than by
/// descending, so it never touches the nesting cap, and every name
/// arrives.
#[test]
fn a_long_reserved_name_list_is_read_whole() {
    let names: Vec<String> = (0..100).map(|index| format!("\"n{index}\"")).collect();
    let src = format!(
        "syntax = \"proto2\";\nmessage M {{ reserved {}; }}\n",
        names.join(", ")
    );
    let file = parse(&src, None).expect("a long reserved-name list parses");
    let reserved = file.message_type[0]
        .reserved_name
        .as_ref()
        .expect("M reserves names");
    assert_eq!(reserved.len(), 100);
    assert_eq!(reserved[0], "n0");
    assert_eq!(reserved[99], "n99");
}

/// A long range list, the other construct read out of `src`.
#[test]
fn a_long_range_list_is_read_whole() {
    let ranges: Vec<String> = (0..100)
        .map(|index| format!("{} to {}", index * 10 + 1, index * 10 + 5))
        .collect();
    let src = format!(
        "syntax = \"proto2\";\nmessage M {{ reserved {}; }}\n",
        ranges.join(", ")
    );
    let file = parse(&src, None).expect("a long range list parses");
    let reserved = file.message_type[0]
        .reserved_range
        .as_ref()
        .expect("M reserves ranges");
    assert_eq!(reserved.len(), 100);
    // Message reserved ranges are half-open: `1 to 5` is [1,6).
    assert_eq!((reserved[0].start, reserved[0].end), (1.0, 6.0));
    assert_eq!((reserved[99].start, reserved[99].end), (991.0, 996.0));
}

/// The reusable fast path carries the same bound as `parse`.
///
/// This is the route the README recommends to a caller reading many
/// documents, which is the caller most likely to be handed untrusted
/// ones. Before `parse_with` existed the documentation sent that caller
/// to the engine's own `parse`, which builds the tree first and cannot
/// refuse it: a deep enough document ABORTED the process, and an abort
/// is not something a caller can catch.
#[test]
fn the_reusable_path_carries_the_same_bound() {
    let parser = make();

    // AT the cap, and one under, and the whole document arrives.
    for depth in [MAX_NESTING_DEPTH - 1, MAX_NESTING_DEPTH] {
        let file = parse_with(&parser, &nested(depth), None)
            .unwrap_or_else(|error| panic!("{depth} levels should parse: {error}"));
        let mut message = &file.message_type[0];
        let mut levels = 1;
        while let Some(inner) = message.nested_type.first() {
            message = inner;
            levels += 1;
        }
        assert_eq!(levels, depth);
    }

    for depth in [MAX_NESTING_DEPTH + 1, 10 * MAX_NESTING_DEPTH] {
        let error = parse_with(&parser, &nested(depth), None)
            .expect_err("past the cap is refused on the reusable path too");
        assert!(error.to_string().contains("nests"), "for {depth}: {error}");
    }

    // Both routes answer the same way, so neither is the fast one and
    // the safe one.
    let deep = nested(MAX_NESTING_DEPTH + 1);
    assert_eq!(
        parse_with(&parser, &deep, None).unwrap_err().to_string(),
        parse(&deep, None).unwrap_err().to_string(),
    );
}

/// The check on its own, for a caller that wants the CST.
#[test]
fn preflight_refuses_at_the_cap_and_accepts_under_it() {
    preflight(&nested(MAX_NESTING_DEPTH - 1)).expect("one under the cap");
    preflight(&nested(MAX_NESTING_DEPTH)).expect("exactly the cap");

    let error = preflight(&nested(MAX_NESTING_DEPTH + 1)).expect_err("one past the cap");
    assert!(error.to_string().contains("nests"), "{error}");

    // The depth it counts is the source's, not the descriptor's, so the
    // braces it skips are the ones the lexer skips.
    preflight(&format!(
        "option a = \"{}\";",
        "{".repeat(10 * MAX_NESTING_DEPTH)
    ))
    .expect("braces inside a string literal do not nest anything");
}
