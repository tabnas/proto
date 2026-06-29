/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Protobuf version detection and option reconciliation.
// Go port of ts/src/detect-version.ts.

package tabnasproto

import (
	"fmt"
	"regexp"
)

// ProtoVersion is one of "proto2", "proto3", "2023", "2024".
type ProtoVersion = string

var (
	syntaxVersions  = map[string]bool{"proto2": true, "proto3": true}
	editionVersions = map[string]bool{"2023": true, "2024": true}
	declRe          = regexp.MustCompile(`^(syntax|edition)=["']([^"']+)["']`)
)

// declaredVersion pulls the declared version out of a `syntaxOrEdition` CST
// node, or "" if the file has no leading syntax/edition declaration. The
// node's src is whitespace-stripped, e.g. `syntax="proto3";`. It returns an
// error for a recognised keyword carrying an unknown version value.
func declaredVersion(syntaxNode map[string]any) (ProtoVersion, error) {
	if syntaxNode == nil {
		return "", nil
	}
	m := declRe.FindStringSubmatch(nsrc(syntaxNode))
	if m == nil {
		return "", nil
	}
	value := m[2]
	if syntaxVersions[value] || editionVersions[value] {
		return value, nil
	}
	return "", fmt.Errorf("proto: unknown %s version %q", m[1], value)
}

// resolveVersion reconciles the version declared in the source with the
// version supplied via the plugin option. With reconcile true (the default) a
// mismatch is an error; otherwise the declaration wins when present. Falls
// back to proto2 (protoc's default for a file with no declaration/option).
func resolveVersion(declared, option ProtoVersion, reconcile bool) (ProtoVersion, error) {
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

// isEdition reports whether v is an edition version (2023 / 2024).
func isEdition(v ProtoVersion) bool {
	return editionVersions[v]
}
