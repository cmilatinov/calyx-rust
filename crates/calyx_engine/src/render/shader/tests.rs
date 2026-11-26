use super::Shader;
use crate::assets::Asset;
use crate::render::AssetShaderLoader;
use crate::render::PARTICLE_KERNELS;
use crate::test_support::TestAssets;
use calyx_shader::MemoryLoader;
use std::collections::HashSet;
use std::path::Path;

/// naga reports a parse error in the expanded source; the shader's
/// error names the included module's file and line instead.
#[test]
fn a_parse_error_in_an_included_module_names_its_file_and_line() {
    let loader = MemoryLoader::new()
        .with(
            "shaders/module.wgsl",
            "// A module.\n//#param keys: storage<array<f32>>\n\n\
                 fn key(index: u32) -> f32 {\n    return #keys[index] +;\n}\n",
        )
        .with(
            "main.wgsl",
            "// The shader.\n//#include \"shaders/module.wgsl\"\n\
                 //#binding keys\n\
                 @group(0) @binding(0) var<storage> particle_keys: array<f32>;\n",
        );
    let shader =
        calyx_shader::preprocess(&loader, Path::new("main.wgsl")).expect("the shader preprocesses");
    let error = naga::front::wgsl::parse_str(&shader.source).expect_err("naga rejects it");
    let expanded = error
        .location(&shader.source)
        .expect("the error has a location")
        .line_number;
    assert_eq!(expanded, 6, "{}", shader.source);

    let description = Shader::describe_parse_error(&error, &shader);
    let first = description.lines().next().unwrap_or_default();
    assert!(first.starts_with("shaders/module.wgsl:5:"), "{description}");
    assert!(first.contains(error.message()), "{description}");
    assert!(
        description.contains("return particle_keys[index] +;"),
        "{description}"
    );
    assert!(!description.contains("main.wgsl"), "{description}");
}

fn bindings(shader: &Shader, entry_point: &str) -> Vec<(u32, u32)> {
    shader.compute_entry_points[entry_point]
        .iter()
        .flat_map(|(group, entries)| entries.iter().map(|entry| (*group, entry.binding)))
        .collect()
}

fn wgsl_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("the asset dir is readable") {
        let path = entry.expect("the asset dir lists").path();
        if path.is_dir() {
            wgsl_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "wgsl") {
            files.push(path);
        }
    }
}

/// Every shader of the engine's and the sandbox's assets loads. A file
/// another file includes is a module, such as `curve.wgsl` with its
/// parameters or `pbr_utils.wgsl` with the constants of the shader
/// that includes it, and loads through the shaders that include it.
#[test]
fn every_shader_of_the_assets_loads() {
    let assets = TestAssets::game();
    let context = assets.asset_context().lock_read();
    let mut files = Vec::new();
    for root in assets.roots() {
        wgsl_files(&root, &mut files);
    }
    let files: Vec<_> = files
        .iter()
        .map(|path| std::fs::canonicalize(path).expect("the shader exists"))
        .collect();
    let mut modules = HashSet::new();
    {
        let registry = context.registries.assets.read();
        let loader = AssetShaderLoader::new(&registry);
        for path in &files {
            let dependencies = loader
                .dependencies(path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            modules.extend(dependencies);
        }
    }
    let shaders: Vec<_> = files
        .iter()
        .filter(|path| !modules.contains(*path))
        .collect();
    for path in &shaders {
        if let Err(error) = Shader::from_file(&context, path) {
            panic!("{} does not load: {error}", path.display());
        }
    }
    assert!(
        modules.iter().any(|path| path.ends_with("curve.wgsl")),
        "{modules:?}"
    );
    assert!(shaders.len() > 10, "only {} shaders", shaders.len());
}

#[test]
fn each_compute_entry_point_binds_only_what_it_uses_and_is_cached() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let shader = context
        .registries
        .assets
        .read()
        .load::<Shader>(PARTICLE_KERNELS)
        .expect("the particle kernels should load");
    let shader = shader.read();

    assert_eq!(bindings(&shader, "update_main"), [(0, 0), (0, 1), (0, 3)]);
    assert_eq!(
        bindings(&shader, "scatter_main"),
        [(0, 0), (0, 1), (0, 2), (0, 3)]
    );
    assert_eq!(bindings(&shader, "sort_main"), [(0, 4), (0, 6), (0, 9)]);

    let update = shader.compute_pipeline("update_main").unwrap();
    assert_eq!(update.bind_group_layouts.len(), 1);
    assert!(shader.bind_group_layouts.is_empty());
    assert_eq!(
        shader.compute_pipeline("update_main").unwrap().pipeline,
        update.pipeline
    );
    let sort = shader.compute_pipeline("sort_main").unwrap();
    assert_ne!(sort.pipeline, update.pipeline);
    assert!(shader.compute_pipeline("vs_main").is_none());
    assert_eq!(shader.compute_pipeline_count(), 2);
}

#[test]
fn an_entry_point_using_every_binding_keeps_the_shader_layout() {
    let assets = TestAssets::engine();
    let context = assets.asset_context().lock_read();
    let shader = context
        .registries
        .assets
        .read()
        .load::<Shader>("shaders/utility/mip_generator_cube")
        .expect("the mip generator should load");
    let shader = shader.read();
    let kernel = shader.compute_pipeline("compute_main").unwrap();
    assert_eq!(kernel.bind_group_layouts, shader.bind_group_layouts);
}

mod hot_reload;
mod kernel_limit;
