//! A small JSON value and writer for the files the importer generates.
//!
//! Calyx reads these files with serde, so any valid JSON would do; this
//! writer keeps keys in insertion order and distinguishes integers from
//! floats, writing floats as their shortest round-trip form (`1.0`, `0.3`,
//! `1e-05`), so a regenerated scene diffs cleanly against the last one.

use std::fmt::Write as _;

/// A JSON value with ordered object keys.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// An object from `(key, value)` pairs, in order.
    pub fn object<K: Into<String>>(entries: impl IntoIterator<Item = (K, Json)>) -> Json {
        Json::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// An array of floats.
    pub fn floats(values: impl IntoIterator<Item = f64>) -> Json {
        Json::Array(values.into_iter().map(Json::Float).collect())
    }

    /// An array of integers.
    pub fn ints(values: impl IntoIterator<Item = i64>) -> Json {
        Json::Array(values.into_iter().map(Json::Int).collect())
    }

    /// Adds or replaces `key` in an object, keeping the key's position when
    /// it is already there.
    pub fn insert(&mut self, key: impl Into<String>, value: Json) {
        let key = key.into();
        if let Json::Object(entries) = self {
            if let Some(entry) = entries.iter_mut().find(|(k, _)| *k == key) {
                entry.1 = value;
            } else {
                entries.push((key, value));
            }
        }
    }

    /// The text, indented by two spaces per level.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, Some(0));
        out
    }

    /// The text on one line.
    pub fn compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None);
        out
    }

    fn write(&self, out: &mut String, indent: Option<usize>) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Int(value) => {
                let _ = write!(out, "{value}");
            }
            Json::Float(value) => out.push_str(&float_text(*value)),
            Json::Str(value) => write_string(out, value),
            Json::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    separate(out, index, indent.map(|i| i + 1));
                    item.write(out, indent.map(|i| i + 1));
                }
                close(out, indent);
                out.push(']');
            }
            Json::Object(entries) => {
                if entries.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    separate(out, index, indent.map(|i| i + 1));
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, indent.map(|i| i + 1));
                }
                close(out, indent);
                out.push('}');
            }
        }
    }
}

fn separate(out: &mut String, index: usize, indent: Option<usize>) {
    match indent {
        Some(level) => {
            if index > 0 {
                out.push(',');
            }
            out.push('\n');
            out.push_str(&"  ".repeat(level));
        }
        None => {
            if index > 0 {
                out.push_str(", ");
            }
        }
    }
}

fn close(out: &mut String, indent: Option<usize>) {
    if let Some(level) = indent {
        out.push('\n');
        out.push_str(&"  ".repeat(level));
    }
}

fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for unit in value.encode_utf16() {
        match unit {
            0x22 => out.push_str("\\\""),
            0x5c => out.push_str("\\\\"),
            0x0a => out.push_str("\\n"),
            0x0d => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0x08 => out.push_str("\\b"),
            0x0c => out.push_str("\\f"),
            0x20..=0x7e => out.push(unit as u8 as char),
            _ => {
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out.push('"');
}

/// A float's shortest round-trip text: fixed notation with at least one
/// decimal for exponents from -4 to 15, otherwise scientific with a signed,
/// two-digit exponent (`1e-05`, `1.5e+16`).
pub fn float_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("`{:e}` writes an exponent");
    let exponent: i32 = exponent.parse().expect("`{:e}` writes an integer exponent");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let mut out = String::from(sign);
    if (-4..16).contains(&exponent) {
        if exponent < 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-exponent - 1) as usize));
            out.push_str(&digits);
        } else {
            let point = exponent as usize + 1;
            if digits.len() <= point {
                out.push_str(&digits);
                out.push_str(&"0".repeat(point - digits.len()));
                out.push_str(".0");
            } else {
                out.push_str(&digits[..point]);
                out.push('.');
                out.push_str(&digits[point..]);
            }
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(
            out,
            "e{}{:02}",
            if exponent < 0 { '-' } else { '+' },
            exponent.abs()
        );
    }
    out
}

/// `value` rounded to `places` decimal places, correctly rounded from its
/// exact binary value.
pub fn round_to(value: f64, places: usize) -> f64 {
    format!("{value:.places$}").parse().unwrap_or(value)
}

#[cfg(test)]
mod tests {
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
}
