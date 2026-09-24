// Conformance against protoc's own parser test corpus, in Rust.
//
// `test/protobuf-suite/*.json` is a vendored extraction of upstream
// protobuf's `src/google/protobuf/compiler/parser_unittest.cc` (v36.2);
// see `../test/protobuf-suite/AGENTS.md` for provenance and lane
// meanings.
//
// This is the Rust counterpart of `ts/test/protobuf-conformance.test.ts`
// and `go/protobuf_conformance_test.go`. It reads the SAME files with the
// SAME contracts and the SAME normalisation, so a runtime cannot drift on
// the third-party corpus without going red. `test/spec/protobuf-suite.tsv`
// feeds the `valid` lane through the shared fixtures as well, but its
// expected column is THIS PARSER's output: that proves Rust matches
// TypeScript, not that Rust matches protoc, and it covers neither
// `accept-only` nor `leniency`.
//
// The corpus is vendored, so this never skips: if the files are missing
// the suite FAILS rather than quietly passing. A conformance suite that
// silently does not run reports green while measuring nothing.

mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Map, Value as Json};

use common::suite_dir;

#[derive(Deserialize)]
struct CorpusCase {
    name: String,
    #[serde(default)]
    helper: String,
    input: String,
    #[serde(default)]
    expected: Json,
}

fn corpus_path(file: &str) -> PathBuf {
    suite_dir().join(file)
}

fn load_corpus(file: &str) -> Vec<CorpusCase> {
    let path = corpus_path(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read the vendored conformance corpus {}: {error}\n\
             This test does not skip: the corpus is committed to this repo, so an absent \
             file is a real failure, not a reason to pass quietly.",
            path.display()
        )
    });
    let cases: Vec<CorpusCase> = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{}: bad JSON: {error}", path.display()));
    assert!(!cases.is_empty(), "{}: no cases", path.display());
    cases
}

// ---- declared deviations from protoc's descriptor encoding ----------------
//
// @tabnas/proto records options as a plain `{name: value}` map rather than
// protoc's `uninterpretedOption` list (see ../AGENTS.md, "Output shape").
// The two carry the same information, so translate protoc's encoding into
// ours and compare: a name or a value this parser failed to capture still
// fails. Kept in step with `bridgeOptions` in the other two runners.

fn option_name(parts: Option<&Json>) -> String {
    let Some(Json::Array(list)) = parts else {
        return String::new();
    };
    let mut names = Vec::with_capacity(list.len());
    for part in list {
        let mut name = part
            .get("namePart")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        if part
            .get("isExtension")
            .and_then(Json::as_bool)
            .unwrap_or(false)
        {
            name = format!("({name})");
        }
        names.push(name);
    }
    names.join(".")
}

/// protojson's two spellings of an integer, a JSON number and, for 64-bit
/// fields, a JSON string, as the same number either way.
fn as_number(value: &Json) -> Json {
    match value {
        Json::String(text) => match text.parse::<f64>() {
            Ok(number) => number_json(number),
            Err(_) => value.clone(),
        },
        _ => value.clone(),
    }
}

fn number_json(number: f64) -> Json {
    serde_json::Number::from_f64(number).map_or(Json::Null, Json::Number)
}

fn option_value(option: &Map<String, Json>) -> Json {
    if let Some(Json::String(text)) = option.get("stringValue") {
        // protojson renders a `bytes` field as standard base64.
        return match base64_decode(text) {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(decoded) => Json::String(decoded),
                Err(_) => Json::String(text.clone()),
            },
            None => Json::String(text.clone()),
        };
    }
    if let Some(value) = option.get("positiveIntValue") {
        return as_number(value);
    }
    if let Some(value) = option.get("negativeIntValue") {
        return as_number(value);
    }
    if let Some(value) = option.get("doubleValue") {
        // protoc evaluates `inf`, `-inf` and `-nan` to a double; this
        // parser keeps the literal text the source wrote.
        if let Json::String(text) = value {
            match text.as_str() {
                "Infinity" => return Json::String("inf".to_string()),
                "-Infinity" => return Json::String("-inf".to_string()),
                "NaN" => return Json::String("-nan".to_string()),
                _ => {}
            }
        }
        return as_number(value);
    }
    if let Some(value) = option.get("aggregateValue") {
        return value.clone();
    }
    match option.get("identifierValue").and_then(Json::as_str) {
        Some("true") => Json::Bool(true),
        Some("false") => Json::Bool(false),
        Some(other) => Json::String(other.to_string()),
        None => Json::String(String::new()),
    }
}

fn bridge_options(value: Json) -> Json {
    let Json::Object(entries) = &value else {
        return value;
    };
    let Some(Json::Array(list)) = entries.get("uninterpretedOption") else {
        return value;
    };
    let mut out = Map::new();
    for (key, entry) in entries {
        if "uninterpretedOption" != key {
            out.insert(key.clone(), entry.clone());
        }
    }
    for entry in list {
        if let Json::Object(option) = entry {
            out.insert(option_name(option.get("name")), option_value(option));
        }
    }
    Json::Object(out)
}

fn bridge(value: Json) -> Json {
    match value {
        Json::Array(items) => Json::Array(items.into_iter().map(bridge).collect()),
        Json::Object(entries) => Json::Object(
            entries
                .into_iter()
                .map(|(key, entry)| {
                    if "options" == key {
                        (key, bridge_options(bridge(entry)))
                    } else {
                        (key, bridge(entry))
                    }
                })
                .collect(),
        ),
        other => other,
    }
}

/// The ONE normalisation, applied identically to BOTH sides: drop a key
/// whose value is absent (JSON null) or an empty list, so a golden that
/// omits a defaulted field compares equal to a descriptor that spells it
/// out as `[]`, and read every number as a double, as the other two
/// runtimes' JSON round trip does. It can never hide a difference in a
/// PRESENT value: if either side has a value the other lacks, the compare
/// still fails.
fn norm_corpus(value: Json) -> Json {
    match value {
        Json::Array(items) => Json::Array(items.into_iter().map(norm_corpus).collect()),
        Json::Object(entries) => {
            let mut out = Map::new();
            for (key, entry) in entries {
                let normalised = norm_corpus(entry);
                if normalised.is_null() {
                    continue;
                }
                if matches!(&normalised, Json::Array(items) if items.is_empty()) {
                    continue;
                }
                out.insert(key, normalised);
            }
            Json::Object(out)
        }
        Json::Number(number) => number_json(number.as_f64().unwrap_or(f64::NAN)),
        other => other,
    }
}

/// `defaultValue` is a string in descriptor.proto. protoc re-renders a
/// numeric default through the field's C++ type; this parser keeps the
/// literal as written. Compare numerically when both sides are numbers.
fn same_defaults(a: Option<&Json>, b: Option<&Json>) -> bool {
    let (Some(Json::String(a)), Some(Json::String(b))) = (a, b) else {
        return false;
    };
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn corpus_equal(got: &Json, want: &Json) -> bool {
    if got == want {
        return true;
    }
    if let (Json::Array(got), Json::Array(want)) = (got, want) {
        return got.len() == want.len()
            && got.iter().zip(want.iter()).all(|(a, b)| corpus_equal(a, b));
    }
    let (Json::Object(got), Json::Object(want)) = (got, want) else {
        return false;
    };
    let mut keys: Vec<&String> = got.keys().collect();
    for key in want.keys() {
        if !got.contains_key(key) {
            keys.push(key);
        }
    }
    for key in keys {
        let a = got.get(key);
        let b = want.get(key);
        let equal = match (a, b) {
            (Some(a), Some(b)) => corpus_equal(a, b),
            (None, None) => true,
            _ => false,
        };
        if equal {
            continue;
        }
        if "defaultValue" == key.as_str() && same_defaults(a, b) {
            continue;
        }
        return false;
    }
    true
}

// ---- scope ----------------------------------------------------------------

/// Every `edition` declaration in `input`, as the value it names.
///
/// The Go runner uses `edition\s*=\s*["']` and the TypeScript one
/// `/edition\s*=\s*["'](?!2023|2024)/`. Both are written out here: the
/// `regex` crate reads `\s` Unicode-aware where JavaScript's class is
/// narrower, and this is a scope decision, not a place to widen anything
/// by accident.
fn edition_values(input: &str) -> Vec<&str> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = input[at..].find("edition") {
        let mut cursor = at + found + "edition".len();
        at = cursor;
        cursor = skip_space(bytes, cursor);
        if bytes.get(cursor) != Some(&b'=') {
            continue;
        }
        cursor = skip_space(bytes, cursor + 1);
        if !matches!(bytes.get(cursor), Some(b'"' | b'\'')) {
            continue;
        }
        let rest = &input[cursor + 1..];
        let end = rest.find(['"', '\'']).unwrap_or(rest.len());
        out.push(&rest[..end]);
    }
    out
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && matches!(bytes[at], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        at += 1;
    }
    at
}

/// An input declaring an edition the plugin does not claim.
///
/// protoc carries internal `UNSTABLE` and `NNNNN_TEST_ONLY` editions for
/// in-development features. @tabnas/proto documents proto2, proto3 and
/// editions 2023 and 2024, and refuses any other edition string.
fn out_of_scope(input: &str) -> bool {
    edition_values(input)
        .iter()
        .any(|value| "2023" != *value && "2024" != *value)
}

/// The exclusion guard: it must stay exactly the protoc-internal
/// editions, not a dumping ground for failures.
fn internal_edition(input: &str) -> bool {
    edition_values(input).iter().any(|value| {
        "UNSTABLE" == *value
            || value
                .strip_suffix("_TEST_ONLY")
                .is_some_and(|head| !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn corpus_label(case: &CorpusCase) -> String {
    let one: String = case.input.split_whitespace().collect::<Vec<_>>().join(" ");
    let short = if 60 < one.chars().count() {
        format!("{}...", one.chars().take(57).collect::<String>())
    } else {
        one
    };
    format!("{}: {short}", case.name)
}

/// Parse, reporting rejection. A panic counts as rejection, exactly as a
/// thrown error does in the TypeScript runner: the three runtimes must
/// classify the same input the same way, or the divergence is the
/// finding.
fn parse_guarded(src: &str) -> Result<Json, String> {
    match catch_unwind(AssertUnwindSafe(|| tabnas_proto::parse(src, None))) {
        Ok(Ok(file)) => Ok(serde_json::to_value(&file).expect("a descriptor is JSON")),
        Ok(Err(error)) => Err(error.to_string()),
        Err(panic) => Err(format!("panic: {}", panic_message(&panic))),
    }
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = panic.downcast_ref::<String>() {
        return text.clone();
    }
    "unknown".to_string()
}

// ---- lanes ----------------------------------------------------------------

/// `valid`: must parse AND equal the FileDescriptorProto protoc's parser
/// produces. The real conformance bar.
#[test]
fn protobuf_corpus_valid() {
    let valid = load_corpus("valid.json");
    let in_scope: Vec<&CorpusCase> = valid
        .iter()
        .filter(|case| !out_of_scope(&case.input))
        .collect();
    let skipped = valid.len() - in_scope.len();

    assert_eq!(
        skipped, 11,
        "excluded {skipped} valid cases, expected 11 - do not widen the exclusion set to \
         hide a failure"
    );
    for case in valid.iter().filter(|case| out_of_scope(&case.input)) {
        assert!(
            internal_edition(&case.input),
            "{}: excluded but does not declare a protoc-internal edition: {:?}",
            case.name,
            case.input
        );
    }

    let mut failures = Vec::new();
    for case in &in_scope {
        let got = match parse_guarded(&case.input) {
            Ok(got) => got,
            Err(why) => {
                failures.push(format!(
                    "{}: rejected a valid document: {why}\n  input: {:?}",
                    case.name, case.input
                ));
                continue;
            }
        };
        let mut normal_got = norm_corpus(got);
        let normal_want = norm_corpus(bridge(case.expected.clone()));

        // protoc omits `syntax` for a file with no declaration (proto2).
        if let (Json::Object(got), Json::Object(want)) = (&mut normal_got, &normal_want) {
            if !want.contains_key("syntax")
                && got.get("syntax").and_then(Json::as_str) == Some("proto2")
            {
                got.remove("syntax");
            }
        }

        if !corpus_equal(&normal_got, &normal_want) {
            failures.push(format!(
                "{}: descriptor mismatch\n  input: {:?}\n  got  {}\n  want {}",
                corpus_label(case),
                case.input,
                normal_got,
                normal_want
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} in-scope valid case(s) failed:\n{}",
        failures.len(),
        in_scope.len(),
        failures.join("\n")
    );
    // Ratcheted at what was measured, so a corpus that shrinks cannot
    // pass by running less.
    assert_eq!(in_scope.len(), 78, "in-scope valid cases");
}

/// `accept-only`: protoc's PARSER accepts these (upstream asserts the
/// error collector is empty) and publishes no descriptor golden, so this
/// lane can only assert accept or reject. Must parse without failing.
#[test]
fn protobuf_corpus_accept_only() {
    let cases = load_corpus("accept-only.json");
    let mut failures = Vec::new();
    for case in &cases {
        if let Err(why) = parse_guarded(&case.input) {
            failures.push(format!(
                "{}: rejected input that protoc's parser accepts (upstream {}): {why}\n  \
                 input: {:?}",
                corpus_label(case),
                case.helper,
                case.input
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} accept-only case(s) failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    assert_eq!(cases.len(), 50, "accept-only cases");
}

#[derive(Deserialize)]
struct LeniencyFile {
    #[serde(rename = "protocVersion")]
    protoc_version: String,
    probes: Vec<Probe>,
}

#[derive(Deserialize)]
struct Probe {
    name: String,
    input: String,
    why: String,
    accepted: bool,
    tabnas: bool,
    #[serde(default, rename = "tabnasNote")]
    tabnas_note: String,
}

/// Lexer-leniency probes: places where the shared tabnas lexer is more
/// permissive than the `.proto` grammar. `accepted` is protoc's answer,
/// `tabnas` is this family's. Pinning both keeps the deviation surface
/// from growing silently: a new divergence turns this red until it is a
/// deliberate, recorded decision.
#[test]
fn protobuf_leniency() {
    let path = corpus_path("leniency.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}\nThis test does not skip.",
            path.display()
        )
    });
    let file: LeniencyFile = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{}: bad JSON: {error}", path.display()));
    assert!(!file.probes.is_empty(), "{}: no probes", path.display());

    let mut failures = Vec::new();
    let mut diverge = Vec::new();
    for probe in &file.probes {
        if probe.accepted != probe.tabnas {
            diverge.push(probe.name.clone());
        }
        let accepted = parse_guarded(&probe.input).is_ok();
        if accepted != probe.tabnas {
            let note = if probe.tabnas_note.is_empty() {
                &probe.why
            } else {
                &probe.tabnas_note
            };
            failures.push(format!(
                "{}: accepted={accepted}, recorded tabnas={} (protoc {} says {})\n  \
                 input: {:?}\n  why:   {}\n  detail: {note}",
                probe.name,
                probe.tabnas,
                file.protoc_version,
                probe.accepted,
                probe.input,
                probe.why
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} leniency probe(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );

    diverge.sort();
    assert_eq!(
        diverge,
        vec![
            "digit-separator-in-field-number",
            "exponent-field-number",
            "hash-line-comment",
            "underscore-suffixed-number-in-enum",
        ],
        "recorded deviations from protoc"
    );
}

/// Standard base64, written out: the corpus is the only caller and a
/// dependency for one decode would land in every crate that takes this
/// one as a dev-dependency.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    let mut padding = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            _ => return None,
        };
        if 0 < padding {
            return None;
        }
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if 8 <= bits {
            bits -= 8;
            out.push(((accumulator >> bits) & 0xFF) as u8);
        }
    }
    if 0 != accumulator & ((1 << bits) - 1) {
        return None;
    }
    Some(out)
}
