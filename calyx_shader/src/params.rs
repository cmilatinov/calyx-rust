//! Module parameters: `//#param` declarations, the `//#binding` lines that
//! bind them and the `#name` references replaced by the bound `var`.

use crate::{Segment, ShaderError, SourceFile, TypeMismatch};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// An address space, access mode and type, as a parameter declares it or a
/// `var` has it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ResourceType {
    space: String,
    access: String,
    /// The type, with whitespace removed.
    ty: String,
}

impl ResourceType {
    pub(crate) fn new(space: &str, access: Option<&str>, ty: &str) -> Self {
        let access = match (space, access) {
            (_, Some(access)) => access.to_owned(),
            ("storage", None) => "read".to_owned(),
            _ => String::new(),
        };
        Self {
            space: space.to_owned(),
            access,
            ty: ty.chars().filter(|c| !c.is_whitespace()).collect(),
        }
    }

    /// The type of a `//#param` declaration: `storage<T>`,
    /// `storage<T, access>` or `uniform<T>`.
    pub(crate) fn parse_declared(text: &str) -> Option<Self> {
        let text = text.trim();
        let (space, inner) = ["storage", "uniform"].into_iter().find_map(|space| {
            let inner = text.strip_prefix(space)?.trim_start().strip_prefix('<')?;
            Some((space, inner.strip_suffix('>')?))
        })?;
        // The access mode follows the type's last top-level comma.
        let mut depth = 0;
        let mut split = None;
        for (index, char) in inner.char_indices() {
            match char {
                '<' => depth += 1,
                '>' => depth -= 1,
                ',' if depth == 0 => split = Some(index),
                _ => {}
            }
        }
        let (ty, access) = match split {
            Some(index) => {
                let access = inner[index + 1..].trim();
                if !matches!(access, "read" | "write" | "read_write") || space != "storage" {
                    return None;
                }
                (&inner[..index], Some(access))
            }
            None => (inner, None),
        };
        let ty = ty.trim();
        (!ty.is_empty()).then(|| Self::new(space, access, ty))
    }

    pub(crate) fn display(&self) -> String {
        if self.access.is_empty() {
            format!("{}<{}>", self.space, self.ty)
        } else {
            format!("{}<{}, {}>", self.space, self.ty, self.access)
        }
    }
}

/// A `//#param` declaration.
struct Param {
    file: usize,
    line: usize,
    resource: ResourceType,
}

/// A `var` a `//#binding` line binds a parameter to.
struct Binding {
    file: usize,
    line: usize,
    var: String,
}

fn param_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"^\s*//#param\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.+?)\s*$")
            .expect("the param regex compiles")
    })
}

fn binding_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"^\s*//#binding\s+([A-Za-z_][A-Za-z0-9_]*)\s*$")
            .expect("the binding regex compiles")
    })
}

fn var_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"^\s*(?:@[A-Za-z_]+(?:\([^)]*\))?\s*)*var\s*<\s*([A-Za-z_]+)\s*(?:,\s*([A-Za-z_]+)\s*)?>\s*([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.+?)\s*;",
        )
        .expect("the var regex compiles")
    })
}

/// Checks the parameters and bindings of `files` and joins `segments` into
/// the shader's source, each parameter replaced by its bound `var`.
pub(crate) fn resolve(files: &[SourceFile], segments: &[Segment]) -> Result<String, ShaderError> {
    let path = |file: usize| files[file].path.clone();

    let mut params: HashMap<String, Param> = HashMap::new();
    let mut declared: Vec<Vec<String>> = vec![Vec::new(); files.len()];
    for (file, source) in files.iter().enumerate() {
        for (index, line) in source.text.lines().enumerate() {
            let malformed = || ShaderError::MalformedParam {
                file: path(file),
                line: index + 1,
                text: line.trim().to_owned(),
            };
            let Some(captures) = param_regex().captures(line) else {
                if line.trim_start().starts_with("//#param") {
                    return Err(malformed());
                }
                continue;
            };
            let name = captures[1].to_owned();
            let Some(resource) = ResourceType::parse_declared(&captures[2]) else {
                return Err(malformed());
            };
            if let Some(other) = params.get(&name) {
                return Err(ShaderError::ParamDeclaredTwice {
                    param: name,
                    file: path(file),
                    line: index + 1,
                    first_file: path(other.file),
                    first_line: other.line,
                });
            }
            declared[file].push(name.clone());
            params.insert(
                name,
                Param {
                    file,
                    line: index + 1,
                    resource,
                },
            );
        }
    }

    let mut bindings: HashMap<String, Binding> = HashMap::new();
    for (file, source) in files.iter().enumerate() {
        let lines: Vec<&str> = source.text.lines().collect();
        let mut index = 0;
        while index < lines.len() {
            let mut names = Vec::new();
            while let Some(captures) = lines.get(index).and_then(|l| binding_regex().captures(l)) {
                names.push((captures[1].to_owned(), index + 1));
                index += 1;
            }
            if names.is_empty() {
                if lines[index].trim_start().starts_with("//#binding") {
                    return Err(ShaderError::MalformedBinding {
                        file: path(file),
                        line: index + 1,
                        text: lines[index].trim().to_owned(),
                    });
                }
                index += 1;
                continue;
            }
            let var = lines.get(index).and_then(|line| var_regex().captures(line));
            let Some(var) = var else {
                let (name, line) = names.pop().expect("a binding was read");
                return Err(ShaderError::BindingWithoutVar {
                    param: name,
                    file: path(file),
                    line,
                });
            };
            let resource = ResourceType::new(&var[1], var.get(2).map(|m| m.as_str()), &var[4]);
            for (name, line) in names {
                let Some(param) = params.get(&name) else {
                    return Err(ShaderError::UnknownBinding {
                        param: name,
                        file: path(file),
                        line,
                    });
                };
                if let Some(other) = bindings.get(&name) {
                    return Err(ShaderError::BoundTwice {
                        param: name,
                        file: path(file),
                        line,
                        first_file: path(other.file),
                        first_line: other.line,
                    });
                }
                if param.resource != resource {
                    return Err(ShaderError::TypeMismatch {
                        param: name,
                        file: path(file),
                        line,
                        mismatch: Box::new(TypeMismatch {
                            var: var[3].to_owned(),
                            declared: param.resource.display(),
                            found: resource.display(),
                            declared_in: path(param.file),
                            declared_line: param.line,
                        }),
                    });
                }
                bindings.insert(
                    name,
                    Binding {
                        file,
                        line,
                        var: var[3].to_owned(),
                    },
                );
            }
            index += 1;
        }
    }

    let mut unbound: Vec<(&String, &Param)> = params
        .iter()
        .filter(|(name, _)| !bindings.contains_key(*name))
        .collect();
    unbound.sort_by_key(|(name, _)| name.as_str());
    if let Some((name, param)) = unbound.first() {
        return Err(ShaderError::UnboundParam {
            param: (*name).clone(),
            file: path(param.file),
            line: param.line,
            shader: path(0),
        });
    }

    let mut source = String::new();
    for segment in segments {
        let replacements: HashMap<&str, &str> = declared[segment.file]
            .iter()
            .map(|name| (name.as_str(), bindings[name].var.as_str()))
            .collect();
        substitute(&segment.text, &replacements, &mut source).map_err(|(line, name)| {
            ShaderError::UndeclaredParam {
                param: name,
                file: path(segment.file),
                line: segment.first_line + line,
            }
        })?;
    }
    Ok(source)
}

/// Appends `text` to `out` with each `#name` in its code, outside comments,
/// replaced by `replacements[name]`. Fails with the line, from 0, and name
/// of a `#name` that has no replacement.
fn substitute(
    text: &str,
    replacements: &HashMap<&str, &str>,
    out: &mut String,
) -> Result<(), (usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut line = 0;
    let mut block_depth = 0;
    let mut index = 0;
    while index < chars.len() {
        let char = chars[index];
        let next = chars.get(index + 1).copied();
        if char == '\n' {
            line += 1;
        }
        if block_depth > 0 {
            match (char, next) {
                ('*', Some('/')) => {
                    block_depth -= 1;
                    out.push_str("*/");
                    index += 2;
                }
                ('/', Some('*')) => {
                    block_depth += 1;
                    out.push_str("/*");
                    index += 2;
                }
                _ => {
                    out.push(char);
                    index += 1;
                }
            }
            continue;
        }
        match (char, next) {
            ('/', Some('/')) => {
                let end = chars[index..]
                    .iter()
                    .position(|&c| c == '\n')
                    .map_or(chars.len(), |offset| index + offset);
                out.extend(&chars[index..end]);
                index = end;
            }
            ('/', Some('*')) => {
                block_depth += 1;
                out.push_str("/*");
                index += 2;
            }
            ('#', Some(start)) if start == '_' || start.is_ascii_alphabetic() => {
                let end = chars[index + 1..]
                    .iter()
                    .position(|&c| !(c == '_' || c.is_ascii_alphanumeric()))
                    .map_or(chars.len(), |offset| index + 1 + offset);
                let name: String = chars[index + 1..end].iter().collect();
                let Some(var) = replacements.get(name.as_str()) else {
                    return Err((line, name));
                };
                out.push_str(var);
                index = end;
            }
            _ => {
                out.push(char);
                index += 1;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ResourceType;

    #[test]
    fn declared_types_parse_with_their_defaults() {
        let read = ResourceType::parse_declared("storage<array<vec4f>>").unwrap();
        assert_eq!(
            read,
            ResourceType::new("storage", Some("read"), "array<vec4f>")
        );
        let write =
            ResourceType::parse_declared("storage< array<vec4f, 4> , read_write >").unwrap();
        assert_eq!(
            write,
            ResourceType::new("storage", Some("read_write"), "array<vec4f,4>")
        );
        let uniform = ResourceType::parse_declared("uniform<Params>").unwrap();
        assert_eq!(uniform.display(), "uniform<Params>");
        assert!(ResourceType::parse_declared("uniform<T, read>").is_none());
        assert!(ResourceType::parse_declared("storage<>").is_none());
    }
}
