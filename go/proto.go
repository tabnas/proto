/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Package tabnasproto parses Protocol Buffers .proto IDL (proto2, proto3,
// edition 2023/2024) into FileDescriptorProto-shaped Go values. It drives the
// Tabnas engine with an ABNF grammar (via @tabnas/abnf) rather than a
// hand-written parser. The version is auto-detected from the file's
// syntax/edition declaration and/or supplied via ProtoOptions.
//
// Go port of ts/src/proto.ts.
package tabnasproto

import (
	abnf "github.com/tabnas/abnf/go"
	tabnas "github.com/tabnas/parser/go"
)

const Version = "0.2.1"

//go:generate go run grammar_gen.go

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
	// Proto drives parsing through @tabnas/abnf. wordKeywords is required so
	// literal keywords match as whole words (e.g. `option` does not grab the
	// `option` prefix of `optional`).
	_, err := abnf.Install(j, GrammarText, &abnf.AbnfConvertOptions{
		Tag:          "proto",
		Start:        "proto",
		WordKeywords: true,
	}, nil)
	return err
}

// ToDescriptor turns a parsed proto CST into a FileDescriptorProto, resolving
// the version from the file's declaration and the supplied options.
func ToDescriptor(cst any, opts *ProtoOptions) (FileDescriptorProto, error) {
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
			return FileDescriptorProto{}, err
		}
		declared = d
	}

	resolved, err := ResolveVersion(declared, version, reconcile)
	if err != nil {
		return FileDescriptorProto{}, err
	}
	return BuildFile(root, resolved), nil
}

// Parse parses a .proto source string to a FileDescriptorProto in one call.
// It builds a fresh engine each time; for repeated parsing reuse an engine:
// build one with tabnas.Make, install Proto, then call ToDescriptor(j.Parse(src)).
func Parse(src string, opts *ProtoOptions) (FileDescriptorProto, error) {
	rh := 8192
	j := tabnas.Make(tabnas.Options{Rewind: &tabnas.RewindOptions{History: &rh}})
	if err := Proto(j); err != nil {
		return FileDescriptorProto{}, err
	}
	cst, err := j.Parse(src)
	if err != nil {
		return FileDescriptorProto{}, err
	}
	return ToDescriptor(cst, opts)
}
