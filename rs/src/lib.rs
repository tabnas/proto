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
//! `ts/embed-grammar.js`. `tabnas-abnf` compiles it at build time into
//! `proto-grammar.json`, which this crate embeds and installs, so the
//! crate itself depends on no ABNF compiler.
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
//! The same descriptor also comes as a tree, through [`parse_value`],
//! [`parse_value_with`] or [`to_descriptor_value`]: the plain value the
//! canonical `parse` returns, with every member named and ordered as its
//! object has them, for a host that walks the value rather than reading
//! the struct. [`descriptor_value`] gives a descriptor alone the same
//! shape, the members a statement places in a documented order.
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
mod order;
mod tree;

/// The README's Rust examples run as doctests, so a stale one fails the
/// gate rather than misleading the reader. Its `toml` and `bash` fences
/// are skipped; rustdoc runs only the `rust` ones.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

use std::sync::OnceLock;

use indexmap::IndexMap;
use serde::Deserialize;
use tabnas::{
    GrammarSpec, Options as EngineOptions, Plugin, PluginError, RewindOptions, Tabnas, Value,
};

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
pub use tree::descriptor_value;

/// This crate's version. It MUST equal `ts/package.json` "version": the
/// release orchestrator rewrites both, and `tests/version_test.rs` fails
/// the build if they drift. Mirrors `VERSION` in `ts/src/proto.ts` and
/// `const VERSION` in `go/proto.go`.
pub const VERSION: &str = "0.6.7";

/// The plugin's name on an instance, and the key its option bag hangs
/// under.
pub const PLUGIN_NAME: &str = "Proto";

/// The compiled grammar: `proto-grammar.json`, the engine's serialized
/// rule set, which `tabnas-abnf` compiles from [`GRAMMAR_TEXT`] at build
/// time (`tests/grammar_spec_test.rs` writes it, and fails when it is
/// stale). Generated: never edit it.
const GRAMMAR_SPEC: &str = include_str!("../proto-grammar.json");

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
    // stateless, but loading and installing it twice is wasted work.
    // The engine's own rule list answers the question without inventing
    // a decoration key.
    if parser.rule_names().iter().any(|name| "proto" == name) {
        return Ok(());
    }
    // The union grammar, compiled at build time with the options this
    // function used to pass at every install. `word_keywords` is REQUIRED:
    // it makes literal keywords match as whole words, so `option` does not
    // grab the `option` prefix of `optional`. Without it the grammar
    // mis-tokenises. `token_classes` compiles `ident` (an identifier or any
    // keyword) to one engine token set, so a lookahead position peeks it as
    // one token rather than one alternate per keyword. The CST builders are
    // the engine's own `@node$` / `@capture$` / `@bubble$` builtins, so the
    // document is pure data and installs with no compiler.
    let spec = GrammarSpec::from_json(GRAMMAR_SPEC)
        .map_err(|error| ProtoError::Grammar(format!("proto: {error}")))?;
    parser
        .grammar(&spec)
        .map_err(|error| ProtoError::Grammar(format!("proto: {error}")))?;
    // An aggregate value (`option (f) = { a: 1 };`) is recorded as the text
    // between its braces, which the CST's `src` does not keep: the lexer
    // drops whitespace and comments. This action reads it from the source
    // while the brace tokens are to hand; see `aggregate.rs`.
    parser.define_rule("constant", |spec| {
        spec.add_ac(aggregate::record_aggregate);
    });
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
/// Installing the grammar dominates a parse, so build one and reuse it.
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
/// Installing the grammar dominates a parse by more than an order of
/// magnitude, and the plugin keeps no per-parse state on the instance, so one instance
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
    to_descriptor_ordered(cst, options).map(|(file, _)| file)
}

/// [`to_descriptor`], with the walk's record of the source's order
/// (`order.rs`).
fn to_descriptor_ordered(
    cst: &Value,
    options: Option<&ProtoOptions>,
) -> Result<(FileDescriptorProto, order::Order), ProtoError> {
    let opts = options.cloned().unwrap_or_default();
    let first = child_rules(cst).into_iter().next();
    let declared = match first {
        Some(node) if "syntaxOrEdition" == nrule(node) => declared_version(node)?,
        _ => None,
    };
    let version = resolve_version(declared, opts.version, opts.reconcile)?;
    build_descriptor::build_file_ordered(cst, version)
}

/// Turn a parsed proto CST into the descriptor as a tree: what
/// [`parse_value`] gives, as [`to_descriptor`] turns one into a
/// [`FileDescriptorProto`]. Every member and option is named and ordered
/// as the canonical object has it, the source's order of the members a
/// statement places included, which the walk records beside the
/// descriptor and [`descriptor_value`], handed the descriptor alone,
/// cannot know.
///
/// As for [`to_descriptor`], a caller who parsed the source itself runs
/// [`preflight`] on that source first; [`parse_value_with`] does both.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let parser = tabnas_proto::make();
///     let source = "option java_package = \"x\";\npackage p;";
///     tabnas_proto::preflight(source)?;
///     let cst = parser.parse(source)?;
///     let tree = tabnas_proto::to_descriptor_value(&cst, None)?;
///     assert_eq!(tree, tabnas_proto::parse_value(source, None)?);
///     Ok(())
/// }
/// ```
pub fn to_descriptor_value(
    cst: &Value,
    options: Option<&ProtoOptions>,
) -> Result<Value, ProtoError> {
    let (file, order) = to_descriptor_ordered(cst, options)?;
    Ok(tree::file_value(&file, Some(&order)))
}

/// Refuse a `.proto` source that nests deeper than
/// [`MAX_NESTING_DEPTH`], before anything builds a tree that deep.
///
/// The TypeScript `preflight` and the Go `Preflight` are the same check,
/// with the same scan, cap and message, and their `parse` and `Parse`
/// run it as [`parse`] does here.
///
/// [`parse`] and [`parse_with`] run this themselves. It is public for
/// the caller who drives the engine directly, through [`make`] or
/// [`proto`] on an instance of their own: the engine's `parse` builds a
/// [`tabnas::Value`] tree that mirrors the document, and that value
/// DROPS recursively, so a deep enough document aborts the process even
/// when the descriptor walk refuses it. An abort cannot be caught, so
/// the check has to come before the tree exists.
///
/// The depth is counted in braces, skipping the string literals and
/// comments the lexer skips, where it skips them: a line comment ends at
/// a carriage return as well as a line feed, a backtick string is a
/// string, and a quote opens one only where a token starts, so a quote
/// inside a word is part of the word. Over-counting is safe here and
/// under-counting is not, so an unterminated string or comment counts
/// every brace inside it; the engine rejects that source anyway.
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
    let depth = build_descriptor::brace_depth(src);
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
/// The high-throughput path. Installing the grammar dominates a parse by
/// more than an order of magnitude, so a caller reading many documents builds one
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

/// Parse a `.proto` source string to the descriptor as a tree: the value
/// the canonical `parse` returns, every member named and ordered as its
/// object has them.
///
/// A host that walks the value, as a translation does when it streams a
/// tree's events, reads this rather than the [`FileDescriptorProto`]
/// struct, whose serialization holds the same members in the order the
/// struct declares them. The members a statement places come in the
/// source's order, which the walk records beside the descriptor; on a
/// parser the caller holds, [`parse_value_with`] gives the same tree.
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let tree = tabnas_proto::parse_value("syntax = \"proto3\";\nmessage M { optional int32 a = 1; }", None)?;
///     let tabnas::Value::Object(file) = &tree else { panic!("an object") };
///     let tabnas::Value::Array(messages) = &file["messageType"] else { panic!("a list") };
///     let tabnas::Value::Object(message) = &messages[0] else { panic!("an object") };
///     let tabnas::Value::Array(fields) = &message["field"] else { panic!("a list") };
///     let tabnas::Value::Object(field) = &fields[0] else { panic!("an object") };
///     let names: Vec<&str> = field.keys().map(String::as_str).collect();
///     // `proto3Optional` straight after `label`, and the synthesised
///     // oneof's index last, as the canonical walk assigns them.
///     assert_eq!(names, ["name", "number", "label", "proto3Optional", "type", "oneofIndex"]);
///     Ok(())
/// }
/// ```
pub fn parse_value(src: &str, options: Option<&ProtoOptions>) -> Result<Value, ProtoError> {
    parse_value_with(shared(), src, options)
}

/// [`parse_value`] on a parser the caller holds, as [`parse_with`] is
/// [`parse`]: it runs the same [`preflight`], then the engine, then
/// [`to_descriptor_value`].
///
/// ```
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let parser = tabnas_proto::make();
///     for source in ["package p;\noption a = 1;", "option a = 1;\npackage p;"] {
///         let tree = tabnas_proto::parse_value_with(&parser, source, None)?;
///         assert_eq!(tree, tabnas_proto::parse_value(source, None)?);
///     }
///     Ok(())
/// }
/// ```
pub fn parse_value_with(
    parser: &Tabnas,
    src: &str,
    options: Option<&ProtoOptions>,
) -> Result<Value, ProtoError> {
    preflight(src)?;
    let cst = parser.parse(src)?;
    to_descriptor_value(&cst, options)
}

/// One optional alchemy translation source and its explicit entry point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranslationPart {
    /// The definition a host calls after linking the source.
    pub entry: &'static str,
    /// The source text, or `None` for an entry supplied by alchemy.
    pub source: Option<&'static str>,
}

/// The package-local structural translation interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranslationParts {
    /// The complete `tabnas.plugin.json` text.
    pub manifest: &'static str,
    /// An optional lift from the grammar's events to its first read shape.
    pub lift: Option<TranslationPart>,
    /// An optional embedding of a plain tree in the format's schema, with its reverse.
    pub embed: Option<TranslationPart>,
    /// An optional render from the write shape to text.
    pub render: Option<TranslationPart>,
}

const TRANSLATION: TranslationParts = TranslationParts {
    manifest: include_str!("../translate/manifest.json"),
    lift: None,
    embed: None,
    render: Some(TranslationPart {
        entry: "proto-render",
        source: Some(include_str!("../translate/render.alc")),
    }),
};

/// Return the translation parts of `.proto` files: the manifest, and the
/// render that writes a FileDescriptorProto, the reader's tree, back as a
/// `.proto` file. There is no lift, and no embed: the tree is the
/// descriptor's own shape, the schema `proto-descriptor`, which a host
/// renders only from a source of the same schema or from a program that
/// builds a descriptor.
///
/// ```
/// let parts = tabnas_proto::translate().expect("proto carries translation parts");
/// assert_eq!(parts.render.map(|part| part.entry), Some("proto-render"));
/// assert_eq!(parts.embed, None);
/// ```
#[must_use]
pub const fn translate() -> Option<TranslationParts> {
    Some(TRANSLATION)
}

/// The plugin's manifest, `tabnas.plugin.json`, as the repository carries
/// it. Its `translate` object is what a host that translates reads: the
/// shape the format is read as and written from (`tree`), the schema of
/// that tree (`proto-descriptor`), the root the render needs (`object`),
/// the file that holds the render, and the sentences that say what a
/// written file does not keep. The crate embeds its own copy,
/// `translate/manifest.json`, since a packaged crate holds nothing outside
/// `rs/`; `tests/translate_test.rs` holds the copy to the file.
///
/// ```
/// assert!(tabnas_proto::manifest_text().contains("\"proto-descriptor\""));
/// ```
pub fn manifest_text() -> &'static str {
    TRANSLATION.manifest
}

/// The render, `alchemy/render.alc`, the file the manifest's
/// `translate.render` names: a library of alchemy definitions, with no
/// `export`, whose entry point `proto-render` writes a FileDescriptorProto's
/// events as one `.proto` file that reads back as the same descriptor. A
/// host links it with its own program. The crate embeds its own copy,
/// `translate/render.alc`, held to the file as the manifest's is.
///
/// ```
/// assert!(tabnas_proto::render_text().contains("def proto-render [input]"));
/// ```
pub fn render_text() -> &'static str {
    match TRANSLATION.render {
        Some(part) => part.source.unwrap_or_default(),
        None => "",
    }
}
