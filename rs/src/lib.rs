// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// The engine's error carries a code, position, hint and a formatted
// report, so it is large by design and `Result<_, TabnasError>` trips
// clippy's `result_large_err`. This crate boxes it inside `ProtoError`
// for that reason; the allow covers the engine's own `Result` where it
// surfaces here.
#![allow(clippy::result_large_err)]

//! Parse Protocol Buffers `.proto` IDL into FileDescriptorProto-shaped
//! values.
//!
//! proto2, proto3 and editions 2023 and 2024, with the version detected
//! from the file's `syntax` or `edition` declaration. The parser is an
//! [ABNF](https://github.com/tabnas/abnf) grammar driving the
//! [`tabnas`](https://github.com/tabnas/parser) engine rather than a
//! hand-written parser: `proto-grammar/*.abnf` at the repository root is
//! the single source of truth, embedded into every runtime by
//! `ts/embed-grammar.js`.
//!
//! ```
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let file = tabnas_proto::parse("syntax = \"proto3\";\nmessage M { int32 a = 1; }", None)?;
//!     assert_eq!(file.syntax.as_deref(), Some("proto3"));
//!     assert_eq!(file.message_type[0].name, "M");
//!     assert_eq!(file.message_type[0].field[0].number, 1.0);
//!     Ok(())
//! }
//! ```
//!
//! TypeScript is canonical: `ts/src` defines behaviour, and the shared
//! fixtures in `test/spec/*.tsv` are the parity contract across
//! TypeScript, Go and Rust. Where this port cannot match the canonical
//! value, `DIVERGENCE.md` at the repository root records it.
//!
//! A parsed `.proto` file is DATA, never instructions. Schema files
//! arrive from outside the system, and every string in a descriptor,
//! `import` paths and `type_name`s included, is untrusted text. See the
//! repository `AGENTS.md`.

mod aggregate;
mod build_descriptor;
mod descriptor;
mod detect_version;
mod error;
mod grammar;
mod jsnum;
mod node;
mod strings;

/// The README's Rust examples run as doctests, so a stale one fails the
/// gate rather than misleading the reader. Its `toml` and `bash` fences
/// are skipped; rustdoc runs only the `rust` ones.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;
use serde::Deserialize;
use tabnas::{
    LexMatcher, Options as EngineOptions, Plugin, PluginError, RewindOptions, Tabnas, Value,
};
use tabnas_abnf::{abnf, AbnfConvertOptions, AbnfOptions};

pub use build_descriptor::{build_file, MAX_NESTING_DEPTH};
pub use descriptor::{
    scalar_type, DescriptorProto, DescriptorRange, EnumDescriptorProto, EnumValueDescriptorProto,
    FieldDescriptorProto, FieldLabel, FieldType, FileDescriptorProto, MethodDescriptorProto,
    OneofDescriptorProto, OptionValue, Options, ServiceDescriptorProto, SymbolVisibility,
    MAX_ENUM_NUMBER, MAX_FIELD_NUMBER_END, MAX_MESSAGE_SET_END, SCALAR_TYPES,
};
pub use detect_version::{
    declared_version, declared_version_src, edition_enum, is_edition, resolve_version, ProtoVersion,
};
pub use error::ProtoError;
pub use grammar::GRAMMAR_TEXT;
pub use node::{child, child_rules, children, gaps, gaps_before, kw, nrule, nsrc};

/// This crate's version. It MUST equal `ts/package.json` "version": the
/// release orchestrator rewrites both, and `tests/version_test.rs` fails
/// the build if they drift. Mirrors `VERSION` in `ts/src/proto.ts` and
/// `const VERSION` in `go/proto.go`.
pub const VERSION: &str = "0.5.1";

/// The plugin's name on an instance, and the key its option bag hangs
/// under.
pub const PLUGIN_NAME: &str = "Proto";

/// The engine's retained backtracking history, as the canonical
/// `new Tabnas({ rewind: { history: 8192 } })` sets it.
///
/// The union grammar backtracks across whole statements, and the engine's
/// default of 64 consumed tokens is not enough to rewind one. This is a
/// parse-affecting setting, not a tuning knob: lower it and documents
/// that should parse stop parsing.
pub const REWIND_HISTORY: usize = 8192;

/// How a `.proto` document is read.
///
/// Mirrors the canonical `ProtoOptions`. The defaults auto-detect the
/// version from the file's declaration and refuse an explicit version
/// that disagrees with it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ProtoOptions {
    /// An explicit protobuf version. `None` auto-detects from the file's
    /// `syntax` or `edition` declaration.
    pub version: Option<ProtoVersion>,
    /// When true (the default), a version that disagrees with the file's
    /// declaration is an error; when false the declaration wins.
    pub reconcile: bool,
}

impl Default for ProtoOptions {
    fn default() -> Self {
        ProtoOptions {
            version: None,
            reconcile: true,
        }
    }
}

/// The names of this plugin's two lexer matchers.
pub(crate) const AGGREGATE_WORD: &str = "protoAggregateWord";
pub(crate) const ADJACENT_STRINGS: &str = "protoAdjacentStrings";

/// Install the union proto grammar on an engine instance, so it can parse
/// `.proto` source into a `{rule, src, kids}` CST.
///
/// Use [`to_descriptor`] to turn that CST into a [`FileDescriptorProto`].
/// The Rust spelling of the canonical `tn.use(Proto)`.
///
/// Driving the engine's own `parse` skips the nesting [`preflight`], so
/// a caller handing it untrusted source runs that check first; see
/// [`preflight`] for what the tree costs without it.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let mut parser = tabnas_proto::engine();
///     tabnas_proto::proto(&mut parser)?;
///     let source = "syntax = \"proto3\";";
///     tabnas_proto::preflight(source)?;
///     let cst = parser.parse(source)?;
///     let file = tabnas_proto::to_descriptor(&cst, None)?;
///     assert_eq!(file.syntax.as_deref(), Some("proto3"));
///     Ok(())
/// }
/// ```
pub fn proto(parser: &mut Tabnas) -> Result<(), ProtoError> {
    // Guard against re-invocation on the same instance: the grammar is
    // stateless, but compiling and installing it twice is wasted work.
    // The engine's own rule list answers the question without inventing
    // a decoration key.
    if parser.rule_names().iter().any(|name| "proto" == name) {
        return Ok(());
    }
    // `word_keywords` is REQUIRED: it makes literal keywords match as
    // whole words, so `option` does not grab the `option` prefix of
    // `optional`. Without it the grammar mis-tokenises.
    let convert = AbnfConvertOptions {
        start: Some("proto".to_string()),
        tag: Some("proto".to_string()),
        word_keywords: true,
        ..AbnfConvertOptions::default()
    };
    abnf(parser, GRAMMAR_TEXT, Some(&AbnfOptions::new(convert)))
        .map_err(|error| ProtoError::Grammar(format!("proto: {error}")))?;
    // An aggregate value (`option (f) = { a: 1 };`) is recorded as the text
    // between its braces, which the CST's `src` does not keep: the lexer
    // drops whitespace and comments. This action reads it from the source
    // while the brace tokens are to hand; see `aggregate.rs`.
    let mut pushed = Vec::new();
    parser.define_rule("constant", |spec| {
        spec.add_ac(aggregate::record_aggregate);
        pushed = aggregate::pushed_rules(spec);
    });
    // Every rule inside an aggregate value carries a mark, set on the
    // rules the `constant` rule pushes and kept by the rules below them,
    // so the matchers below can tell inside from outside at once; see
    // `aggregate.rs`.
    for name in pushed {
        parser.define_rule(name, |spec| {
            spec.add_bo(aggregate::mark_aggregate);
        });
    }
    // Text format has no keywords: inside an aggregate value a word the
    // grammar spells as a keyword, and a bracketed extension or Any name,
    // are identifiers. This matcher runs ahead of the grammar's own (the
    // 1e6 band) and reads them so there; see `aggregate.rs`. The second
    // refuses adjacent string literals that protoc's tokenizer refuses;
    // see `strings.rs`.
    parser
        .set_options(|options| {
            options.lex.matchers.insert(
                AGGREGATE_WORD.to_string(),
                LexMatcher {
                    name: AGGREGATE_WORD.to_string(),
                    order: 900_000.0,
                    matcher: None,
                    imperative: Some(Arc::new(aggregate::aggregate_word)),
                    factory: None,
                },
            );
            options.lex.matchers.insert(
                ADJACENT_STRINGS.to_string(),
                LexMatcher {
                    name: ADJACENT_STRINGS.to_string(),
                    order: 950_000.0,
                    matcher: None,
                    imperative: Some(Arc::new(strings::adjacent_strings)),
                    factory: None,
                },
            );
        })
        .map_err(|error| ProtoError::Grammar(format!("proto: {error}")))?;
    Ok(())
}

/// The plugin form of [`proto`], for [`Tabnas::use_plugin`].
///
/// Installed this way the grammar is re-applied to derived instances, as
/// every native plugin is.
pub fn plugin() -> Plugin {
    let mut defaults = IndexMap::new();
    defaults.insert("version".to_string(), Value::Null);
    defaults.insert("reconcile".to_string(), Value::Bool(true));
    Plugin::new(PLUGIN_NAME, |parser, _options| {
        proto(parser).map_err(|error| PluginError(error.to_string()))
    })
    .with_defaults(Value::object(defaults))
}

/// A bare engine configured the way this plugin needs it, with no grammar
/// installed yet.
///
/// The canonical `new Tabnas({ rewind: { history: 8192 } })`. See
/// [`REWIND_HISTORY`] for why the setting is not optional.
pub fn engine() -> Tabnas {
    Tabnas::with_options(EngineOptions {
        rewind: RewindOptions {
            history: Some(REWIND_HISTORY),
        },
        ..EngineOptions::default()
    })
}

/// Build a proto parser: [`engine`] with this plugin installed, the
/// counterpart of `new Tabnas().use(Proto)` and the Go `Proto(j)`.
///
/// Compiling the grammar dominates a parse, so build one and reuse it.
/// Read each document through [`parse_with`], which runs the nesting
/// [`preflight`] the engine's own `parse` does not.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let parser = tabnas_proto::make();
///     let file = tabnas_proto::parse_with(&parser, "message M {}", None)?;
///     // No declaration and no option: protoc's default is proto2.
///     assert_eq!(file.syntax.as_deref(), Some("proto2"));
///     Ok(())
/// }
/// ```
pub fn make() -> Tabnas {
    let mut parser = engine();
    proto(&mut parser).expect("the embedded proto grammar is fixed and valid");
    parser
}

/// The shared default parser.
///
/// Compiling the grammar dominates a parse by orders of magnitude, and
/// the plugin keeps no per-parse state on the instance, so one instance
/// serves every call to [`parse`]. Parsing builds a fresh context and
/// only reads instance state, so it is safe for concurrent use.
fn shared() -> &'static Tabnas {
    static DEFAULT: OnceLock<Tabnas> = OnceLock::new();
    DEFAULT.get_or_init(make)
}

/// Turn a parsed proto CST into a [`FileDescriptorProto`], resolving the
/// version from the file's declaration and the supplied options.
///
/// The walk refuses a CST nesting past [`MAX_NESTING_DEPTH`], but by
/// then the tree EXISTS, and a `tabnas::Value` that deep aborts the
/// process when it drops. A caller who parsed the source itself runs
/// [`preflight`] on that source first; [`parse_with`] does both.
pub fn to_descriptor(
    cst: &Value,
    options: Option<&ProtoOptions>,
) -> Result<FileDescriptorProto, ProtoError> {
    let opts = options.cloned().unwrap_or_default();
    let first = child_rules(cst).into_iter().next();
    let declared = match first {
        Some(node) if "syntaxOrEdition" == nrule(node) => declared_version(node)?,
        _ => None,
    };
    let version = resolve_version(declared, opts.version, opts.reconcile)?;
    build_file(cst, version)
}

/// Refuse a `.proto` source that nests deeper than
/// [`MAX_NESTING_DEPTH`], before anything builds a tree that deep.
///
/// [`parse`] and [`parse_with`] run this themselves. It is public for
/// the caller who drives the engine directly, through [`make`] or
/// [`proto`] on an instance of their own: the engine's `parse` builds a
/// [`tabnas::Value`] tree that mirrors the document, and that value
/// DROPS recursively, so a deep enough document aborts the process even
/// when the descriptor walk refuses it. An abort cannot be caught, so
/// the check has to come before the tree exists.
///
/// The depth is counted in braces, and in the angle brackets that nest a
/// message inside an aggregate value (`{ a < b: 1 > }`), skipping the
/// string literals and comments the lexer skips. Over-counting is safe
/// here and under-counting is not, so an unterminated string or comment
/// counts every brace inside it; the engine rejects that source anyway.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let cap = tabnas_proto::MAX_NESTING_DEPTH;
///     let parser = tabnas_proto::make();
///
///     let deep = format!("{}{}", "message M {".repeat(cap + 1), "}".repeat(cap + 1));
///     assert!(tabnas_proto::preflight(&deep).is_err());
///
///     // At the cap the document is accepted, and the engine may run.
///     let ok = format!("{}{}", "message M {".repeat(cap), "}".repeat(cap));
///     tabnas_proto::preflight(&ok)?;
///     let cst = parser.parse(&ok)?;
///     assert_eq!(tabnas_proto::nrule(&cst), "proto");
///     Ok(())
/// }
/// ```
pub fn preflight(src: &str) -> Result<(), ProtoError> {
    let depth = build_descriptor::nesting_depth(src);
    if depth > MAX_NESTING_DEPTH {
        return Err(ProtoError::TooDeep(format!(
            "proto: document nests {depth} levels deep, past the {MAX_NESTING_DEPTH} this \
             parser accepts"
        )));
    }
    Ok(())
}

/// Parse a `.proto` source string on a parser the caller holds.
///
/// The high-throughput path. Compiling the grammar dominates a parse by
/// orders of magnitude, so a caller reading many documents builds one
/// instance with [`make`] and passes it here, rather than driving the
/// engine directly: this runs the same [`preflight`] [`parse`] runs, and
/// the engine's own `parse` does not.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let parser = tabnas_proto::make();
///     for source in ["syntax = \"proto2\";", "edition = \"2023\";"] {
///         let file = tabnas_proto::parse_with(&parser, source, None)?;
///         assert!(file.syntax.is_some());
///     }
///     Ok(())
/// }
/// ```
pub fn parse_with(
    parser: &Tabnas,
    src: &str,
    options: Option<&ProtoOptions>,
) -> Result<FileDescriptorProto, ProtoError> {
    preflight(src)?;
    let cst = parser.parse(src)?;
    to_descriptor(&cst, options)
}

/// Parse a `.proto` source string to a [`FileDescriptorProto`].
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let source = "syntax = \"proto2\";\nmessage M { required int32 id = 1; }";
///     let file = tabnas_proto::parse(source, None)?;
///     let field = &file.message_type[0].field[0];
///     assert_eq!(field.label, Some(tabnas_proto::FieldLabel::Required));
///     assert_eq!(field.r#type, Some(tabnas_proto::FieldType::Int32));
///     Ok(())
/// }
/// ```
///
/// A document nesting deeper than [`MAX_NESTING_DEPTH`] is refused before
/// the engine builds a tree that deep; see that constant.
pub fn parse(src: &str, options: Option<&ProtoOptions>) -> Result<FileDescriptorProto, ProtoError> {
    parse_with(shared(), src, options)
}
