//! A reader for Unity's serialized YAML: scenes, prefabs, materials and
//! `.meta` files.
//!
//! Unity writes a restricted, regular YAML that general YAML parsers trip on
//! (its `--- !u!<class> &<fileID>` document tags): documents headed
//! `--- !u!<class id> &<file id>` (with ` stripped` on the stubs of prefab
//! instance objects), block mappings and sequences, and one-line flow
//! mappings such as `{fileID: 2100000, guid: 58d5..., type: 2}`. This reader
//! handles exactly that. Every scalar stays a string; callers parse numbers.

use indexmap::IndexMap;

/// A parsed node. Scalars are kept as their text.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Str(String),
    List(Vec<Value>),
    Map(IndexMap<String, Value>),
}

impl Value {
    /// The value at `key` of a mapping.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(map) => map.get(key),
            _ => None,
        }
    }

    /// The text of a scalar.
    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    /// The text at `key`, if it is a scalar.
    pub fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::str)
    }

    /// The number at `key`.
    pub fn f64_at(&self, key: &str) -> Option<f64> {
        self.str_at(key).and_then(|text| text.trim().parse().ok())
    }

    /// The integer at `key`.
    pub fn i64_at(&self, key: &str) -> Option<i64> {
        self.str_at(key).and_then(|text| text.trim().parse().ok())
    }

    /// The items of a sequence; empty for anything else.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::List(items) => items,
            _ => &[],
        }
    }

    /// The entries of a mapping, in file order.
    pub fn entries(&self) -> impl Iterator<Item = (&String, &Value)> {
        match self {
            Value::Map(map) => Some(map.iter()),
            _ => None,
        }
        .into_iter()
        .flatten()
    }
}

/// One `--- !u!` document.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// Unity's class id, such as 1 for GameObject or 4 for Transform.
    pub class_id: i64,
    /// The object's file id, which other documents refer to it by.
    pub file_id: i64,
    /// Whether this is the stub of an object a prefab instance provides.
    pub stripped: bool,
    /// The class name the document's body is filed under.
    pub class_name: String,
    pub body: Value,
}

/// Parses an inline scalar: a flow mapping, a flow sequence, a quoted
/// string or plain text.
pub fn parse_scalar(text: &str) -> Value {
    let text = text.trim();
    if text.starts_with('{') && text.ends_with('}') {
        return parse_flow(text);
    }
    if text.starts_with('[') && text.ends_with(']') {
        let inner = text[1..text.len() - 1].trim();
        if inner.is_empty() {
            return Value::List(Vec::new());
        }
        return Value::List(inner.split(',').map(parse_scalar).collect());
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 2
        && bytes[0] == bytes[bytes.len() - 1]
        && (bytes[0] == b'\'' || bytes[0] == b'"')
    {
        return Value::Str(text[1..text.len() - 1].to_string());
    }
    Value::Str(text.to_string())
}

/// Parses `{a: 1, b: {c: 2}}`.
fn parse_flow(text: &str) -> Value {
    let text = text.trim();
    let inner = &text[1..text.len() - 1];
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (index, ch) in inner.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&inner[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    let mut map = IndexMap::new();
    for part in parts {
        if let Some((key, value)) = part.split_once(':') {
            map.insert(key.trim().to_string(), parse_scalar(value));
        }
    }
    Value::Map(map)
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn starts_item(line: &str) -> bool {
    line.trim_start().starts_with("- ")
}

/// Whether an item's body opens a mapping (`key: ...`) rather than a scalar.
fn opens_mapping(body: &str) -> bool {
    let Some(first) = body.chars().next() else {
        return false;
    };
    if matches!(first, '{' | '[' | '"' | '\'') {
        return false;
    }
    match body.find(':') {
        Some(colon) => {
            let after = &body[colon + 1..];
            after.is_empty() || after.starts_with(' ')
        }
        None => false,
    }
}

fn parse_block(lines: &mut [String], index: usize, indent: usize) -> (Value, usize) {
    if index < lines.len() && starts_item(&lines[index]) && indent_of(&lines[index]) == indent {
        let mut items = Vec::new();
        let mut index = index;
        while index < lines.len()
            && indent_of(&lines[index]) == indent
            && lines[index].trim_start().starts_with('-')
        {
            let body = lines[index].trim_start()[1..].trim_start().to_string();
            if body.is_empty() {
                let child = if index + 1 < lines.len() {
                    indent_of(&lines[index + 1])
                } else {
                    indent + 2
                };
                let (value, next) = parse_block(lines, index + 1, child);
                items.push(value);
                index = next;
                continue;
            }
            if opens_mapping(&body) {
                // A mapping item: re-read its first key at the item's inner indent.
                let inner = indent + 2;
                lines[index] = format!("{}{}", " ".repeat(inner), body);
                let (value, next) = parse_mapping(lines, index, inner);
                items.push(value);
                index = next;
            } else {
                items.push(parse_scalar(&body));
                index += 1;
            }
        }
        return (Value::List(items), index);
    }
    parse_mapping(lines, index, indent)
}

fn parse_mapping(lines: &mut [String], index: usize, indent: usize) -> (Value, usize) {
    let mut map = IndexMap::new();
    let mut index = index;
    while index < lines.len() {
        let line = lines[index].clone();
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        let current = indent_of(&line);
        if current < indent || (current == indent && starts_item(&line)) {
            break;
        }
        if current > indent {
            index += 1; // the continuation of a multi-line scalar
            continue;
        }
        let trimmed = line.trim();
        let (key, rest) = match trimmed.split_once(':') {
            Some((key, rest)) => (key.to_string(), rest.trim().to_string()),
            None => (trimmed.to_string(), String::new()),
        };
        if !rest.is_empty() {
            map.insert(key, parse_scalar(&rest));
            index += 1;
        } else {
            index += 1;
            let nested = index < lines.len()
                && (indent_of(&lines[index]) > indent
                    || (indent_of(&lines[index]) == indent && starts_item(&lines[index])));
            if nested {
                let child = indent_of(&lines[index]);
                let (value, next) = parse_block(lines, index, child);
                map.insert(key, value);
                index = next;
            } else {
                map.insert(key, Value::Null);
            }
        }
    }
    (Value::Map(map), index)
}

/// Parses a `--- !u!<class> &<file id>` header line.
fn parse_header(line: &str) -> Option<(i64, i64, bool)> {
    let rest = line.strip_prefix("--- !u!")?;
    let (class, rest) = rest.split_once(" &")?;
    let rest = rest.trim_end();
    let (file_id, stripped) = match rest.strip_suffix(" stripped") {
        Some(file_id) => (file_id, true),
        None => (rest, false),
    };
    Some((class.parse().ok()?, file_id.trim().parse().ok()?, stripped))
}

/// The documents of a Unity YAML file, in file order.
pub fn parse_documents(text: &str) -> Vec<Document> {
    let mut raw: Vec<(i64, i64, bool, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if let Some((class_id, file_id, stripped)) = parse_header(line) {
            raw.push((class_id, file_id, stripped, Vec::new()));
        } else if let Some(document) = raw.last_mut() {
            document.3.push(line.to_string());
        }
    }
    raw.into_iter()
        .filter_map(|(class_id, file_id, stripped, lines)| {
            let mut lines: Vec<String> =
                lines.into_iter().filter(|l| !l.starts_with('%')).collect();
            let first = lines.first()?.clone();
            let class_name = first.trim_end().trim_end_matches(':').trim().to_string();
            let body = if lines.len() > 1 {
                parse_mapping(&mut lines[1..], 0, 2).0
            } else {
                Value::Map(IndexMap::new())
            };
            Some(Document {
                class_id,
                file_id,
                stripped,
                class_name,
                body,
            })
        })
        .collect()
}

/// Parses a `.meta` file, whose top-level keys start at column 0.
pub fn parse_meta(text: &str) -> Value {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    parse_mapping(&mut lines, 0, 0).0
}

#[cfg(test)]
mod tests;
