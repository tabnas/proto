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
}

//go:embed translate/manifest.json
var translationManifest string

//go:embed translate/render.alc
var translationRender string

var translationParts = TranslationParts{
	Manifest: translationManifest,
	Render:   &TranslationPart{Entry: "proto-render", Source: translationRender},
}

// Translate returns the translation parts of .proto files: the manifest,
// and the render that writes a FileDescriptorProto, the reader's tree, back
// as a .proto file. There is no lift and no embed: the tree is the
// descriptor's own shape, the schema proto-descriptor.
func Translate() *TranslationParts { return &translationParts }
