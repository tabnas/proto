/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Package tabnasproto parses Protocol Buffers .proto IDL (proto2, proto3,
// edition 2023/2024) into FileDescriptorProto-shaped Go values. It drives the
// Tabnas engine with an ABNF grammar rather than a hand-written parser: the
// grammar is compiled by github.com/tabnas/abnf/go at build time (go
// generate) and embedded, so the package imports no compiler. The version
// is auto-detected from the file's syntax/edition declaration and/or
// supplied via ProtoOptions.
//
// Go port of ts/src/proto.ts.
package tabnasproto

import (
	_ "embed"
	"fmt"

	tabnas "github.com/tabnas/parser/go"
)

// VERSION is this module's version. It MUST equal ts/package.json
// "version": the release orchestrator rewrites both, and
// TestVersionMatchesPackageJSON fails the build if they drift.
const VERSION = "0.7.0"

//go:generate go run grammar_gen.go

// grammarSpec is the compiled grammar: proto-grammar.json, the engine's
// serialized rule set, which grammar_gen.go compiles from GrammarText with
// github.com/tabnas/abnf/go when `go generate` runs. Generated: never edit
// it; grammar_spec_test.go fails when it is stale.
//
//go:embed proto-grammar.json
var grammarSpec []byte

// ProtoOptions configures descriptor construction.
type ProtoOptions struct {
	// Version forces the protobuf version. "" (the default) auto-detects from
	// the file's syntax/edition declaration.
	Version ProtoVersion
	// Reconcile, when nil (the default) or true, errors if an explicit Version
	// disagrees with the file's declaration; when false the declaration wins.
	Reconcile *bool
}

// Proto installs the union proto grammar onto j so it can parse .proto source
// into a {rule, src, kids} CST. Use ToDescriptor to turn that CST into a
// FileDescriptorProto. Mirrors the TS `tn.use(Proto)` plugin.
func Proto(j *tabnas.Tabnas) error {
	// The union grammar, compiled at build time by grammar_gen.go with the
	// options this function used to pass at every install. WordKeywords is
	// required so literal keywords match as whole words (e.g. `option` does
	// not grab the `option` prefix of `optional`); TokenClasses compiles
	// `ident` (an identifier or any keyword) to one engine token set, so a
	// lookahead position peeks it as one token rather than one alternate
	// per keyword. The CST builders are the engine's own @node$ / @capture$
	// / @bubble$ builtins, so the file is pure data and installs through
	// the engine's serialized-grammar door, with no compiler.
	gs, err := tabnas.GrammarSpecFromJSON(grammarSpec)
	if err != nil {
		return fmt.Errorf("proto: the compiled grammar failed to load: %w", err)
	}
	if err := j.Grammar(gs); err != nil {
		return err
	}
	// An aggregate value (`option (f) = { a: 1 };`) is recorded as the text
	// between its braces, which the CST's src does not keep: the lexer drops
	// whitespace and comments. This action reads it from the source while
	// the brace tokens are to hand; see aggregate.go.
	j.Rule("constant", func(rs *tabnas.RuleSpec, _ *tabnas.Parser) {
		rs.AddAC(recordAggregate)
	})
	// The nesting cap, counted as the lexer gives the parser its tokens,
	// and in a parse from a reader an aggregate's text: see read.go.
	j.SetOptions(tabnas.Options{
		Error: map[string]string{tooDeep: tooDeepMessage},
		Hint:  map[string]string{tooDeep: tooDeepHint},
	})
	j.Sub(readToken, nil)
	startUndefined(j)
	return nil
}

// startUndefined gives the start rule the engine's Undefined as its node.
// The Go engine starts a parse with a nil node, which a pass carries as a
// null value (transduce's parser-event source), where TypeScript's and
// Rust's start with no node at all. The start rule closes with the CST
// lifted into its node, but on a document that fails it closes with the
// node it began with, and that pass carried a null here and nothing in the
// canonical runtime; and the parser MakeStreaming builds makes no node, so
// every rule there would carry one. With Undefined, a rule carries a value
// in neither parser, as in TypeScript and Rust.
func startUndefined(j *tabnas.Tabnas) {
	j.Rule("__start__", func(rs *tabnas.RuleSpec, _ *tabnas.Parser) {
		rs.AddBO(func(r *tabnas.Rule, _ *tabnas.Context) { r.Node = tabnas.Undefined })
	})
}

// treeBuilders are the compiled grammar's tree builders, the engine's
// builtins that build the {rule, src, kids} CST, each with the member of an
// alternate's K that configures it, if any.
var treeBuilders = map[string]string{
	"@node$":    "node$",
	"@capture$": "capture$",
	"@bubble$":  "",
}

// streamingGrammar is the compiled grammar without its tree builders, which
// the parser MakeStreaming builds installs: the same rules, alternates,
// tokens and counters, so the same rule passes, and no node built. A
// builder is an alternate's action, which runs after the alternate has
// matched, and the grammar's conditions read counters, never a node, so
// taking the builders away changes what a parse builds and nothing of how
// it goes. Port of withoutTree in ts/src/proto.ts.
func streamingGrammar() (*tabnas.GrammarSpec, error) {
	gs, err := tabnas.GrammarSpecFromJSON(grammarSpec)
	if err != nil {
		return nil, fmt.Errorf("proto: the compiled grammar failed to load: %w", err)
	}
	for _, rule := range gs.Rule {
		if rule == nil {
			continue
		}
		for _, state := range []any{rule.Open, rule.Close} {
			for _, alt := range alternates(state) {
				ref, _ := alt.A.(string)
				config, builder := treeBuilders[ref]
				if !builder {
					continue
				}
				alt.A = nil
				if config != "" && alt.K != nil {
					delete(alt.K, config)
					if len(alt.K) == 0 {
						alt.K = nil
					}
				}
			}
		}
	}
	return gs, nil
}

// alternates are the alternates a rule state of a grammar spec holds, in
// either of the shapes the engine reads.
func alternates(state any) []*tabnas.GrammarAltSpec {
	switch v := state.(type) {
	case []*tabnas.GrammarAltSpec:
		return v
	case *tabnas.GrammarAltListSpec:
		if v != nil {
			return v.Alts
		}
	}
	return nil
}

// ToDescriptor turns a parsed proto CST into a FileDescriptorProto, resolving
// the version from the file's declaration and the supplied options.
func ToDescriptor(cst any, opts *ProtoOptions) (FileDescriptorProto, error) {
	file, _, err := toDescriptor(cst, opts)
	return file, err
}

// toDescriptor is ToDescriptor, with the walk's order record (order.go).
func toDescriptor(cst any, opts *ProtoOptions) (FileDescriptorProto, *order, error) {
	version := ProtoVersion("")
	reconcile := true
	if opts != nil {
		version = opts.Version
		if opts.Reconcile != nil {
			reconcile = *opts.Reconcile
		}
	}

	root, _ := cst.(map[string]any)
	var first map[string]any
	for _, k := range nkids(root) {
		if nrule(k) != "" {
			first = k
			break
		}
	}
	declared := ProtoVersion("")
	if first != nil && nrule(first) == "syntaxOrEdition" {
		d, err := DeclaredVersion(first)
		if err != nil {
			return FileDescriptorProto{}, nil, err
		}
		declared = d
	}

	resolved, err := ResolveVersion(declared, version, reconcile)
	if err != nil {
		return FileDescriptorProto{}, nil, err
	}
	file, rec := buildFile(root, resolved)
	return file, rec, nil
}

// Parse parses a .proto source string to a FileDescriptorProto in one call.
// It builds a fresh engine each time; for repeated parsing reuse Make's
// parser and call ToDescriptor(j.Parse(src)).
//
// A document nesting deeper than MaxNestingDepth is refused before the
// engine runs, the message naming its depth: see Preflight. The engine
// refuses one too, as it reads the brace past the cap (read.go).
func Parse(src string, opts *ProtoOptions) (FileDescriptorProto, error) {
	cst, err := parseCST(src)
	if err != nil {
		return FileDescriptorProto{}, err
	}
	return ToDescriptor(cst, opts)
}

// parseCST runs Preflight, then a fresh engine with Proto installed.
func parseCST(src string) (any, error) {
	if err := Preflight(src); err != nil {
		return nil, err
	}
	j, err := Make()
	if err != nil {
		return nil, err
	}
	return j.Parse(src)
}
