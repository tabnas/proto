/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! What this crate refuses, and why.
//!
//! The package declares no error CODES of its own: input the grammar
//! cannot recognise fails under the engine's base codes, and the two
//! rejections the plugin makes itself, the syntax against edition version
//! check and the nesting cap, are told apart by their messages. See the
//! repository `AGENTS.md`, "Error codes".

use std::fmt;

use tabnas::TabnasError;

/// A refused `.proto` document, or a plugin that could not be installed.
#[derive(Debug, Clone)]
pub enum ProtoError {
    /// The engine rejected the source. Carries the engine's own error,
    /// with its code, position and rendered report.
    Parse(Box<TabnasError>),
    /// The embedded compiled grammar could not be loaded or installed. Only a
    /// broken build reaches this.
    Grammar(String),
    /// The plugin's own version check refused the document: an unknown
    /// `syntax` or `edition` value, or a declaration disagreeing with the
    /// supplied option.
    Version(String),
    /// The document nests deeper than [`crate::MAX_NESTING_DEPTH`].
    ///
    /// Every runtime refuses such a document before the engine runs, with
    /// the same message and no code; only this port's walk also refuses a
    /// tree that deep handed to it directly, because a Rust stack that
    /// runs out aborts the process. See `DIVERGENCE.md`, section 2.
    TooDeep(String),
}

impl ProtoError {
    /// The engine error code, or `""` for a rejection this crate made
    /// itself.
    pub fn code(&self) -> &str {
        match self {
            ProtoError::Parse(error) => &error.code,
            _ => "",
        }
    }

    /// The 1-based row and column the engine reported, when there is one.
    pub fn position(&self) -> Option<(usize, usize)> {
        match self {
            ProtoError::Parse(error) => Some((error.row, error.col)),
            _ => None,
        }
    }
}

impl fmt::Display for ProtoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Parse(error) => error.fmt(formatter),
            ProtoError::Grammar(message)
            | ProtoError::Version(message)
            | ProtoError::TooDeep(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ProtoError {}

impl From<TabnasError> for ProtoError {
    fn from(error: TabnasError) -> Self {
        ProtoError::Parse(Box::new(error))
    }
}
