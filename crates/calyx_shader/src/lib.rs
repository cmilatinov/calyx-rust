//! The WGSL preprocessor of the Calyx engine.
//!
//! [`preprocess`] turns a shader file into the WGSL source wgpu compiles,
//! with a [`SourceMap`] from each line of that source back to the file and
//! line it came from. It is a pure source-to-source pass: a
//! [`ShaderLoader`] finds and reads the files, and every failure is a
//! [`ShaderError`] that names the file and line. The language is three
//! directives, each a WGSL line comment, so a file stays valid WGSL to
//! tools that do not know them.
//!
//! # `//#include`
//!
//! ```wgsl
//! //#include "shaders/common/curve.wgsl"
//! ```
//!
//! On a line of its own, an include is replaced by the text of the file it
//! names, which the loader resolves. Each file expands at most once per
//! shader, where it is first included: a later include of the same file,
//! directly or through another include, expands to nothing. So a module that
//! defines structs or functions can be included by every file that uses
//! it. A file that includes itself, directly or through other files, is an
//! [include cycle](ShaderError::IncludeCycle).
//!
//! # `//#param`
//!
//! A module can use a resource without binding it. It declares a parameter
//! on a line of its own, with its address space and type:
//!
//! ```wgsl
//! //#param curve_storage: storage<array<CurveKey>>
//! ```
//!
//! The forms are `storage<T>` (read), `storage<T, read>`,
//! `storage<T, read_write>` and `uniform<T>`. The module's code then names
//! the parameter with a `#`, which WGSL never uses, so the accessor lives in
//! the module:
//!
//! ```wgsl
//! fn curve_key(index: u32) -> CurveKey { return #curve_storage[index]; }
//! ```
//!
//! A name matches `#[A-Za-z_][A-Za-z0-9_]*`, and a file may name only the
//! parameters it declares itself.
//!
//! # `//#binding`
//!
//! The shader that includes the module declares the `var` that fulfills
//! each parameter, marked with a `//#binding` line directly above it.
//! Several `//#binding` lines may stack above one `var`:
//!
//! ```wgsl
//! //#binding curve_storage
//! @group(0) @binding(10) var<storage, read> particle_curve_keys: array<CurveKey>;
//! ```
//!
//! The `var`'s address space, access mode and type must match the
//! parameter's, compared with whitespace removed. Preprocessing then
//! replaces each `#curve_storage` in the module's code with
//! `particle_curve_keys`. Only code changes: `//` and `/* */` comments keep
//! their text, so "see #109" stays, and directive lines keep their `//#`
//! form.
//!
//! Every parameter of the shader's modules must be bound exactly once. One
//! module included twice still expands once, so it cannot take two
//! bindings of one parameter.

mod error;
mod params;
mod source_map;
#[cfg(test)]
mod tests;

pub use error::{ShaderError, TypeMismatch};
pub use source_map::{SourceLocation, SourceMap};

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Finds and reads the files of a shader.
///
/// The paths it returns identify files: two includes of one file must
/// resolve to equal paths, and the path given to [`preprocess`] should take
/// the same form, or include-once and cycle detection miss them.
pub trait ShaderLoader {
    /// The file `include`, as written in a `//#include` of `parent`, names,
    /// or `None` when it names none.
    fn resolve(&self, parent: &Path, include: &str) -> Option<PathBuf>;

    /// The source of `path`: the path given to [`preprocess`] or one
    /// [`resolve`](Self::resolve) returned.
    fn read(&self, path: &Path) -> io::Result<String>;
}

/// A loader over sources held in memory, by path. An include names a file
/// by its full path, whatever file includes it, as an include names a file
/// from an asset root in the engine.
#[derive(Debug, Clone, Default)]
pub struct MemoryLoader {
    files: HashMap<PathBuf, String>,
}

impl MemoryLoader {
    /// A loader with no file.
    pub fn new() -> Self {
        Self::default()
    }

    /// This loader with `source` as the file at `path`.
    pub fn with(mut self, path: impl Into<PathBuf>, source: impl Into<String>) -> Self {
        self.insert(path, source);
        self
    }

    /// Sets the file at `path` to `source`.
    pub fn insert(&mut self, path: impl Into<PathBuf>, source: impl Into<String>) {
        self.files.insert(path.into(), source.into());
    }
}

impl ShaderLoader for MemoryLoader {
    fn resolve(&self, _parent: &Path, include: &str) -> Option<PathBuf> {
        let path = PathBuf::from(include);
        self.files.contains_key(&path).then_some(path)
    }

    fn read(&self, path: &Path) -> io::Result<String> {
        self.files.get(path).cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no file {}", path.display()),
            )
        })
    }
}

/// A preprocessed shader.
#[derive(Debug, Clone)]
pub struct Preprocessed {
    /// The WGSL source, includes expanded and parameters replaced.
    pub source: String,
    /// The file and line each line of `source` came from.
    pub source_map: SourceMap,
}

/// Preprocesses the shader at `path`: expands its includes, checks its
/// modules' parameters against its bindings and replaces each parameter by
/// its bound `var`.
pub fn preprocess<L: ShaderLoader + ?Sized>(
    loader: &L,
    path: &Path,
) -> Result<Preprocessed, ShaderError> {
    let mut expansion = Expansion::default();
    let source = read(loader, path)?;
    expansion.expand(loader, path.to_path_buf(), source)?;
    let source = params::resolve(&expansion.files, &expansion.segments)?;
    let source_map = SourceMap::new(&expansion.files, &expansion.segments);
    Ok(Preprocessed { source, source_map })
}

/// The files the shader at `path` includes, directly or through other
/// includes, in the order they are first included. A change to any of them
/// changes the shader. Unlike [`preprocess`], it reads only includes, so it
/// lists a module's dependencies too.
pub fn dependencies<L: ShaderLoader + ?Sized>(
    loader: &L,
    path: &Path,
) -> Result<Vec<PathBuf>, ShaderError> {
    let mut visited = HashSet::from([path.to_path_buf()]);
    let mut dependencies = Vec::new();
    collect_dependencies(loader, path, &mut visited, &mut dependencies)?;
    Ok(dependencies)
}

fn collect_dependencies<L: ShaderLoader + ?Sized>(
    loader: &L,
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    dependencies: &mut Vec<PathBuf>,
) -> Result<(), ShaderError> {
    let source = read(loader, path)?;
    for include in includes(&source) {
        let dependency = resolve(loader, path, &include)?;
        if visited.insert(dependency.clone()) {
            dependencies.push(dependency.clone());
            collect_dependencies(loader, &dependency, visited, dependencies)?;
        }
    }
    Ok(())
}

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

/// An `//#include` of a file's source.
struct Include {
    /// The directive's text, which the include's file replaces.
    range: Range<usize>,
    /// The directive's line, from 1.
    line: usize,
    /// The included path as written.
    path: String,
}

/// The `//#include` directives of `source`, in order.
fn includes(source: &str) -> impl Iterator<Item = Include> + '_ {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    let regex = REGEX.get_or_init(|| {
        Regex::new(r#"(?m)^\s*//#include\s+"([^"]*)"\s*$"#).expect("the include regex compiles")
    });
    regex.captures_iter(source).map(|captures| {
        let directive = captures.get(0).expect("a match has its text");
        let path = captures.get(1).expect("an include names a path");
        Include {
            range: directive.range(),
            line: line_of(source, path.start()),
            path: path.as_str().to_owned(),
        }
    })
}

/// The line, from 1, of byte `offset` of `source`.
fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].matches('\n').count() + 1
}

fn read<L: ShaderLoader + ?Sized>(loader: &L, path: &Path) -> Result<String, ShaderError> {
    loader.read(path).map_err(|source| ShaderError::Read {
        file: path.to_path_buf(),
        source,
    })
}

fn resolve<L: ShaderLoader + ?Sized>(
    loader: &L,
    parent: &Path,
    include: &Include,
) -> Result<PathBuf, ShaderError> {
    loader
        .resolve(parent, &include.path)
        .ok_or_else(|| ShaderError::IncludeNotFound {
            file: parent.to_path_buf(),
            line: include.line,
            include: include.path.clone(),
        })
}

/// The files of one shader's expansion.
#[derive(Default)]
struct Expansion {
    /// Files being expanded, to report cycles.
    stack: HashSet<PathBuf>,
    /// Files expanded so far, which later includes skip.
    expanded: HashSet<PathBuf>,
    /// Each file expanded, in order.
    files: Vec<SourceFile>,
    /// The shader's text, as runs of its files' text between includes.
    segments: Vec<Segment>,
}

impl Expansion {
    /// Appends the file at `path`, whose text is `source`, with its
    /// includes expanded.
    fn expand<L: ShaderLoader + ?Sized>(
        &mut self,
        loader: &L,
        path: PathBuf,
        source: String,
    ) -> Result<(), ShaderError> {
        self.stack.insert(path.clone());
        self.expanded.insert(path.clone());
        let file = self.files.len();
        self.files.push(SourceFile {
            path: path.clone(),
            text: source.clone(),
        });
        let mut last_end = 0;
        for include in includes(&source) {
            self.push_segment(file, &source, last_end..include.range.start);
            last_end = include.range.end;
            let included = resolve(loader, &path, &include)?;
            if self.stack.contains(&included) {
                return Err(ShaderError::IncludeCycle {
                    file: path,
                    line: include.line,
                    include: include.path,
                });
            }
            if self.expanded.contains(&included) {
                continue;
            }
            let included_source = read(loader, &included)?;
            self.expand(loader, included, included_source)?;
        }
        self.push_segment(file, &source, last_end..source.len());
        self.stack.remove(&path);
        Ok(())
    }

    /// Appends `range` of `file`'s `source` to the shader's text.
    fn push_segment(&mut self, file: usize, source: &str, range: Range<usize>) {
        self.segments.push(Segment {
            file,
            first_line: line_of(source, range.start),
            text: source[range].to_owned(),
        });
    }
}
