// The engine's error carries a code, position, hint and a formatted
// report, so `Result<_, TabnasError>` trips clippy's `result_large_err`;
// an integration test is a crate of its own, so it allows it again.
#![allow(clippy::result_large_err)]

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole, by each of the two parsers, `make()`'s, whose value is
// the CST, and `make_streaming()`'s, which builds no tree. Both declare
// `lex.window`: the nesting cap is counted as the lexer gives the parser
// its tokens, and `make()`'s parser keeps an aggregate's text, which a
// parse of a whole text reads from the source, from the tokens between its
// braces (`src/read.rs`). The keywords, match
// tokens, need nothing of their own: the engine cuts a word that met the
// window's end again over a wider window (tabnas/parser 0.12.17).
//
// Every row of the shared fixtures (`test/spec`, `test/passes`,
// `test/tree`), every input of the protoc corpus (`test/protobuf-suite`)
// and `test/descriptor/descriptor.proto` is parsed whole and from a reader
// that hands its bytes over in pieces of 1, 3 and 64 bytes, under the
// engine's window and under windows whose margin and block are a few
// characters, so that every token meets the window's edge; a long
// document, whose long tokens of every kind run past the engine's window,
// in pieces of 4 KiB too. The value (the CST, an aggregate's text
// included, or the streaming parser's none), the tokens the lexer gives,
// the rule passes, and an error's code, position, row, column and length
// must be the same. An error's
// excerpt is not compared: a parse from a reader shows a long line only as
// far back as the window reaches (tabnas/parser's DIVERGENCE.md).
//
// ts/test/window.test.ts and go/window_test.go hold the TypeScript and Go
// ports to the same.

mod common;

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tabnas::{RuleState, Tabnas, TabnasError, Value, WindowSizes};
use tabnas_proto::{make, make_streaming, parse_tree, MAX_NESTING_DEPTH};

use common::{documents, long_document, long_error_documents};

/// A parser's constructor.
type Build = fn() -> Tabnas;

/// The two parsers, each by the name of its constructor.
const PARSERS: [(&str, Build); 2] = [("make()", make), ("make_streaming()", make_streaming)];

/// A reader that hands its bytes over at most `size` at a time.
struct Pieces<'a> {
    bytes: &'a [u8],
    at: usize,
    size: usize,
}

impl Read for Pieces<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.size.min(buf.len()).min(self.bytes.len() - self.at);
        buf[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// A read of a source: the pieces a reader hands its bytes over in, and the
/// window it is read through, the engine's own (64 KiB) where it is `None`,
/// or one whose margin and block are a few characters, which the hidden
/// `parse_reader_with_sizes` sets, its reads as small as the pieces.
type Reading = (usize, Option<(usize, usize)>);

const PIECES: [usize; 3] = [1, 3, 64];
const WINDOWS: [Option<(usize, usize)>; 4] = [None, Some((1, 1)), Some((3, 2)), Some((64, 64))];

/// A fixture or corpus document is read in every pieces size through every
/// window.
fn reads() -> Vec<Reading> {
    PIECES
        .iter()
        .flat_map(|&size| WINDOWS.iter().map(move |&window| (size, window)))
        .collect()
}

/// A document longer than 16 KiB, of which the corpus holds one, is read
/// in pieces of 64 and 4096 bytes, through the engine's window and one of
/// 64 and 64 characters: a debug build's parse of it a byte at a time
/// takes seconds where its parse takes a fraction of one.
const BIG_READS: [Reading; 3] = [(64, None), (4096, None), (64, Some((64, 64)))];

/// A long document in pieces of 1, 3, 64 and 4096 bytes through the
/// engine's window, whose edges its long tokens straddle, and through a
/// window of 64 and 64 characters.
const LONG_READS: [Reading; 5] = [
    (1, None),
    (3, None),
    (64, None),
    (4096, None),
    (64, Some((64, 64))),
];

type Log = Arc<Mutex<(Vec<String>, Vec<String>)>>;

/// A parser `build` makes, with a log of the tokens the lexer gives and the
/// rule passes.
fn logged(log: &Log, build: Build) -> Tabnas {
    let mut parser = build();
    let tokens = Arc::clone(log);
    parser.subscribe_tokens(move |token| {
        tokens.lock().unwrap().0.push(format!(
            "{} {:?} {} {} {}:{}",
            token.tin, token.src, token.val, token.site.pos, token.site.ri, token.site.ci
        ));
    });
    let rules = Arc::clone(log);
    parser.subscribe_rule_done(move |rule, _context, done| {
        let state = match done.state {
            RuleState::Open => "open",
            RuleState::Close => "close",
        };
        rules
            .lock()
            .unwrap()
            .1
            .push(format!("{} {state} {}", rule.name.as_str(), rule.d));
    });
    parser
}

/// What a parse showed: its value, or its error without the excerpt; and
/// the tokens and rule passes `log` gathered, which is empty again after.
fn seen(log: &Log, parsed: Result<Value, TabnasError>) -> (String, Vec<String>, Vec<String>) {
    let outcome = match parsed {
        Ok(value) => format!("ok {value}"),
        Err(e) => format!(
            "err {} at {} {}:{} {} {:?}",
            e.code, e.pos, e.row, e.col, e.len, e.token
        ),
    };
    let (tokens, rules) = std::mem::take(&mut *log.lock().unwrap());
    (outcome, tokens, rules)
}

/// `text` parsed whole and from a reader in each of `reads`; the count of
/// reader parses compared.
fn differential(parser: &Tabnas, log: &Log, at: &str, text: &str, reads: &[Reading]) -> usize {
    let whole = seen(log, parser.parse(text));
    for &(size, window) in reads {
        let reader = Pieces {
            bytes: text.as_bytes(),
            at: 0,
            size,
        };
        let parsed = match window {
            None => parser.parse_reader(reader),
            Some((margin, block)) => parser.parse_reader_with_sizes(
                reader,
                WindowSizes {
                    margin,
                    block,
                    chunk: size,
                },
            ),
        };
        let read = seen(log, parsed);
        assert_eq!(read, whole, "{at} in pieces of {size}, window {window:?}");
    }
    reads.len()
}

#[test]
fn both_parsers_read_a_reader_a_window_at_a_time() {
    for (name, build) in PARSERS {
        assert!(build().options.lex.window, "{name}");
    }
}

#[test]
fn every_fixture_and_corpus_document_parses_the_same_from_a_reader() {
    let documents = documents();
    assert!(documents.len() > 500, "{}", documents.len());
    let tree = make();
    for (name, build) in PARSERS {
        let log: Log = Arc::default();
        let parser = logged(&log, build);
        let (mut compared, mut aggregates) = (0, 0);
        for (index, (at, text)) in documents.iter().enumerate() {
            let reads = if text.len() > 16 * 1024 {
                BIG_READS.to_vec()
            } else {
                reads()
            };
            compared += differential(&parser, &log, at, text, &reads);
            if matches!(tree.parse(text), Ok(value) if value.to_string().contains("\"aggregate\""))
            {
                aggregates += 1;
            }
            if index % 100 == 0 || index + 1 == documents.len() {
                eprintln!(
                    "window documents, {name}: {} of {} ({}%), {compared} reader parses",
                    index + 1,
                    documents.len(),
                    (index + 1) * 100 / documents.len()
                );
            }
        }
        assert!(aggregates >= 50, "{aggregates} documents hold an aggregate");
    }
}

#[test]
fn a_long_document_parses_the_same_from_a_reader() {
    let text = long_document();
    assert!(text.len() > 512 * 1024, "{}", text.len());
    let tree = parse_tree(&text).expect("the long document parses");
    assert_eq!(tree.to_json()["body"].as_array().map(Vec::len), Some(508));
    for (name, build) in PARSERS {
        let log: Log = Arc::default();
        let parser = logged(&log, build);
        assert_eq!(
            differential(
                &parser,
                &log,
                &format!("the long document, {name}"),
                &text,
                &LONG_READS
            ),
            LONG_READS.len()
        );
    }
}

#[test]
fn a_long_document_fails_the_same_from_a_reader() {
    // A token that runs past the engine's window and is then refused, or
    // never ends, and nesting past the cap after a long run: the error is
    // the whole text's, code, place and token.
    let bad = long_error_documents();
    for (name, build) in PARSERS {
        let log: Log = Arc::default();
        let parser = logged(&log, build);
        let mut compared = 0;
        for (index, text) in bad.iter().enumerate() {
            let whole = seen(&log, parser.parse(text));
            assert!(
                whole.0.starts_with("err "),
                "{name} {index}: {}",
                &whole.0[..200.min(whole.0.len())]
            );
            compared += differential(
                &parser,
                &log,
                &format!("long error document {index}, {name}"),
                text,
                &LONG_READS,
            );
        }
        assert_eq!(compared, bad.len() * LONG_READS.len());
    }
}

#[test]
fn the_parser_refuses_nesting_past_the_cap_as_it_reads() {
    // The brace that opens the level past the cap is refused, with the
    // code `proto_too_deep` at that brace, whole and from a reader, and the
    // cap itself parses.
    let nest = |depth: usize, open: &str| {
        format!(
            "syntax = \"proto2\";\n{}{}",
            open.repeat(depth),
            "}".repeat(depth)
        )
    };
    let cases = [
        (
            nest(MAX_NESTING_DEPTH + 1, "message M {"),
            19 + 11 * MAX_NESTING_DEPTH + 10,
        ),
        (
            nest(MAX_NESTING_DEPTH + 1, "message M{"),
            19 + 10 * MAX_NESTING_DEPTH + 9,
        ),
        (
            format!("option (x) = {}", "{a ".repeat(MAX_NESTING_DEPTH + 1)),
            13 + 3 * MAX_NESTING_DEPTH,
        ),
        ("message M {".repeat(100_000), 11 * MAX_NESTING_DEPTH + 10),
    ];
    for (name, build) in PARSERS {
        let parser = build();
        parser
            .parse(&nest(MAX_NESTING_DEPTH, "message M {"))
            .expect("the cap parses");
        for (text, pos) in &cases {
            for from_reader in [false, true] {
                let error = if from_reader {
                    parser.parse_reader(Pieces {
                        bytes: text.as_bytes(),
                        at: 0,
                        size: 64,
                    })
                } else {
                    parser.parse(text)
                }
                .expect_err("past the cap");
                assert_eq!(
                    (error.code.as_str(), error.pos, error.token.src.as_str()),
                    ("proto_too_deep", *pos, "{"),
                    "{name}: {:.40} (from a reader {from_reader})",
                    text
                );
                assert!(
                    error.to_string().contains(
                        "proto: document nests deeper than the 100 levels this parser accepts"
                    ),
                    "{error}"
                );
            }
        }
    }
}

#[test]
fn a_long_document_parses_from_a_reader_in_time_linear_in_its_length() {
    // Ten times the messages, read through the engine's window in pieces
    // of 4 KiB, take about ten times as long, never the square of it.
    for (name, build) in PARSERS {
        linear_time(name, build());
    }
}

/// Time `parser` over 300 messages and 3,000.
fn linear_time(name: &str, parser: Tabnas) {
    let document = |messages: usize| -> String {
        let mut text = String::from("syntax = \"proto3\";\n");
        for i in 0..messages {
            text.push_str(&format!(
                "message M{i} {{ string a = 1; int32 b = 2 [(o) = {{x: {i}}}]; }} // c\n"
            ));
        }
        text
    };
    let time = |text: &str| -> f64 {
        let started = Instant::now();
        parser
            .parse_reader(Pieces {
                bytes: text.as_bytes(),
                at: 0,
                size: 4096,
            })
            .expect("the document parses");
        started.elapsed().as_secs_f64()
    };
    let (small, large) = (document(300), document(3000));
    time(&small);
    let (one, ten) = (time(&small), time(&large));
    eprintln!(
        "window linear time, {name}: 300 messages {one:.3}s, 3000 messages {ten:.3}s, {:.1} times",
        ten / one
    );
    assert!(ten < 30.0 * one, "{one:.3}s and {ten:.3}s");
}
