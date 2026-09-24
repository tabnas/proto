/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! Protobuf version detection and option reconciliation.
//!
//! Rust port of `ts/src/detect-version.ts`.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use tabnas::Value;

use crate::error::ProtoError;
use crate::node::nsrc;
use crate::strings::adjacent_value;

/// The protobuf version a file is read as.
///
/// The canonical runtime spells this as the string union
/// `'proto2' | 'proto3' | '2023' | '2024'`, and [`ProtoVersion::as_str`]
/// gives the same four strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ProtoVersion {
    #[serde(rename = "proto2")]
    Proto2,
    #[serde(rename = "proto3")]
    Proto3,
    #[serde(rename = "2023")]
    Edition2023,
    #[serde(rename = "2024")]
    Edition2024,
}

impl ProtoVersion {
    /// The canonical spelling: `proto2`, `proto3`, `2023` or `2024`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProtoVersion::Proto2 => "proto2",
            ProtoVersion::Proto3 => "proto3",
            ProtoVersion::Edition2023 => "2023",
            ProtoVersion::Edition2024 => "2024",
        }
    }

    /// The version a declaration value names, or `None` for anything
    /// outside the four this package supports.
    pub fn from_declared(value: &str) -> Option<ProtoVersion> {
        match value {
            "proto2" => Some(ProtoVersion::Proto2),
            "proto3" => Some(ProtoVersion::Proto3),
            "2023" => Some(ProtoVersion::Edition2023),
            "2024" => Some(ProtoVersion::Edition2024),
            _ => None,
        }
    }
}

impl fmt::Display for ProtoVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProtoVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        ProtoVersion::from_declared(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("proto: unknown version {:?}", text)))
    }
}

/// Whether `version` is an edition version (2023 or 2024).
pub fn is_edition(version: ProtoVersion) -> bool {
    matches!(
        version,
        ProtoVersion::Edition2023 | ProtoVersion::Edition2024
    )
}

/// The `FileDescriptorProto` edition enum name for a version, such as
/// `EDITION_2023`.
///
/// A syntax file records `syntax`; an edition file records both
/// `syntax: "editions"` and this.
pub fn edition_enum(version: ProtoVersion) -> String {
    format!("EDITION_{}", version.as_str())
}

/// The declared version in a `syntaxOrEdition` CST node, or `None` when
/// the file has no leading `syntax` or `edition` declaration.
///
/// The node's `src` is whitespace-stripped, for example
/// `syntax="proto3";` or `edition="2023";`.
///
/// The version may be written as adjacent literals, `syntax = "pro" "to3";`,
/// which protoc reads as the one string they concatenate to
/// (`strings.rs`). A single literal is read from `src` as it always has
/// been.
pub fn declared_version(node: &Value) -> Result<Option<ProtoVersion>, ProtoError> {
    declared_version_src(nsrc(node))
}

/// [`declared_version`] over the node's `src` directly.
///
/// The canonical runtime answers `null` for a node with no string `src`,
/// which here is the empty string and the same answer. The canonical
/// runtime reads adjacent literals from the node's `strLit` children; their
/// text is the whole of `src` between the `=` and the closing `;`, which is
/// where this reads them.
pub fn declared_version_src(src: &str) -> Result<Option<ProtoVersion>, ProtoError> {
    // The canonical `/^(syntax|edition)=["']([^"']+)["']/`, written out:
    // the regexp crate reads a character class Unicode-aware where this
    // one is plain ASCII, and the whole pattern is four literal steps.
    let Some((keyword, rest)) = keyword_of(src) else {
        return Ok(None);
    };
    let Some(rest) = rest.strip_prefix('=') else {
        return Ok(None);
    };
    if let Some(value) = adjacent_value(rest.strip_suffix(';').unwrap_or(rest), false) {
        return known_version(keyword, &value);
    }
    let mut chars = rest.char_indices();
    match chars.next() {
        Some((_, '"' | '\'')) => {}
        _ => return Ok(None),
    }
    // `[^"']+` is greedy and the closing `["']` can only land where the
    // run stops, so there is no backtracking to reproduce: scan to the
    // first quote of either kind and require at least one character
    // before it.
    let body = &rest[1..];
    let Some(end) = body.find(['"', '\'']) else {
        return Ok(None);
    };
    if 0 == end {
        return Ok(None);
    }
    known_version(keyword, &body[..end])
}

/// The version `value` names, or the error for one this package does not
/// know.
fn known_version(keyword: &str, value: &str) -> Result<Option<ProtoVersion>, ProtoError> {
    match ProtoVersion::from_declared(value) {
        Some(version) => Ok(Some(version)),
        None => Err(ProtoError::Version(format!(
            "proto: unknown {keyword} version \"{value}\""
        ))),
    }
}

/// The `(syntax|edition)` alternation at the head of `src`.
fn keyword_of(src: &str) -> Option<(&'static str, &str)> {
    if let Some(rest) = src.strip_prefix("syntax") {
        return Some(("syntax", rest));
    }
    src.strip_prefix("edition").map(|rest| ("edition", rest))
}

/// Reconcile the version declared in the source with the version supplied
/// through the options.
///
/// With `reconcile` true (the default) a mismatch is an error; otherwise
/// the declaration wins when present. Falls back to proto2, which is what
/// protoc assumes for a file with neither.
pub fn resolve_version(
    declared: Option<ProtoVersion>,
    option: Option<ProtoVersion>,
    reconcile: bool,
) -> Result<ProtoVersion, ProtoError> {
    if let (Some(declared), Some(option)) = (declared, option) {
        if declared != option {
            if reconcile {
                return Err(ProtoError::Version(format!(
                    "proto: version mismatch \u{2014} option \"{option}\" but the file \
                     declares \"{declared}\". Set reconcile:false to let the file win."
                )));
            }
            return Ok(declared);
        }
    }
    Ok(declared.or(option).unwrap_or(ProtoVersion::Proto2))
}
