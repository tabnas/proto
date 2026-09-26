// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! The size of the compiled grammar is a contract (tabnas/bnf#71,
//! docs/design/alt-explosion.md section 9.5): admitting every keyword as
//! an identifier used to multiply the dispatch tables into the millions.
//! Per-decision lookahead and the `ident` token class hold it to a few
//! hundred alternates. TypeScript and Go pin the same bounds.

#[test]
fn the_compiled_grammar_is_a_few_hundred_alternates() {
    let parser = tabnas_proto::make();
    let rules = parser.rule_specs();
    let total: usize = rules.iter().map(|spec| spec.open.len()).sum();
    let (biggest, biggest_n) = rules
        .iter()
        .map(|spec| (spec.name.as_str(), spec.open.len()))
        .max_by_key(|(_, n)| *n)
        .unwrap_or(("", 0));
    assert!(rules.len() <= 500, "{} rules", rules.len());
    assert!(total <= 1000, "{total} open alternates");
    assert!(biggest_n <= 60, "{biggest} has {biggest_n} open alternates");
    assert!(
        parser.token_set("ident").is_some(),
        "the identifier class is not a token set"
    );
}
