// The rule passes the transducer part (`alchemy/transduce.alc`) reads a
// statement by, held to the shared fixtures in `test/passes/*.tsv`, as
// `ts/test/passes.test.ts` holds the TypeScript port and
// `go/passes_test.go` the Go one; and the ten-thousand items test:
// repetition is replacement, so the items of a list add no rule depth
// however many there are.

mod common;

use std::sync::{Arc, Mutex};

use tabnas::RuleState;
use tabnas_support::{Runner, Value};

fn is_statement(name: &str) -> bool {
    let dispatch = [
        "syntaxOrEdition",
        "topLevelDef",
        "messageElement",
        "enumElement",
        "oneofElement",
        "serviceElement",
    ];
    if let Some((rule, n)) = name.split_once("$alt") {
        return dispatch.contains(&rule) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
    }
    ["field", "optionStmt", "fieldOption", "messageValueEntry"].contains(&name)
}

/// The names of the statements' rules, in the order their open passes
/// come, and `constant$alt5` where a constant opens an aggregate.
fn statements(src: &str) -> Result<Vec<String>, tabnas_proto::ProtoError> {
    let mut parser = tabnas_proto::make();
    let names = Arc::new(Mutex::new(Vec::new()));
    let seen = names.clone();
    parser.subscribe_rule_done(move |rule, _ctx, done| {
        if done.probe || done.state != RuleState::Open {
            return;
        }
        let name = rule.name.as_str();
        let mut seen = seen.lock().expect("one parse at a time");
        if is_statement(name) {
            seen.push(name.to_string());
        }
        if "constant" == name
            && done
                .alt
                .as_ref()
                .is_some_and(|alt| "constant$alt5" == alt.p)
        {
            seen.push("constant$alt5".to_string());
        }
    });
    parser.parse(src)?;
    let found = names.lock().expect("the parse is done").clone();
    Ok(found)
}

#[test]
fn passes_spec() {
    let dir = common::spec_dir().join("..").join("passes");
    Runner::new(|input| {
        let names = statements(input).map_err(common::to_failure)?;
        let text = serde_json::to_string(&names).expect("names are JSON");
        Ok(Value::from(serde_json::Value::String(text)))
    })
    .parse_expected(|expected, _row| {
        Ok(Value::from(serde_json::Value::String(expected.to_string())))
    })
    .dir(dir);
}

/// The deepest rule a parse of `src` reaches.
fn depth(src: &str) -> usize {
    let mut parser = tabnas_proto::make();
    let deepest = Arc::new(Mutex::new(0usize));
    let seen = deepest.clone();
    parser.subscribe_rule_done(move |rule, _ctx, done| {
        let mut seen = seen.lock().expect("one parse at a time");
        if !done.probe && rule.d > *seen {
            *seen = rule.d;
        }
    });
    parser.parse(src).expect("the document parses");
    let found = *deepest.lock().expect("the parse is done");
    found
}

/// A document of `n` items.
type Document = fn(usize) -> String;

fn items(n: usize, item: impl Fn(usize) -> String, sep: &str) -> String {
    (0..n).map(item).collect::<Vec<_>>().join(sep)
}

#[test]
fn repetition_adds_no_rule_depth() {
    const N: usize = 10_000;
    let repeated: [(&str, Document); 6] = [
        ("declarations of a file", |n| "message M {}\n".repeat(n)),
        ("fields of a message", |n| {
            format!(
                "message M {{\n{}}}\n",
                items(n, |i| format!("  int32 f{i} = {};\n", i + 1), "")
            )
        }),
        ("values of an enum", |n| {
            format!(
                "enum E {{\n{}}}\n",
                items(n, |i| format!("  V{i} = {i};\n"), "")
            )
        }),
        ("fields of an aggregate", |n| {
            format!("option (a) = {{{} }};\n", " b: 1".repeat(n))
        }),
        ("options in brackets", |n| {
            format!(
                "message M {{ int32 f = 1 [{}]; }}\n",
                items(n, |i| format!("(o{i}) = 1"), ", ")
            )
        }),
        ("ranges of a reserved statement", |n| {
            format!(
                "message M {{ reserved {}; }}\n",
                items(n, |i| format!("{}", i + 1), ", ")
            )
        }),
    ];
    // Two items, since a list's first item can sit apart from its repeated
    // ones (`x *( "," x )`): the loop's own item is the second.
    for (what, doc) in repeated {
        assert_eq!(
            depth(&doc(N)),
            depth(&doc(2)),
            "the {what}: {N} reach a deeper rule than two"
        );
    }
}
