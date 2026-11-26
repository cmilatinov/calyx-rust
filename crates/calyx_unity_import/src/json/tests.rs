use super::*;

#[test]
fn floats_keep_a_decimal_and_switch_to_scientific_far_from_one() {
    assert_eq!(float_text(1.0), "1.0");
    assert_eq!(float_text(0.3), "0.3");
    assert_eq!(float_text(-0.0), "-0.0");
    assert_eq!(float_text(84.16901), "84.16901");
    assert_eq!(float_text(1500.0), "1500.0");
    assert_eq!(float_text(0.0001), "0.0001");
    assert_eq!(float_text(0.00001), "1e-05");
    assert_eq!(float_text(-1.5e-7), "-1.5e-07");
    assert_eq!(float_text(1e16), "1e+16");
    assert_eq!(float_text(123456789012345.6), "123456789012345.6");
}

#[test]
fn objects_keep_their_order_and_indent_by_two_spaces() {
    let value = Json::object([
        ("b", Json::Int(1)),
        ("a", Json::floats([0.5, 2.0])),
        ("empty", Json::Array(vec![])),
        ("none", Json::Null),
    ]);
    assert_eq!(
        value.pretty(),
        "{\n  \"b\": 1,\n  \"a\": [\n    0.5,\n    2.0\n  ],\n  \"empty\": [],\n  \"none\": null\n}"
    );
    assert_eq!(
        value.compact(),
        "{\"b\": 1, \"a\": [0.5, 2.0], \"empty\": [], \"none\": null}"
    );
}

#[test]
fn strings_escape_quotes_and_non_ascii() {
    assert_eq!(
        Json::Str("Flowers&Plants \"é\"".into()).compact(),
        "\"Flowers&Plants \\\"\\u00e9\\\"\""
    );
}

#[test]
fn rounding_follows_the_decimal_places() {
    assert_eq!(round_to(1.5973, 3), 1.597);
    assert_eq!(round_to(0.123456789, 6), 0.123457);
    assert_eq!(round_to(36.0, 3), 36.0);
}
