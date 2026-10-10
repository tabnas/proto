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

/// The refusal carries no code, as every refusal the plugin makes itself
/// does, and its message is the one the TypeScript and Go ports give:
/// `test/spec/nesting.tsv` holds all three to it.
#[test]
fn the_nesting_refusal_carries_no_code_and_names_the_depth() {
    let error = parse(&nested(MAX_NESTING_DEPTH + 1), None).expect_err("past the cap");
    assert_eq!(error.code(), "");
    assert_eq!(error.position(), None);
    assert_eq!(
        error.to_string(),
        "proto: document nests 101 levels deep, past the 100 this parser accepts"
    );
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

/// The walk's own bound, which is this port's alone: a tree past the cap,
/// built by a caller who drove the engine and skipped the preflight, is
/// refused by `to_descriptor`, because a Rust stack that runs out aborts.
/// TypeScript's `toDescriptor` and Go's `ToDescriptor` walk it
/// (`ts/test/preflight.test.ts`, `go/preflight_test.go`), as
/// `DIVERGENCE.md` section 2 records.
#[test]
fn the_walk_refuses_a_tree_past_the_cap() {
    let parser = make();
    let cst = parser
        .parse(&nested(MAX_NESTING_DEPTH + 1))
        .expect("the engine's own parse has no cap");
    let error = tabnas_proto::to_descriptor(&cst, None).expect_err("the walk refuses it");
    assert_eq!(
        error.to_string(),
        format!("proto: document nests deeper than {MAX_NESTING_DEPTH} levels")
    );
}

/// The check on its own, for a caller that wants the CST.
#[test]
fn preflight_accepts_at_the_cap_and_refuses_past_it() {
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

// The scan skips strings and comments where the lexer finds them, no
// sooner and no later: a brace it skips that the lexer counts would let a
// document past the cap reach the engine, and the engine build the tree
// the cap exists to keep it from building.

/// A document one level past the cap after `head`.
fn past(head: &str) -> String {
    format!(
        "{head}{}{}",
        "message M {".repeat(MAX_NESTING_DEPTH + 1),
        "}".repeat(MAX_NESTING_DEPTH + 1)
    )
}

fn refused_at(src: &str, depth: usize) {
    let want = format!("nests {depth} levels deep");
    let error = preflight(src).expect_err("the check refuses it");
    assert!(error.to_string().contains(&want), "{src:.60}: {error}");
    let error = parse(src, None).expect_err("parse refuses it");
    assert!(error.to_string().contains(&want), "{src:.60}: {error}");
}

/// The lexer ends a line at a carriage return as well as a line feed, and
/// a line comment with it, so a document that breaks its lines with
/// carriage returns alone cannot hide its nesting in a comment.
#[test]
fn a_line_comment_ends_at_a_carriage_return() {
    for comment in ["// c", "# c"] {
        refused_at(
            &past(&format!("syntax = \"proto2\";\r{comment}\r")),
            MAX_NESTING_DEPTH + 1,
        );
    }
}

/// The lexer reads a backtick string as a string, across lines too, and a
/// backslash escapes a backtick in it, so its braces nest nothing.
#[test]
fn braces_in_a_backtick_string_do_not_count() {
    let noise = "{".repeat(4 * MAX_NESTING_DEPTH);
    for src in [
        format!("syntax = \"proto2\";\noption a = `{noise}`;\n"),
        format!("syntax = \"proto2\";\noption a = `\\`{noise}`;\n"),
        format!("syntax = \"proto2\";\noption a = `{noise}\n{noise}`;\n"),
    ] {
        preflight(&src).unwrap_or_else(|error| panic!("{src:.40}: {error}"));
        parse(&src, None).unwrap_or_else(|error| panic!("{src:.40}: {error}"));
    }
}

/// A quote opens a string only where the lexer starts a token. Inside a
/// word the lexer reads it as part of the word (`message a"b` names a
/// message `a"b`), so the braces after it count; straight after a keyword
/// it opens a string, whose braces close nothing.
#[test]
fn a_quote_opens_a_string_only_where_a_token_starts() {
    for quote in ['"', '\'', '`'] {
        let head = format!("syntax = \"proto2\";\nmessage a{quote}b {{");
        refused_at(&format!("{}}}", past(&head)), MAX_NESTING_DEPTH + 2);
    }
    refused_at(
        &format!(
            "syntax = \"proto2\";\n{}reserved\"{}\";{}{}",
            "message M {".repeat(90),
            "}".repeat(90),
            "message M {".repeat(11),
            "}".repeat(101)
        ),
        101,
    );
    let noise = "{".repeat(4 * MAX_NESTING_DEPTH);
    let file = parse(
        &format!("syntax = \"proto2\";\nmessage M {{ reserved\"{noise}\"; }}\n"),
        None,
    )
    .expect("a string after a keyword parses");
    assert_eq!(file.message_type[0].reserved_name, Some(vec![noise]));
}

/// The scan's keywords and separators are the grammar's: after each
/// keyword the grammar's match tokens name, and after each fixed token, a
/// quote opens a string, whose braces close nothing. A keyword the grammar
/// gains and the scan does not know fails here.
#[test]
fn the_scan_knows_the_grammars_tokens() {
    let grammar: serde_json::Value =
        serde_json::from_str(include_str!("../proto-grammar.json")).expect("the grammar is JSON");
    // Sixty levels open, a string of sixty closers, and sixty levels more:
    // 120 levels when the closers are a string's, under the cap when they
    // count.
    let depth = |lead: &str| -> usize {
        let src = format!(
            "{}{lead}\"{}\"{}",
            "{".repeat(60),
            "}".repeat(60),
            "{".repeat(60)
        );
        match preflight(&src) {
            Ok(()) => 0,
            Err(error) => error
                .to_string()
                .split("nests ")
                .nth(1)
                .and_then(|rest| rest.split(' ').next())
                .and_then(|depth| depth.parse().ok())
                .expect("the refusal names a depth"),
        }
    };
    let keywords = grammar["options"]["match"]["token"]
        .as_object()
        .expect("the grammar names match tokens");
    assert!(!keywords.is_empty());
    for (name, pattern) in keywords {
        let pattern = pattern.as_str().expect("a pattern");
        let word = pattern
            .strip_prefix("@~/^")
            .and_then(|rest| rest.strip_suffix("\\b/"))
            .filter(|word| !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphabetic()))
            .unwrap_or_else(|| {
                panic!("match token {name} is not a keyword the scan can read: {pattern}")
            });
        assert_eq!(depth(&format!(" {word}")), 120, "after the keyword {word}");
        assert_eq!(depth(&format!(" {word}x")), 0, "after the word {word}x");
    }
    let fixed = grammar["options"]["fixed"]["token"]
        .as_object()
        .expect("the grammar names fixed tokens");
    for (name, token) in fixed {
        let token = token.as_str().expect("a token");
        if "{" == token || "}" == token {
            continue;
        }
        assert_eq!(
            depth(&format!("a{token}")),
            120,
            "after the fixed token {name} ({token})"
        );
    }
}
