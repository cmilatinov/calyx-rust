use crate::{
    dependencies, preprocess, MemoryLoader, Preprocessed, ShaderError, SourceLocation, TypeMismatch,
};
use std::path::{Path, PathBuf};

const CURVE_MODULE: &str = "//#param curve_storage: storage<array<CurveKey>>\n\
    // See #109: reads #curve_storage /* #nothing */ directly.\n\
    fn curve_key(index: u32) -> CurveKey { return #curve_storage[index]; }\n\
    fn other() -> u32 { return #curve_storage_count; }\n";

/// Preprocesses `main.wgsl`, whose text is `root`, with `module{i}.wgsl`
/// holding `modules[i]`, each included at the start of `root`.
fn preprocess_with(root: &str, modules: &[&str]) -> Result<Preprocessed, ShaderError> {
    let mut loader = MemoryLoader::new();
    let mut main = String::new();
    for (index, module) in modules.iter().enumerate() {
        loader.insert(format!("module{index}.wgsl"), *module);
        main.push_str(&format!("//#include \"module{index}.wgsl\"\n"));
    }
    main.push_str(root);
    loader.insert("main.wgsl", main);
    preprocess(&loader, Path::new("main.wgsl"))
}

fn source(root: &str, modules: &[&str]) -> String {
    preprocess_with(root, modules)
        .expect("the shader preprocesses")
        .source
}

fn failure(root: &str, modules: &[&str]) -> ShaderError {
    preprocess_with(root, modules).expect_err("preprocessing fails")
}

fn path(path: &str) -> PathBuf {
    PathBuf::from(path)
}

/// The number of lines `main.wgsl` spends on including `count` modules.
fn root_line(line: usize, modules: usize) -> usize {
    line + modules
}

#[test]
fn expands_shader_includes() {
    let loader = MemoryLoader::new().with("common.wgsl", "let a = 1u;").with(
        "shaders/main.wgsl",
        "before\n//#include \"common.wgsl\"\nafter",
    );
    let shader = preprocess(&loader, Path::new("shaders/main.wgsl")).expect("the shader expands");
    assert_eq!(shader.source, "before\nlet a = 1u;\nafter");
}

#[test]
fn expands_each_include_once_per_shader() {
    let loader = MemoryLoader::new()
        .with("common.wgsl", "struct A { a: u32 };")
        .with("left.wgsl", "//#include \"common.wgsl\"\nleft")
        .with("right.wgsl", "//#include \"common.wgsl\"\nright")
        .with(
            "main.wgsl",
            "//#include \"left.wgsl\"\n//#include \"right.wgsl\"\n//#include \"common.wgsl\"\nmain",
        );
    let shader = preprocess(&loader, Path::new("main.wgsl")).expect("the shader expands");
    // Each skipped include leaves its line empty.
    assert_eq!(shader.source, "struct A { a: u32 };\nleft\n\nright\n\nmain");
}

#[test]
fn collects_transitive_dependencies_in_include_order() {
    let loader = MemoryLoader::new()
        .with(
            "root.wgsl",
            "//#include \"nested.wgsl\"\n//#include \"main.wgsl\"",
        )
        .with("nested.wgsl", "let a = 1u;")
        .with("other.wgsl", "")
        .with(
            "main.wgsl",
            "//#include \"root.wgsl\"\n//#include \"other.wgsl\"\n//#include \"nested.wgsl\"",
        );
    let dependencies = dependencies(&loader, Path::new("main.wgsl")).expect("dependencies collect");
    assert_eq!(
        dependencies,
        [path("root.wgsl"), path("nested.wgsl"), path("other.wgsl")]
    );
}

#[test]
fn dependencies_of_a_module_need_no_binding() {
    let loader = MemoryLoader::new()
        .with("types.wgsl", "struct CurveKey { time: f32 };")
        .with(
            "curve.wgsl",
            format!("//#include \"types.wgsl\"\n{CURVE_MODULE}"),
        );
    let dependencies =
        dependencies(&loader, Path::new("curve.wgsl")).expect("dependencies collect");
    assert_eq!(dependencies, [path("types.wgsl")]);
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
    let source = source(root, &[module]);
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
    let source = source(root, &[a, b]);
    assert!(source.contains("return shared_keys[0];"));
    assert!(source.contains("return shared_keys[1];"));
}

#[test]
fn a_binding_matches_its_declared_type_without_whitespace() {
    let module =
        "//#param keys: storage<array<CurveKey>, read_write>\nfn f() { #keys[0] = CurveKey(); }\n";
    let root =
        "//#binding keys\n@group(0) @binding(0) var<storage,read_write> k: array< CurveKey >;\n";
    assert!(source(root, &[module]).contains("k[0] = CurveKey();"));
}

#[test]
fn the_source_map_names_the_file_and_line_of_each_line() {
    let loader = MemoryLoader::new()
        .with(
            "shaders/types.wgsl",
            "struct A { a: u32 };\nstruct B { b: u32 };\n",
        )
        .with(
            "shaders/curve.wgsl",
            "//#include \"shaders/types.wgsl\"\n//#param keys: storage<array<A>>\n\n\
             fn f() -> A { return #keys[0]; }",
        )
        .with(
            "main.wgsl",
            "// main\n//#include \"shaders/curve.wgsl\"\n//#include \"shaders/types.wgsl\"\n\
             //#binding keys\n@group(0) @binding(0) var<storage> k: array<A>;\n",
        );
    let shader = preprocess(&loader, Path::new("main.wgsl")).expect("the shader expands");
    let expected = [
        ("main.wgsl", 1),
        ("shaders/types.wgsl", 1),
        ("shaders/types.wgsl", 2),
        // What is left of the line of `curve.wgsl`'s include.
        ("shaders/curve.wgsl", 1),
        ("shaders/curve.wgsl", 2),
        ("shaders/curve.wgsl", 3),
        ("shaders/curve.wgsl", 4),
        // `types.wgsl` again, skipped.
        ("main.wgsl", 3),
        ("main.wgsl", 4),
        ("main.wgsl", 5),
        ("main.wgsl", 6),
    ];
    let lines: Vec<&str> = shader.source.split('\n').collect();
    assert_eq!(lines.len(), expected.len(), "{}", shader.source);
    assert_eq!(shader.source_map.len(), expected.len());
    for (index, (file, line)) in expected.into_iter().enumerate() {
        assert_eq!(
            shader.source_map.location(index + 1),
            Some(SourceLocation {
                file: Path::new(file),
                line
            }),
            "expanded line {}: {:?}",
            index + 1,
            lines[index]
        );
    }
    assert_eq!(lines[6], "fn f() -> A { return k[0]; }");
    assert_eq!(shader.source_map.location(0), None);
    assert_eq!(shader.source_map.location(expected.len() + 1), None);
    assert_eq!(
        shader.source_map.files(),
        [
            path("main.wgsl"),
            path("shaders/curve.wgsl"),
            path("shaders/types.wgsl")
        ]
    );
}

// One test per error.

#[test]
fn an_unreadable_shader_fails() {
    let error = preprocess(&MemoryLoader::new(), Path::new("main.wgsl")).unwrap_err();
    let ShaderError::Read { file, source } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(*file, path("main.wgsl"));
    assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
    assert!(error.to_string().starts_with("main.wgsl: "), "{error}");
}

#[test]
fn an_include_cycle_fails() {
    let loader = MemoryLoader::new()
        .with("a.wgsl", "//#include \"b.wgsl\"")
        .with("b.wgsl", "// b\n//#include \"a.wgsl\"");
    let error = preprocess(&loader, Path::new("a.wgsl")).unwrap_err();
    let ShaderError::IncludeCycle {
        file,
        line,
        include,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(
        (file, *line, include.as_str()),
        (&path("b.wgsl"), 2, "a.wgsl")
    );
    assert!(error.to_string().starts_with("b.wgsl:2: "), "{error}");
}

#[test]
fn a_missing_include_fails() {
    let loader = MemoryLoader::new().with("main.wgsl", "\n\n  //#include \"missing.wgsl\"  \n");
    let error = preprocess(&loader, Path::new("main.wgsl")).unwrap_err();
    let ShaderError::IncludeNotFound {
        file,
        line,
        include,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(
        (file, *line, include.as_str()),
        (&path("main.wgsl"), 3, "missing.wgsl")
    );
    assert!(
        error
            .to_string()
            .contains("main.wgsl:3: shader include \"missing.wgsl\""),
        "{error}"
    );
    assert!(matches!(
        dependencies(&loader, Path::new("main.wgsl")),
        Err(ShaderError::IncludeNotFound { line: 3, .. })
    ));
}

#[test]
fn a_malformed_parameter_fails() {
    for module in ["//#param keys: buffer<array<f32>>\n", "\n//#param keys\n"] {
        let error = failure("", &[module]);
        let ShaderError::MalformedParam { file, line, text } = &error else {
            panic!("{error:?}");
        };
        assert_eq!(*file, path("module0.wgsl"));
        assert_eq!(*line, module.lines().count());
        assert!(text.starts_with("//#param keys"), "{text}");
    }
}

#[test]
fn a_parameter_declared_twice_fails() {
    let a = "//#param keys: storage<array<f32>>\n";
    let b = "\n//#param keys: storage<array<f32>>\n";
    let error = failure("", &[a, b]);
    let ShaderError::ParamDeclaredTwice {
        param,
        file,
        line,
        first_file,
        first_line,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(param, "keys");
    assert_eq!((file, *line), (&path("module1.wgsl"), 2));
    assert_eq!((first_file, *first_line), (&path("module0.wgsl"), 1));
}

#[test]
fn a_malformed_binding_fails() {
    let error = failure(
        "//#binding\n@group(0) @binding(0) var<storage> k: array<f32>;\n",
        &[],
    );
    let ShaderError::MalformedBinding { file, line, text } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        (file, *line, text.as_str()),
        (&path("main.wgsl"), 1, "//#binding")
    );
}

#[test]
fn an_unbound_parameter_fails() {
    let error = failure("fn main() {}\n", &[CURVE_MODULE]);
    let ShaderError::UnboundParam {
        param,
        file,
        line,
        shader,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(param, "curve_storage");
    assert_eq!((file, *line), (&path("module0.wgsl"), 1));
    assert_eq!(*shader, path("main.wgsl"));
    let message = error.to_string();
    assert!(
        message.contains("module0.wgsl:1") && message.contains("main.wgsl"),
        "{message}"
    );
}

#[test]
fn a_binding_of_no_parameter_fails() {
    let root = "//#binding curve_storag\n@group(0) @binding(0) var<storage> k: array<f32>;\n";
    let error = failure(root, &[]);
    let ShaderError::UnknownBinding { param, file, line } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        (param.as_str(), file, *line),
        ("curve_storag", &path("main.wgsl"), 1)
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
        let error = failure(root, &[module]);
        let ShaderError::BindingWithoutVar { param, file, line } = &error else {
            panic!("{error:?}");
        };
        assert_eq!(
            (param.as_str(), file, *line),
            ("keys", &path("main.wgsl"), root_line(1, 1))
        );
        assert!(
            error
                .to_string()
                .contains("not directly followed by a `var`"),
            "{error}"
        );
    }
}

#[test]
fn a_parameter_bound_twice_fails() {
    let module = "//#param keys: storage<array<f32>>\nfn f() -> f32 { return #keys[0]; }\n";
    let root = "//#binding keys\n@group(0) @binding(0) var<storage> a: array<f32>;\n\
        //#binding keys\n@group(0) @binding(1) var<storage> b: array<f32>;\n";
    let error = failure(root, &[module]);
    let ShaderError::BoundTwice {
        param,
        file,
        line,
        first_file,
        first_line,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(param, "keys");
    assert_eq!((file, *line), (&path("main.wgsl"), root_line(3, 1)));
    assert_eq!(
        (first_file, *first_line),
        (&path("main.wgsl"), root_line(1, 1))
    );
}

#[test]
fn an_undeclared_parameter_in_code_fails() {
    let root = "//#binding curve_storage\n\
        @group(0) @binding(0) var<storage> k: array<CurveKey>;\n";
    let error = failure(root, &[CURVE_MODULE]);
    let ShaderError::UndeclaredParam { param, file, line } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        (param.as_str(), file, *line),
        ("curve_storage_count", &path("module0.wgsl"), 4)
    );

    // Another module's parameter is not this file's.
    let module = "//#param keys: storage<array<f32>>\nfn f() -> f32 { return #keys[0]; }\n";
    let root = "//#binding keys\n@group(0) @binding(0) var<storage> k: array<f32>;\n\
        fn g() -> f32 { return #keys[1]; }\n";
    let error = failure(root, &[module]);
    let ShaderError::UndeclaredParam { param, file, line } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        (param.as_str(), file, *line),
        ("keys", &path("main.wgsl"), root_line(3, 1))
    );
}

#[test]
fn a_binding_of_another_type_fails() {
    let module =
        "//#param keys: storage<array<CurveKey>, read_write>\nfn f() { #keys[0] = CurveKey(); }\n";
    let mismatch = |root: &str| {
        let error = failure(root, &[module]);
        let ShaderError::TypeMismatch {
            param,
            file,
            line,
            mismatch,
        } = &error
        else {
            panic!("{error:?}");
        };
        let TypeMismatch {
            var,
            declared,
            found,
            declared_in,
            declared_line,
        } = mismatch.as_ref();
        assert_eq!(param, "keys");
        assert_eq!((file, *line), (&path("main.wgsl"), root_line(1, 1)));
        assert_eq!(var, "k");
        assert_eq!((declared_in, *declared_line), (&path("module0.wgsl"), 1));
        assert_eq!(declared, "storage<array<CurveKey>, read_write>");
        let message = error.to_string();
        assert!(
            message.contains(declared.as_str()) && message.contains(found.as_str()),
            "{message}"
        );
        found.clone()
    };

    // Another access mode.
    let found =
        mismatch("//#binding keys\n@group(0) @binding(0) var<storage, read> k: array<CurveKey>;\n");
    assert_eq!(found, "storage<array<CurveKey>, read>");
    // Another type.
    let found = mismatch(
        "//#binding keys\n@group(0) @binding(0) var<storage, read_write> k: array<vec4f>;\n",
    );
    assert_eq!(found, "storage<array<vec4f>, read_write>");
    // Another address space.
    let found =
        mismatch("//#binding keys\n@group(0) @binding(0) var<uniform> k: array<CurveKey, 4>;\n");
    assert_eq!(found, "uniform<array<CurveKey,4>>");
}
