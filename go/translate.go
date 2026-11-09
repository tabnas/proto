/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

import _ "embed"

// TranslationPart is one optional alchemy source and the entry point a host calls.
type TranslationPart struct {
	Entry  string
	Source string
}

// TranslationParts is the package-local structural translation interface.
type TranslationParts struct {
	Manifest string
	Lift     *TranslationPart
	Embed    *TranslationPart
	Render   *TranslationPart
	// Transduce is the transducer part: the source-ordered tree's JSON
	// events from the grammar's rule passes, which a host that reads parser
	// events reads a .proto file through.
	Transduce *TranslationPart
}

//go:embed translate/manifest.json
var translationManifest string

//go:embed translate/render.alc
var translationRender string

//go:embed translate/transduce.alc
var translationTransduce string

var translationParts = TranslationParts{
	Manifest:  translationManifest,
	Render:    &TranslationPart{Entry: "proto-render", Source: translationRender},
	Transduce: &TranslationPart{Entry: "proto-transduce", Source: translationTransduce},
}

// Translate returns the translation parts of .proto files: the manifest,
// the render that writes the source-ordered tree (the schema proto-ast,
// ParseTree's) back as a .proto file, and the transducer part,
// proto-transduce, that makes the same tree's JSON events from the
// grammar's rule passes. There is no lift and no embed: the tree is the
// format's own shape, which a host renders only from a source of the same
// schema or from a program that builds the tree.
// Each call returns a copy of its own, so that what one caller changes
// is not what another reads.
func Translate() *TranslationParts {
	parts := translationParts
	parts.Lift = copyPart(parts.Lift)
	parts.Embed = copyPart(parts.Embed)
	parts.Render = copyPart(parts.Render)
	parts.Transduce = copyPart(parts.Transduce)
	return &parts
}

// copyPart is a part of its own, so that no caller reaches another's.
func copyPart(part *TranslationPart) *TranslationPart {
	if part == nil {
		return nil
	}
	copied := *part
	return &copied
}
