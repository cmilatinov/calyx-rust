//! Shader parameters: resources a WGSL module uses without declaring their
//! binding, bound by the shader that includes it.
//!
//! A module declares a parameter with its address space and type:
//!
//! ```wgsl
//! //#param curve_storage: storage<array<CurveKey>>
//! ```
//!
//! The forms are `storage<T>` (read), `storage<T, read>`,
//! `storage<T, read_write>` and `uniform<T>`. Its code then names the
//! parameter as `#curve_storage`. The shader that includes the module marks
//! the `var` that fulfills each parameter with a `//#binding` line directly
//! above it; several `//#binding` lines may stack above one `var`:
//!
//! ```wgsl
//! //#binding curve_storage
//! @group(0) @binding(10) var<storage, read> particle_curve_keys: array<CurveKey>;
//! ```
//!
//! Preprocessing replaces each `#curve_storage` in the module's code, never
//! in its comments, with `particle_curve_keys`. It fails, naming the files
//! and the parameter, when a parameter is unbound, bound twice or bound to a
//! `var` of another address space, access or type (compared with whitespace
//! removed); when a `//#binding` names no parameter or is not directly above
//! a `var`; and when code names a `#identifier` its file does not declare.

use regex::Regex;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

/// One file of a shader's expansion.
pub(crate) struct SourceFile {
    pub path: PathBuf,
    pub text: String,
}

/// A run of one file's text in the expanded shader, between its includes.
pub(crate) struct Segment {
    /// Index of the file in the expansion's files.
    pub file: usize,
    /// Line of the file the run starts on, from 1.
    pub first_line: usize,
    pub text: String,
}

/// An address space, access mode and type, as a parameter declares it or a
/// `var` has it.
#[derive(Debug, PartialEq, Eq)]
struct ResourceType {
    space: String,
    access: String,
    /// The type, with whitespace removed.
    ty: String,
}

impl ResourceType {
    fn new(space: &str, access: Option<&str>, ty: &str) -> Self {
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
    fn parse_declared(text: &str) -> Option<Self> {
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

    fn display(&self) -> String {
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

fn error(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Checks the parameters and bindings of `files` and joins `segments` into
/// the shader's source, each parameter replaced by its bound `var`.
pub(crate) fn resolve(files: &[SourceFile], segments: &[Segment]) -> io::Result<String> {
    let path = |file: usize| files[file].path.display().to_string();

    let mut params: HashMap<String, Param> = HashMap::new();
    let mut declared: Vec<Vec<String>> = vec![Vec::new(); files.len()];
    for (file, source) in files.iter().enumerate() {
        for (index, line) in source.text.lines().enumerate() {
            let Some(captures) = param_regex().captures(line) else {
                if line.trim_start().starts_with("//#param") {
                    return Err(error(format!(
                        "{}:{}: a parameter is declared as `//#param name: storage<T>`, \
                         `storage<T, read_write>` or `uniform<T>`",
                        path(file),
                        index + 1
                    )));
                }
                continue;
            };
            let name = captures[1].to_owned();
            let Some(resource) = ResourceType::parse_declared(&captures[2]) else {
                return Err(error(format!(
                    "{}:{}: parameter `{name}` declares `{}`, not `storage<T>`, \
                     `storage<T, read_write>` or `uniform<T>`",
                    path(file),
                    index + 1,
                    &captures[2]
                )));
            };
            if let Some(other) = params.get(&name) {
                return Err(error(format!(
                    "parameter `{name}` is declared twice: {}:{} and {}:{}",
                    path(other.file),
                    other.line,
                    path(file),
                    index + 1
                )));
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
                    return Err(error(format!(
                        "{}:{}: a binding is marked as `//#binding name`",
                        path(file),
                        index + 1
                    )));
                }
                index += 1;
                continue;
            }
            let var = lines.get(index).and_then(|line| var_regex().captures(line));
            let Some(var) = var else {
                let (name, line) = &names[names.len() - 1];
                return Err(error(format!(
                    "{}:{line}: `//#binding {name}` is not directly followed by a `var` declaration",
                    path(file)
                )));
            };
            let resource = ResourceType::new(&var[1], var.get(2).map(|m| m.as_str()), &var[4]);
            for (name, line) in names {
                let Some(param) = params.get(&name) else {
                    return Err(error(format!(
                        "{}:{line}: `//#binding {name}` names no parameter of the shader's modules",
                        path(file)
                    )));
                };
                if let Some(other) = bindings.get(&name) {
                    return Err(error(format!(
                        "parameter `{name}` is bound twice: {}:{} and {}:{line}",
                        path(other.file),
                        other.line,
                        path(file)
                    )));
                }
                if param.resource != resource {
                    return Err(error(format!(
                        "parameter `{name}` of {} is declared `{}`, but {}:{line} binds it to \
                         `{}`, which is `{}`",
                        path(param.file),
                        param.resource.display(),
                        path(file),
                        &var[3],
                        resource.display()
                    )));
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
        return Err(error(format!(
            "{} binds no `var` to parameter `{name}`, which {}:{} declares; mark the `var` \
             with `//#binding {name}` on the line above it",
            path(0),
            path(param.file),
            param.line
        )));
    }

    let mut source = String::new();
    for segment in segments {
        let replacements: HashMap<&str, &str> = declared[segment.file]
            .iter()
            .map(|name| (name.as_str(), bindings[name].var.as_str()))
            .collect();
        substitute(&segment.text, &replacements, &mut source).map_err(|(line, name)| {
            error(format!(
                "{}:{}: `#{name}` is not a parameter this file declares with `//#param`",
                path(segment.file),
                segment.first_line + line
            ))
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
    use super::*;

    fn file(path: &str, text: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(path),
            text: text.to_owned(),
        }
    }

    /// Resolves a shader whose root, file 0, includes each module at its
    /// start, as the preprocessor's segments would order them.
    fn resolve_files(root: &str, modules: &[&str]) -> io::Result<String> {
        let mut files = vec![file("main.wgsl", root)];
        files.extend(
            modules
                .iter()
                .enumerate()
                .map(|(index, text)| file(&format!("module{index}.wgsl"), text)),
        );
        let mut segments: Vec<Segment> = (1..files.len())
            .map(|index| Segment {
                file: index,
                first_line: 1,
                text: files[index].text.clone(),
            })
            .collect();
        segments.push(Segment {
            file: 0,
            first_line: 1,
            text: root.to_owned(),
        });
        resolve(&files, &segments)
    }

    const CURVE_MODULE: &str = "//#param curve_storage: storage<array<CurveKey>>\n\
        // See #109: reads #curve_storage /* #nothing */ directly.\n\
        fn curve_key(index: u32) -> CurveKey { return #curve_storage[index]; }\n\
        fn other() -> u32 { return #curve_storage_count; }\n";

    fn message(result: io::Result<String>) -> String {
        result.expect_err("preprocessing fails").to_string()
    }

    #[test]
    fn a_parameter_becomes_its_bound_var_in_code_only() {
        let module = "//#param curve_storage: storage<array<CurveKey>>\n\
            //#param curve_storage_count: uniform<u32>\n\
            // See #109 and #curve_storage, in a comment.\n\
            /* #curve_storage /* nested #x */ still a comment */\n\
            fn curve_key(index: u32) -> CurveKey { return #curve_storage[index]; }\n\
            fn count() -> u32 { return #curve_storage_count; }\n";
        let root = "//#binding curve_storage\n\
            @group(0) @binding(10) var<storage, read> particle_curve_keys: array< CurveKey >;\n\
            //#binding curve_storage_count\n\
            @group(0) @binding(11) var<uniform> key_count: u32;\n";
        let source = resolve_files(root, &[module]).expect("the shader resolves");
        assert!(
            source.contains("return particle_curve_keys[index];"),
            "{source}"
        );
        assert!(source.contains("return key_count;"), "{source}");
        assert!(source.contains("// See #109 and #curve_storage, in a comment."));
        assert!(source.contains("/* #curve_storage /* nested #x */ still a comment */"));
        assert!(source.contains("//#param curve_storage: storage<array<CurveKey>>"));
        assert!(source.contains("//#binding curve_storage\n"));
    }

    #[test]
    fn several_parameters_may_bind_to_one_var() {
        let a = "//#param a_keys: storage<array<vec4f>>\nfn a() -> vec4f { return #a_keys[0]; }\n";
        let b = "//#param b_keys: storage<array<vec4f>>\nfn b() -> vec4f { return #b_keys[1]; }\n";
        let root = "//#binding a_keys\n//#binding b_keys\n\
            @group(0) @binding(0) var<storage> shared_keys: array<vec4f>;\n";
        let source = resolve_files(root, &[a, b]).expect("the shader resolves");
        assert!(source.contains("return shared_keys[0];"));
        assert!(source.contains("return shared_keys[1];"));
    }

    #[test]
    fn a_binding_must_match_the_declared_type() {
        let module = "//#param keys: storage<array<CurveKey>, read_write>\nfn f() { #keys[0] = CurveKey(); }\n";
        let matching =
            "//#binding keys\n@group(0) @binding(0) var<storage, read_write> k: array<CurveKey>;\n";
        resolve_files(matching, &[module]).expect("a matching var binds");

        let read =
            "//#binding keys\n@group(0) @binding(0) var<storage, read> k: array<CurveKey>;\n";
        let error = message(resolve_files(read, &[module]));
        assert!(error.contains("`keys`"), "{error}");
        assert!(
            error.contains("module0.wgsl") && error.contains("main.wgsl"),
            "{error}"
        );
        assert!(
            error.contains("storage<array<CurveKey>, read_write>"),
            "{error}"
        );
        assert!(error.contains("storage<array<CurveKey>, read>"), "{error}");

        let other_type =
            "//#binding keys\n@group(0) @binding(0) var<storage, read_write> k: array<vec4f>;\n";
        let error = message(resolve_files(other_type, &[module]));
        assert!(
            error.contains("storage<array<vec4f>, read_write>"),
            "{error}"
        );

        let uniform =
            "//#binding keys\n@group(0) @binding(0) var<uniform> k: array<CurveKey, 4>;\n";
        assert!(message(resolve_files(uniform, &[module])).contains("uniform<"));
    }

    #[test]
    fn an_unbound_parameter_fails() {
        let error = message(resolve_files("fn main() {}\n", &[CURVE_MODULE]));
        assert!(error.contains("`curve_storage`"), "{error}");
        assert!(
            error.contains("main.wgsl") && error.contains("module0.wgsl"),
            "{error}"
        );
    }

    #[test]
    fn a_binding_of_no_parameter_fails() {
        let root = "//#binding curve_storag\n@group(0) @binding(0) var<storage> k: array<f32>;\n";
        let error = message(resolve_files(root, &[]));
        assert!(
            error.contains("main.wgsl:1") && error.contains("curve_storag"),
            "{error}"
        );
    }

    #[test]
    fn a_binding_not_above_a_var_fails() {
        let module = "//#param keys: storage<array<f32>>\nfn f() -> f32 { return #keys[0]; }\n";
        for root in [
            "//#binding keys\n\n@group(0) @binding(0) var<storage> k: array<f32>;\n",
            "//#binding keys\nfn main() {}\n",
            "//#binding keys\n",
        ] {
            let error = message(resolve_files(root, &[module]));
            assert!(error.contains("main.wgsl:1"), "{error}");
            assert!(
                error.contains("not directly followed by a `var`"),
                "{error}"
            );
        }
    }

    #[test]
    fn a_parameter_bound_twice_fails() {
        let module = "//#param keys: storage<array<f32>>\nfn f() -> f32 { return #keys[0]; }\n";
        let root = "//#binding keys\n@group(0) @binding(0) var<storage> a: array<f32>;\n\
            //#binding keys\n@group(0) @binding(1) var<storage> b: array<f32>;\n";
        let error = message(resolve_files(root, &[module]));
        assert!(error.contains("bound twice"), "{error}");
        assert!(
            error.contains("main.wgsl:1") && error.contains("main.wgsl:3"),
            "{error}"
        );
    }

    #[test]
    fn an_undeclared_parameter_in_code_fails() {
        let root = "//#binding curve_storage\n\
            @group(0) @binding(0) var<storage> k: array<CurveKey>;\n";
        let error = message(resolve_files(root, &[CURVE_MODULE]));
        assert!(error.contains("module0.wgsl:4"), "{error}");
        assert!(error.contains("`#curve_storage_count`"), "{error}");

        // Another module's parameter is not this file's.
        let module = "//#param keys: storage<array<f32>>\nfn f() -> f32 { return #keys[0]; }\n";
        let root = "//#binding keys\n@group(0) @binding(0) var<storage> k: array<f32>;\n\
            fn g() -> f32 { return #keys[1]; }\n";
        let error = message(resolve_files(root, &[module]));
        assert!(
            error.contains("main.wgsl:3") && error.contains("`#keys`"),
            "{error}"
        );
    }

    #[test]
    fn a_malformed_parameter_fails() {
        let module = "//#param keys: buffer<array<f32>>\n";
        let error = message(resolve_files("", &[module]));
        assert!(
            error.contains("module0.wgsl:1") && error.contains("`keys`"),
            "{error}"
        );
        let module = "//#param keys\n";
        assert!(message(resolve_files("", &[module])).contains("module0.wgsl:1"));
    }

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
