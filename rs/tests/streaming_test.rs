// The engine's error carries a code, position, hint and a formatted
// report, so `Result<_, TabnasError>` trips clippy's `result_large_err`;
// an integration test is a crate of its own, so it allows it again.
#![allow(clippy::result_large_err)]

// The parser a host streams a `.proto` document through, `make_streaming()`,
// against the parser for a tree, `make()`. Its grammar is the compiled one
// without the CST's builders, so it builds no tree, and its rule passes,
// which the transducer part (`alchemy/transduce.alc`) reads, must be
// `make()`'s: over every document the window test reads, every pass alike,
// its rule, state, depth, alternate, counters and tokens, and whether the
// rule's node is a value a pass carries, and every parse ending alike, an
// error at the same code, place and token. ts/test/streaming.test.ts and
// go/streaming_test.go hold the TypeScript and Go ports to the same.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tabnas::{RuleState, Tabnas, Value};
use tabnas_proto::{make, make_streaming, nrule};

use common::every_document;

type Log = Arc<Mutex<Vec<String>>>;

/// A parser `build` makes, with `log` taking every pass of its parses.
fn logging(log: &Log, build: fn() -> Tabnas) -> Tabnas {
    let mut parser = build();
    let passes = Arc::clone(log);
    parser.subscribe_rule_done(move |rule, _context, done| {
        let close = done.state == RuleState::Close;
        let tokens = if close { &rule.c } else { &rule.o };
        let tokens: Vec<String> = tokens
            .iter()
            .map(|t| format!("{} {:?} {}", t.name, t.src, t.site.si))
            .collect();
        let alt = done.alt.as_ref().map_or("-".to_string(), |alt| {
            format!("{}/{}/{}/{}", alt.p, alt.r, alt.b, alt.g.join(","))
        });
        // Whether the pass carries the rule's node as a value, as
        // transduce's parser-event source asks it: a close pass does, where
        // the node is a scalar or a null.
        let carried = close
            && matches!(
                &*rule.node.borrow(),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Text(_)
            );
        let mut counters: Vec<String> = rule.n.iter().map(|(k, v)| format!("{k}={v}")).collect();
        counters.sort();
        passes.lock().unwrap().push(format!(
            "{} | {} | {} | {} | {alt} | {} | {} | {} | {carried} | {}",
            rule.name.as_str(),
            if close { "c" } else { "o" },
            rule.d,
            rule.i,
            done.forced,
            done.probe,
            counters.join(","),
            tokens.join(", ")
        ));
    });
    parser
}

/// Every pass of a parse of `text` on `parser`, and how the parse ended.
fn passes(parser: &Tabnas, log: &Log, text: &str) -> Vec<String> {
    let end = match parser.parse(text) {
        Ok(_) => "ok".to_string(),
        Err(e) => format!(
            "err {} at {} {}:{} {:?}",
            e.code, e.pos, e.row, e.col, e.token.src
        ),
    };
    let mut passes = std::mem::take(&mut *log.lock().unwrap());
    passes.push(end);
    passes
}

#[test]
fn make_streaming_builds_no_tree() {
    let text = "syntax = \"proto3\";\nmessage M { int32 a = 1 [(o) = { x: 1 }]; }\n";
    // The engine hands back a parse that built no value as a null.
    assert_eq!(
        make_streaming().parse(text).expect("it parses"),
        Value::Null
    );
    assert_eq!(nrule(&make().parse(text).expect("it parses")), "proto");
}

#[test]
fn make_streamings_passes_are_makes_over_every_document() {
    let documents = every_document();
    assert!(documents.len() > 500, "{}", documents.len());
    let (tree_log, stream_log): (Log, Log) = (Arc::default(), Arc::default());
    let tree = logging(&tree_log, make);
    let stream = logging(&stream_log, make_streaming);
    let (mut compared, mut failed) = (0, 0);
    let mut reported = Instant::now();
    for (index, (at, text)) in documents.iter().enumerate() {
        let want = passes(&tree, &tree_log, text);
        let got = passes(&stream, &stream_log, text);
        assert_eq!(got.len(), want.len(), "{at}: the count of passes");
        for (i, (got, want)) in got.iter().zip(&want).enumerate() {
            assert_eq!(got, want, "{at}: pass {i}");
        }
        compared += want.len();
        if want.last().is_some_and(|end| end.starts_with("err ")) {
            failed += 1;
        }
        if reported.elapsed().as_secs() >= 20 || index + 1 == documents.len() {
            reported = Instant::now();
            eprintln!(
                "streaming passes: {} of {} documents ({}%)",
                index + 1,
                documents.len(),
                (index + 1) * 100 / documents.len()
            );
        }
    }
    assert!(failed > 50, "{failed} documents fail");
    eprintln!(
        "streaming passes: {} documents, {failed} failing, {compared} passes alike",
        documents.len()
    );
}
