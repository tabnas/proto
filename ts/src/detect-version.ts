/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Protobuf version detection and option reconciliation.

export type ProtoVersion = 'proto2' | 'proto3' | '2023' | '2024'

const SYNTAX_VERSIONS = new Set(['proto2', 'proto3'])
const EDITION_VERSIONS = new Set(['2023', '2024'])

// Pull the declared version out of a `syntaxOrEdition` CST node, or null
// if the file has no leading `syntax`/`edition` declaration. The node's
// `src` is whitespace-stripped, e.g. `syntax="proto3";` or `edition="2023";`.
export function declaredVersion(syntaxNode: any): ProtoVersion | null {
  if (!syntaxNode || 'string' !== typeof syntaxNode.src) return null
  const m = syntaxNode.src.match(/^(syntax|edition)=["']([^"']+)["']/)
  if (!m) return null
  const value = m[2]
  if (SYNTAX_VERSIONS.has(value) || EDITION_VERSIONS.has(value)) {
    return value as ProtoVersion
  }
  throw new Error(`proto: unknown ${m[1]} version "${value}"`)
}

// Reconcile the version declared in the source with the version supplied
// via the plugin option. With `reconcile` true (the default) a mismatch is
// an error; otherwise the declaration wins when present. Falls back to
// proto2 (protoc's default for a file with no declaration and no option).
export function resolveVersion(
  declared: ProtoVersion | null,
  option: ProtoVersion | null,
  reconcile: boolean,
): ProtoVersion {
  if (null != declared && null != option && declared !== option) {
    if (reconcile) {
      throw new Error(
        `proto: version mismatch — option "${option}" but the file ` +
        `declares "${declared}". Set reconcile:false to let the file win.`,
      )
    }
    return declared
  }
  return declared ?? option ?? 'proto2'
}

export function isEdition(v: ProtoVersion): boolean {
  return EDITION_VERSIONS.has(v)
}

// FileDescriptorProto records syntax files via `syntax` and edition files
// via `edition` (as `EDITION_2023` / `EDITION_2024`).
export function editionEnum(v: ProtoVersion): string {
  return 'EDITION_' + v
}
