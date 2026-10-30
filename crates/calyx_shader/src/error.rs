use std::io;
use std::path::PathBuf;

/// Why a shader failed to preprocess. Each variant names the file and the
/// line, from 1, where the failure is, and the include or parameter
/// involved.
#[derive(Debug, thiserror::Error)]
pub enum ShaderError {
    /// The loader could not read a file of the shader.
    #[error("{}: cannot read shader source: {source}", file.display())]
    Read {
        /// The file that could not be read.
        file: PathBuf,
        #[source]
        source: io::Error,
    },

    /// An include names a file that is already being expanded.
    #[error(
        "{}:{line}: `//#include \"{include}\"` includes a file that includes this one: \
         an include cycle",
        file.display()
    )]
    IncludeCycle {
        /// The file whose include closes the cycle.
        file: PathBuf,
        line: usize,
        /// The include as written.
        include: String,
    },

    /// The loader resolved an include to no file.
    #[error("{}:{line}: shader include \"{include}\" was not found", file.display())]
    IncludeNotFound {
        /// The file with the include.
        file: PathBuf,
        line: usize,
        /// The include as written.
        include: String,
    },

    /// A `//#param` line is not `//#param name: storage<T>`,
    /// `storage<T, access>` or `uniform<T>`.
    #[error(
        "{}:{line}: `{text}` is not a parameter declaration: write `//#param name: storage<T>`, \
         `storage<T, read_write>` or `uniform<T>`",
        file.display()
    )]
    MalformedParam {
        file: PathBuf,
        line: usize,
        /// The line as written, trimmed.
        text: String,
    },

    /// Two `//#param` lines of one shader declare the same name.
    #[error(
        "{}:{line}: parameter `{param}` is declared again; {}:{first_line} declares it first",
        file.display(),
        first_file.display()
    )]
    ParamDeclaredTwice {
        param: String,
        /// The second declaration.
        file: PathBuf,
        line: usize,
        /// The first declaration.
        first_file: PathBuf,
        first_line: usize,
    },

    /// A `//#binding` line is not `//#binding name`.
    #[error(
        "{}:{line}: `{text}` is not a binding: write `//#binding name`",
        file.display()
    )]
    MalformedBinding {
        file: PathBuf,
        line: usize,
        /// The line as written, trimmed.
        text: String,
    },

    /// A module declares a parameter that no `//#binding` of the shader
    /// binds.
    #[error(
        "{}:{line}: parameter `{param}` is bound to no `var` of {}; mark the `var` with \
         `//#binding {param}` on the line above it",
        file.display(),
        shader.display()
    )]
    UnboundParam {
        param: String,
        /// The file that declares the parameter.
        file: PathBuf,
        /// The line of its `//#param`.
        line: usize,
        /// The shader being preprocessed.
        shader: PathBuf,
    },

    /// A `//#binding` names a parameter no file of the shader declares.
    #[error(
        "{}:{line}: `//#binding {param}` names no parameter of the shader's modules",
        file.display()
    )]
    UnknownBinding {
        param: String,
        file: PathBuf,
        line: usize,
    },

    /// A `//#binding` is not directly above a `var` declaration, or above
    /// more `//#binding` lines that are.
    #[error(
        "{}:{line}: `//#binding {param}` is not directly followed by a `var` declaration",
        file.display()
    )]
    BindingWithoutVar {
        param: String,
        file: PathBuf,
        line: usize,
    },

    /// Two `//#binding` lines bind the same parameter.
    #[error(
        "{}:{line}: parameter `{param}` is bound again; {}:{first_line} binds it first",
        file.display(),
        first_file.display()
    )]
    BoundTwice {
        param: String,
        /// The second binding.
        file: PathBuf,
        line: usize,
        /// The first binding.
        first_file: PathBuf,
        first_line: usize,
    },

    /// Code names `#param`, but its file declares no such parameter.
    #[error(
        "{}:{line}: `#{param}` is not a parameter this file declares with `//#param`",
        file.display()
    )]
    UndeclaredParam {
        param: String,
        file: PathBuf,
        line: usize,
    },

    /// A `//#binding` binds a parameter to a `var` of another address
    /// space, access mode or type than the parameter declares.
    #[error(
        "{}:{line}: `//#binding {param}` binds `{}`, which is `{}`, but {}:{} declares \
         `{param}` as `{}`",
        file.display(),
        mismatch.var,
        mismatch.found,
        mismatch.declared_in.display(),
        mismatch.declared_line,
        mismatch.declared
    )]
    TypeMismatch {
        param: String,
        /// The file of the `//#binding`.
        file: PathBuf,
        /// The line of the `//#binding`.
        line: usize,
        /// The declared and the found type.
        mismatch: Box<TypeMismatch>,
    },
}

/// The two sides of a [`ShaderError::TypeMismatch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeMismatch {
    /// The name of the bound `var`.
    pub var: String,
    /// The parameter's type as declared, such as
    /// `storage<array<CurveKey>, read>`.
    pub declared: String,
    /// The `var`'s type in the same form.
    pub found: String,
    /// The file of the `//#param`.
    pub declared_in: PathBuf,
    /// The line of the `//#param`.
    pub declared_line: usize,
}
