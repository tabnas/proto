// The rewind history this crate's parsers keep: the engine's default, 64
// consumed tokens. The compiled grammar has no probe, the one builtin that
// rewinds, so no parse rewinds; the one reader of the history is the action
// that records an aggregate's text, which reads the last token consumed.
// Every document the window test reads parses alike at 64 and at 8,192, the
// bound this crate kept through 0.7.0, a longer history only holding a
// window's text back to its oldest token. ts/test/history.test.ts and
// go/history_test.go hold the TypeScript and Go ports to the same.

mod common;

use tabnas::{Options as EngineOptions, RewindOptions, Tabnas};
use tabnas_proto::{make, make_streaming, proto, REWIND_HISTORY};

use common::{every_document, repo_dir};

/// A parse of `text` on `parser`: its CST, or its error's code, place and
/// token.
fn outcome(parser: &Tabnas, text: &str) -> String {
    match parser.parse(text) {
        Ok(value) => format!("ok {value}"),
        Err(e) => format!(
            "err {} at {} {}:{} {:?}",
            e.code, e.pos, e.row, e.col, e.token.src
        ),
    }
}

/// An engine with this crate's grammar and a rewind history of `history`.
fn parser(history: usize) -> Tabnas {
    let mut parser = Tabnas::with_options(EngineOptions {
        rewind: RewindOptions {
            history: Some(history),
        },
        ..EngineOptions::default()
    });
    proto(&mut parser).expect("the grammar installs");
    parser
}

#[test]
fn the_grammar_has_no_probe_and_the_parsers_keep_the_default_history() {
    let spec = std::fs::read_to_string(repo_dir().join("rs").join("proto-grammar.json"))
        .expect("the compiled grammar reads");
    assert!(!spec.contains("@probe"), "a probe builtin");
    assert!(!spec.contains("\"pd_"), "a probe key");
    assert_eq!(REWIND_HISTORY, RewindOptions::default().history.unwrap());
    assert_eq!(REWIND_HISTORY, 64);
    assert_eq!(make().options.rewind.history, Some(64));
    assert_eq!(make_streaming().options.rewind.history, Some(64));
    // Both bound the rule history, the engine's default here.
    assert_eq!(make_streaming().options.rule.history, Some(3));
    assert_eq!(make().options.rule.history, Some(3));
}

#[test]
fn every_document_parses_alike_at_the_default_history_and_at_8192() {
    let documents = every_document();
    let (short, long) = (parser(REWIND_HISTORY), parser(8192));
    for (index, (at, text)) in documents.iter().enumerate() {
        assert_eq!(outcome(&short, text), outcome(&long, text), "{at}");
        if index % 100 == 0 || index + 1 == documents.len() {
            eprintln!(
                "history: {} of {} documents ({}%)",
                index + 1,
                documents.len(),
                (index + 1) * 100 / documents.len()
            );
        }
    }
}
