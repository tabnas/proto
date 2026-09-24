/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Protobuf version detection and option reconciliation.
// Go port of ts/src/detect-version.ts.

package tabnasproto

import (
	"fmt"
	"regexp"
	"strings"
)

// ProtoVersion is one of "proto2", "proto3", "2023", "2024".
type ProtoVersion = string

var (
	syntaxVersions  = map[string]bool{"proto2": true, "proto3": true}
	editionVersions = map[string]bool{"2023": true, "2024": true}
	declRe          = regexp.MustCompile(`^(syntax|edition)=["']([^"']+)["']`)
)

// DeclaredVersion pulls the declared version out of a `syntaxOrEdition` CST
// node, or "" if the file has no leading syntax/edition declaration. The
// node's src is whitespace-stripped, e.g. `syntax="proto3";`. It returns an
// error for a recognised keyword carrying an unknown version value.
//
// The version may be written as adjacent literals, `syntax = "pro" "to3";`,
// which protoc reads as the one string they concatenate to (strings.go). A
// single literal is read from src as it always has been.
// Go counterpart of the TS `declaredVersion` (ts/src/detect-version.ts).
func DeclaredVersion(syntaxNode map[string]any) (ProtoVersion, error) {
	if syntaxNode == nil {
		return "", nil
	}
	var literals strings.Builder
	count := 0
	for _, k := range childRules(syntaxNode) {
		if nrule(k) == "strLit" {
			literals.WriteString(nsrc(k))
			count++
		}
	}
	if count > 1 {
		kind := "syntax"
		if strings.HasPrefix(nsrc(syntaxNode), "edition") {
			kind = "edition"
		}
		value, ok := adjacentValue(literals.String(), false)
		if !ok {
			return "", nil
		}
		return knownVersion(kind, value)
	}
	m := declRe.FindStringSubmatch(nsrc(syntaxNode))
	if m == nil {
		return "", nil
	}
	return knownVersion(m[1], m[2])
}

func knownVersion(kind, value string) (ProtoVersion, error) {
	if syntaxVersions[value] || editionVersions[value] {
		return value, nil
	}
	return "", fmt.Errorf("proto: unknown %s version %q", kind, value)
}

// ResolveVersion reconciles the version declared in the source with the
// version supplied via the plugin option. With reconcile true (the default) a
// mismatch is an error; otherwise the declaration wins when present. Falls
// back to proto2 (protoc's default for a file with no declaration/option).
// Go counterpart of the TS `resolveVersion` (ts/src/detect-version.ts).
func ResolveVersion(declared, option ProtoVersion, reconcile bool) (ProtoVersion, error) {
	if declared != "" && option != "" && declared != option {
		if reconcile {
			return "", fmt.Errorf(
				"proto: version mismatch — option %q but the file declares %q. "+
					"Set Reconcile:false to let the file win.", option, declared)
		}
		return declared, nil
	}
	if declared != "" {
		return declared, nil
	}
	if option != "" {
		return option, nil
	}
	return "proto2", nil
}

// IsEdition reports whether v is an edition version (2023 / 2024).
// Go counterpart of the TS `isEdition` (ts/src/detect-version.ts).
func IsEdition(v ProtoVersion) bool {
	return editionVersions[v]
}

// EditionEnum returns the FileDescriptorProto edition enum name for an
// edition version, e.g. "EDITION_2023" for "2023". FileDescriptorProto
// records syntax files via Syntax and edition files via Edition.
// Go counterpart of the TS `editionEnum` (ts/src/detect-version.ts).
func EditionEnum(v ProtoVersion) string {
	return "EDITION_" + v
}
