// The engine's error carries a code, position, hint and a formatted
// report, so `Result<_, TabnasError>` trips clippy's `result_large_err`;
// an integration test is a crate of its own, so it allows it again.
#![allow(clippy::result_large_err)]

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole. `make()` declares `lex.window`: the nesting cap is
// counted as the lexer gives the parser its tokens, and an aggregate's
// text, which a parse of a whole text reads from the source, is kept from
// the tokens between its braces (`src/read.rs`). The keywords, match
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
// included), the tokens the lexer gives, the rule passes, and an error's
// code, position, row, column and length must be the same. An error's
// excerpt is not compared: a parse from a reader shows a long line only as
// far back as the window reaches (tabnas/parser's DIVERGENCE.md).
//
// ts/test/window.test.ts and go/window_test.go hold the TypeScript and Go
// ports to the same.

mod common;

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tabnas::{RuleState, Tabnas, TabnasError, Value, WindowSizes};
use tabnas_proto::{make, parse_tree, MAX_NESTING_DEPTH};

use common::{repo_dir, spec_dir, suite_dir};

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

/// `make()`'s parser with a log of the tokens the lexer gives and the rule
/// passes.
fn logged(log: &Log) -> Tabnas {
    let mut parser = make();
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

/// The input of every row of every fixture file of `dir`.
fn rows(dir: PathBuf, into: &mut Vec<(String, String)>) {
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "tsv"))
        .collect();
    files.sort();
    for file in files {
        let text = fs::read_to_string(&file).expect("a fixture file reads");
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        for (index, line) in text.lines().enumerate() {
            if index == 0 || line.is_empty() || (line.starts_with('#') && !line.contains('\t')) {
                continue;
            }
            let input = line.split('\t').next().unwrap_or_default();
            into.push((
                format!("{name}:{}", index + 1),
                tabnas_support::unescape(input),
            ));
        }
    }
}

/// The input of every row of the shared fixtures, every input of the
/// protoc corpus and its leniency probes, and descriptor.proto.
fn documents() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let test = repo_dir().join("test");
    rows(spec_dir(), &mut out);
    rows(test.join("passes"), &mut out);
    rows(test.join("tree"), &mut out);
    for file in ["valid", "invalid", "accept-only"] {
        let path = suite_dir().join(format!("{file}.json"));
        let cases: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("the corpus reads"))
                .expect("the corpus is JSON");
        for (index, case) in cases
            .as_array()
            .expect("a list of cases")
            .iter()
            .enumerate()
        {
            let input = case["input"].as_str().expect("a case has an input");
            out.push((format!("{file}:{index}"), input.to_string()));
        }
    }
    let leniency: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(suite_dir().join("leniency.json")).expect("the probes read"),
    )
    .expect("the probes are JSON");
    for (index, probe) in leniency["probes"]
        .as_array()
        .expect("a list of probes")
        .iter()
        .enumerate()
    {
        if let Some(input) = probe["input"].as_str() {
            out.push((format!("leniency:{index}"), input.to_string()));
        }
    }
    let descriptor = test.join("descriptor").join("descriptor.proto");
    out.push((
        "descriptor.proto".to_string(),
        fs::read_to_string(descriptor).expect("descriptor.proto reads"),
    ));
    out
}

/// A `.proto` file of every kind of token, the long ones longer than the
/// engine's 64 KiB window, with characters past ASCII in its comments and
/// strings and aggregate option values of every shape, so that tokens of
/// every kind straddle the window's edges as a reader hands the text over.
/// The same document as `longDocument` in ts/test/window.test.ts and
/// go/window_test.go.
fn long_document() -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "// {}\r\n",
        "a comment past the window, \u{e9} \u{1d11e}; ".repeat(2000)
    ));
    parts.push(format!(
        "/* {}*/\r\n",
        "a block comment past the window\r\n\t".repeat(2000)
    ));
    parts.push("syntax = \"proto2\";\r\npackage a.b.c;\r\n".into());
    parts.push(format!(
        "option long = \"{}\";\r\n",
        "a string past the window \u{fc} ".repeat(3000)
    ));
    parts.push(format!(
        "option tick = '{}';\r\n",
        "single quotes past the window ".repeat(3000)
    ));
    parts.push(format!("option (named) = {};\r\n", "n".repeat(70000)));
    parts.push(format!(
        "option (big) = {{\r\n{}}};\r\n",
        "  a: 1 /* c\t*/ b { c: \"d\" } # e\r\n\t".repeat(2000)
    ));
    parts.push(format!(
        "message {} {{ optional int32 f = 1; }}\r\n",
        "N".repeat(70000)
    ));
    for i in 0..250 {
        parts.push(format!("message M{i} {{ // r\u{e9}gle {i}\r\n"));
        parts.push(format!(
            "  optional int32 a{i} = {} [default = -0x{i:X}, (o) = {{ x: {i} y: \"z\" }}];\r\n",
            i + 1
        ));
        parts.push(format!(
            "  repeated string optionals = {};\r\n  required group G{i} = {} {{ optional bool b = 1; }}\r\n",
            i + 2,
            i + 3
        ));
        parts.push(format!(
            "  enum E{i} {{ option allow_alias = true; V{i} = 0; W{i} = 1 [deprecated = true]; }}\r\n"
        ));
        parts.push(format!(
            "  oneof o{i} {{ int64 one = {}; }}\r\n  map<string, int32> m{i} = {};\r\n",
            i + 4,
            i + 5
        ));
        parts.push(format!(
            "  extensions 100 to max;\r\n  reserved 50 to 60;\r\n  reserved \"res{i}\";\r\n}}\r\n"
        ));
        parts.push(format!(
            "service S{i} {{ rpc R{i}(stream M{i}) returns (M{i}) {{ option (x) = {{a: 1}}; }} }}\r\n"
        ));
    }
    parts.push("extend M0 { optional int32 ext = 1000; }\r\n".into());
    parts.concat()
}

#[test]
fn make_reads_a_reader_a_window_at_a_time() {
    assert!(make().options.lex.window);
}

#[test]
fn every_fixture_and_corpus_document_parses_the_same_from_a_reader() {
    let log: Log = Arc::default();
    let parser = logged(&log);
    let documents = documents();
    assert!(documents.len() > 500, "{}", documents.len());
    let (mut compared, mut aggregates) = (0, 0);
    for (index, (at, text)) in documents.iter().enumerate() {
        let reads = if text.len() > 16 * 1024 {
            BIG_READS.to_vec()
        } else {
            reads()
        };
        compared += differential(&parser, &log, at, text, &reads);
        if matches!(parser.parse(text), Ok(value) if value.to_string().contains("\"aggregate\"")) {
            aggregates += 1;
        }
        log.lock().unwrap().0.clear();
        log.lock().unwrap().1.clear();
        if index % 100 == 0 || index + 1 == documents.len() {
            eprintln!(
                "window documents: {} of {} ({}%), {compared} reader parses",
                index + 1,
                documents.len(),
                (index + 1) * 100 / documents.len()
            );
        }
    }
    assert!(aggregates >= 50, "{aggregates} documents hold an aggregate");
}

#[test]
fn a_long_document_parses_the_same_from_a_reader() {
    let log: Log = Arc::default();
    let parser = logged(&log);
    let text = long_document();
    assert!(text.len() > 512 * 1024, "{}", text.len());
    let tree = parse_tree(&text).expect("the long document parses");
    assert_eq!(tree.to_json()["body"].as_array().map(Vec::len), Some(508));
    assert_eq!(
        differential(&parser, &log, "the long document", &text, &LONG_READS),
        LONG_READS.len()
    );
}

#[test]
fn a_long_document_fails_the_same_from_a_reader() {
    // A token that runs past the engine's window and is then refused, or
    // never ends, and nesting past the cap after a long run: the error is
    // the whole text's, code, place and token.
    let log: Log = Arc::default();
    let parser = logged(&log);
    let pad = format!(
        "// {}\r\nsyntax = \"proto3\";\r\n",
        "padding \u{fc} ".repeat(9000)
    );
    let cap = MAX_NESTING_DEPTH + 1;
    let bad = [
        format!("{pad}option a = \"{}\r\n", "unclosed string ".repeat(6000)),
        format!("{pad}/* {}\r\n", "an unclosed comment ".repeat(4000)),
        format!("{pad}message M {{ {}\r\n", "int32 a = 1; ".repeat(6000)),
        format!("{pad}option b = {{{} ) }};\r\n", " c: 1".repeat(15000)),
        format!("{pad}message {} }} \r\n", "m".repeat(70000)),
        format!(
            "{pad}option (x) = {}{};\r\n",
            "{a ".repeat(cap),
            "}".repeat(cap)
        ),
        format!("{pad}{}{}", "message M{".repeat(cap), "}".repeat(cap)),
        format!("{pad}{}", "message M {".repeat(100_000)),
    ];
    let mut compared = 0;
    for (index, text) in bad.iter().enumerate() {
        let whole = seen(&log, parser.parse(text));
        assert!(
            whole.0.starts_with("err "),
            "{index}: {}",
            &whole.0[..200.min(whole.0.len())]
        );
        compared += differential(
            &parser,
            &log,
            &format!("long error document {index}"),
            text,
            &LONG_READS,
        );
    }
    assert_eq!(compared, bad.len() * LONG_READS.len());
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
    let parser = make();
    parser
        .parse(&nest(MAX_NESTING_DEPTH, "message M {"))
        .expect("the cap parses");
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
    for (text, pos) in cases {
        for from_reader in [false, true] {
            let error = if from_reader {
                parser.parse_reader(Pieces {
                    bytes: text.as_bytes(),
                    at: 0,
                    size: 64,
                })
            } else {
                parser.parse(&text)
            }
            .expect_err("past the cap");
            assert_eq!(
                (error.code.as_str(), error.pos, error.token.src.as_str()),
                ("proto_too_deep", pos, "{"),
                "{:.40} (from a reader {from_reader})",
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

#[test]
fn a_long_document_parses_from_a_reader_in_time_linear_in_its_length() {
    // Ten times the messages, read through the engine's window in pieces
    // of 4 KiB, take about ten times as long, never the square of it.
    let parser = make();
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
        "window linear time: 300 messages {one:.3}s, 3000 messages {ten:.3}s, {:.1} times",
        ten / one
    );
    assert!(ten < 30.0 * one, "{one:.3}s and {ten:.3}s");
}
