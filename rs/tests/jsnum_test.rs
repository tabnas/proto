// `js_number_to_string` against node, over 60,366 doubles.
//
// The descriptor's JSON is this package's output format, so a number in
// it has to be spelt the way `JSON.stringify` spells it, character for
// character. ECMA-262 6.1.6.1.20 is NOT Rust's shortest-float formatter:
// Rust breaks an exact decimal midpoint away from zero where the
// specification takes the even digit, and it has no switch to exponent
// form at 1e21 or 1e-7.
//
// `rs/scripts/js-number-oracle.mjs` builds the SAME value list, formats
// each one with node, and prints the three lines this file pins: the
// count, a hash over the raw bit patterns (which says the two generators
// agree on the VALUES) and a hash over the formatted text (which says
// the two formatters agree). Re-run it after touching either generator:
//
//     node rs/scripts/js-number-oracle.mjs
//
// The hashes below were measured that way on 2026-09-21, against node
// v22.22.2. The named cases beneath them are the same claim in a form
// that says WHICH value moved when it does.

use tabnas_proto::{parse, FileDescriptorProto};

/// The value list, in the order `js-number-oracle.mjs` builds it.
fn values() -> Vec<f64> {
    let mut out = Vec::with_capacity(60_366);
    // Small integers: the `k <= n <= 21` branch, all of it.
    for i in 0..10_000u32 {
        out.push(f64::from(i));
    }
    // Sixteenths, tenths, hundredths and thousandths: where an exact
    // decimal midpoint sits between two shortest digit strings and the
    // specification takes the even one.
    for i in 1..=10_000u32 {
        out.push(f64::from(i) / 16.0);
    }
    for i in 1..=10_000u32 {
        out.push(f64::from(i) / 10.0);
    }
    for i in 1..=10_000u32 {
        out.push(f64::from(i) / 100.0);
    }
    for i in 1..=5_000u32 {
        out.push(f64::from(i) / 1000.0);
    }
    // Negatives, including the sign path.
    for i in 1..=5_000u32 {
        out.push(-f64::from(i) / 8.0);
    }
    // Every decade across the two thresholds where the spelling changes
    // form, 1e21 and 1e-7.
    for m in [1u32, 2, 3, 5, 7, 9] {
        for k in -30i32..=30 {
            out.push(
                format!("{m}e{k}")
                    .parse::<f64>()
                    .expect("a decimal literal"),
            );
        }
    }
    // Raw bit patterns, so the digit generator meets doubles no decimal
    // literal would reach. A 64-bit LCG, the same constants as the
    // oracle.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut taken = 0;
    while taken < 10_000 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let candidate = f64::from_bits(state);
        if candidate.is_finite() {
            out.push(candidate);
            taken += 1;
        }
    }
    out
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// `js_number_to_string` is not exported: it is reached the way a caller
/// reaches it, through the descriptor's JSON. A one-field message whose
/// option value is the double under test renders that double and nothing
/// else numeric, so the text between `"x":` and `}` is the spelling.
fn spell(value: f64) -> String {
    // Built by hand rather than parsed, so the formatter is measured and
    // not the reader: a source literal for every one of 60,366 doubles
    // would also make this suite minutes long.
    let mut file = FileDescriptorProto::default();
    let mut options = tabnas_proto::Options::new();
    options.insert("x".to_string(), tabnas_proto::OptionValue::Number(value));
    let mut field = tabnas_proto::FieldDescriptorProto {
        name: "a".to_string(),
        number: 1.0,
        ..tabnas_proto::FieldDescriptorProto::default()
    };
    field.options = Some(options);
    let mut message = tabnas_proto::DescriptorProto::new("M");
    message.field.push(field);
    file.message_type.push(message);

    let json = serde_json::to_string(&file).expect("a descriptor is JSON");
    let at = json.find("\"x\":").expect("the option is there") + 4;
    let rest = &json[at..];
    let end = rest.find('}').expect("the option map closes");
    rest[..end].to_string()
}

#[test]
fn the_generator_still_builds_what_the_oracle_builds() {
    let values = values();
    // Pinned first, so a generator that drifted fails before a hash is
    // compared and the message says which half moved.
    assert_eq!(
        values.len(),
        60_366,
        "the value list changed shape; re-run rs/scripts/js-number-oracle.mjs"
    );
    let mut bits = Vec::with_capacity(values.len() * 8);
    for value in &values {
        bits.extend_from_slice(&value.to_be_bytes());
    }
    assert_eq!(
        format!("{:x}", fnv1a(&bits)),
        "4746686d1131edd",
        "the VALUES differ from the oracle's, so any text comparison \
         below would be measuring two different lists"
    );
}

#[test]
fn every_double_is_spelt_the_way_node_spells_it() {
    let text = values()
        .into_iter()
        .map(spell)
        .collect::<Vec<String>>()
        .join("\n");
    assert_eq!(
        format!("{:x}", fnv1a(text.as_bytes())),
        "78d6a8c6ddd36e76",
        "the descriptor spells at least one double differently from node; \
         run `node rs/scripts/js-number-oracle.mjs` and bisect"
    );
}

// The same claim in a form that names the value. A hash says something
// moved; these say what.
#[test]
fn the_cases_the_algorithm_exists_for() {
    for (value, want) in [
        (0.0, "0"),
        // JavaScript spells both zeros "0"; Rust keeps the sign.
        (-0.0, "0"),
        (1.0, "1"),
        (-1.0, "-1"),
        (1.5, "1.5"),
        (0.1, "0.1"),
        (100.0, "100"),
        // The fixed-to-exponent thresholds, from both sides.
        (1e20, "100000000000000000000"),
        (1e21, "1e+21"),
        (1e-6, "0.000001"),
        (1e-7, "1e-7"),
        (1.2e-7, "1.2e-7"),
        // Past the range of an i64, where a float formatter would show
        // its exponent form and JavaScript still does not.
        (9.223372036854776e18, "9223372036854776000"),
        (1.8446744073709552e19, "18446744073709552000"),
        (1.2345678901234568e29, "1.2345678901234568e+29"),
        // A large integral double, whose exact binary value is NOT its
        // shortest round-tripping digits (123456789012345683968).
        (1.2345678901234568e20, "123456789012345680000"),
        // Midpoints: the specification takes the even digit.
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
    ] {
        assert_eq!(spell(value), want, "for {value:?}");
    }
}

// Non-finite doubles have no JSON spelling, so `JSON.stringify` writes
// `null` and so does the descriptor. This is the one place the JSON
// rendering departs from `String(value)`, which would say `NaN`.
#[test]
fn a_non_finite_double_is_null_in_json() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(spell(value), "null", "for {value:?}");
    }
}

// And end to end, through a real parse, so the wiring between the walk
// and the serializer is measured too.
#[test]
fn a_parsed_document_spells_its_numbers_the_same_way() {
    for (source, want) in [
        (
            "message M { optional double a = 1 [x = 1e20]; }",
            "100000000000000000000",
        ),
        ("message M { optional double a = 1 [x = 1e21]; }", "1e+21"),
        (
            "message M { optional double a = 1 [x = 0.000001]; }",
            "0.000001",
        ),
        ("message M { optional double a = 1 [x = 1e-7]; }", "1e-7"),
        (
            "message M { optional double a = 1 [x = 0x10000000000000000]; }",
            "18446744073709552000",
        ),
        ("message M { optional double a = 1 [x = 1e400]; }", "null"),
    ] {
        let file = parse(source, None).expect("parse");
        let json = serde_json::to_string(&file).expect("a descriptor is JSON");
        assert!(
            json.contains(&format!("\"x\":{want}")),
            "for {source}\n  want \"x\":{want}\n  got  {json}"
        );
    }
}
