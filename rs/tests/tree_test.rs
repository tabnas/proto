// The source-ordered tree, `parse_tree`, held to the shared fixtures in
// `test/tree/*.tsv` (see `test/AGENTS.md`): each source and the tree's JSON
// text, member order included, which `ts/test/tree.test.ts` and
// `go/tree_test.go` hold the other runtimes to. The order is part of the
// contract: a host that reads the tree streams its members in that order,
// and the transducer part makes the same events.

mod common;

use tabnas_support::{Runner, Value};

#[test]
fn tree_spec() {
    let dir = common::spec_dir().join("..").join("tree");
    Runner::new(|input| {
        let tree = tabnas_proto::parse_tree(input).map_err(common::to_failure)?;
        let text = serde_json::to_string(&tree.to_json()).expect("a tree is JSON");
        Ok(Value::from(serde_json::Value::String(text)))
    })
    // The cell is the tree's JSON text, compared as text.
    .parse_expected(|expected, _row| {
        Ok(Value::from(serde_json::Value::String(expected.to_string())))
    })
    // An `ERROR:<want>` cell holds a fragment of the message: the nesting
    // cap is the plugin's own refusal, which carries no code.
    .match_error(|failure, want, _row| failure.message.contains(want))
    .dir(dir);
}
