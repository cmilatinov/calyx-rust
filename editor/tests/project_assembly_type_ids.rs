//! Reflection downcasts compare `TypeId`s, and the editor runs them on values
//! the project assembly creates. The assembly is a DLL that links its own
//! copy of `engine`, so these tests load the sandbox assembly the way the
//! editor does and check that engine types keep their `TypeId` on both sides
//! of that boundary: trait metadata the assembly registers, and engine-typed
//! fields of components the assembly constructs, downcast in the editor.

use engine::component::ReflectComponent;
use engine::reflect::type_registry::{PluginEngineBuild, TypeRegistry};
use engine::reflect::{Reflect, ReflectDefault, TypeInfo};
use engine::scene::GameObjectRef;
use libloading::{Library, Symbol};
use std::any::TypeId;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The target directory this test binary was built into, which is the one
/// the editor builds the assembly into.
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .expect("the test scratch directory sits inside the target directory")
        .to_path_buf()
}

/// Builds the sandbox assembly with the editor's own cargo arguments, once
/// per test run, and returns the path of its library.
fn sandbox_assembly() -> &'static Path {
    static ASSEMBLY: OnceLock<PathBuf> = OnceLock::new();
    ASSEMBLY.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("editor sits inside the workspace");
        let target = target_dir();
        let args = editor::project_assembly_build_args("dev", &target);
        let output = Command::new(env!("CARGO"))
            .current_dir(root.join("sandbox"))
            .args(&args)
            .output()
            .expect("cargo runs");
        assert!(
            output.status.success(),
            "building the sandbox assembly failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let library = target
            .join("debug")
            .join(libloading::library_filename("sandbox"));
        assert!(library.exists(), "{} was built", library.display());
        library
    })
}

/// A host registry holding the editor's types and the sandbox assembly's,
/// registered through the assembly's `plugin_main` as the editor does.
///
/// The library is returned alongside so it stays loaded while the
/// registrations, whose code lives in it, are used.
fn load_sandbox() -> (Library, TypeRegistry) {
    let library = unsafe { Library::new(sandbox_assembly()) }.expect("the sandbox assembly loads");
    let mut registry = TypeRegistry::new();
    registry.begin_plugin_load();
    unsafe {
        let plugin_main: Symbol<extern "C" fn(&mut TypeRegistry)> = library
            .get(b"plugin_main\0")
            .expect("the sandbox assembly exports plugin_main");
        plugin_main(&mut registry);
    }
    (library, registry)
}

/// The type UUID the assembly registered for the type named `type_name`.
fn type_uuid_named(registry: &TypeRegistry, type_name: &str) -> uuid::Uuid {
    registry
        .types
        .iter()
        .find(|(_, registration)| match &registration.type_info {
            TypeInfo::Struct(info) => info.type_name == type_name,
            _ => false,
        })
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("the sandbox assembly registers {type_name}"))
}

#[test]
fn the_sandbox_assembly_links_the_editors_engine_build() {
    let (_library, registry) = load_sandbox();
    // `register_crate` records `TypeId::of::<TypeRegistry>()` as the assembly
    // computes it; `Same` means it equals the editor's.
    assert_eq!(registry.plugin_engine_build(), PluginEngineBuild::Same);
}

#[test]
fn engine_types_keep_their_type_id_across_the_assembly_boundary() {
    let (_library, registry) = load_sandbox();
    let sentry = type_uuid_named(&registry, "sandbox::sentry::ComponentSentry");

    // Trait metadata the assembly built downcasts in the editor.
    let default = registry
        .trait_meta::<ReflectDefault>(sentry)
        .expect("the assembly's ReflectDefault downcasts in the editor");
    let component = registry
        .trait_meta::<ReflectComponent>(sentry)
        .expect("the assembly's ReflectComponent downcasts in the editor");

    // A component the assembly constructs, read through its reflected field.
    let instance: Box<dyn Reflect> = default.default();
    assert!(component.get(instance.as_ref()).is_some());
    let Some(TypeInfo::Struct(info)) = registry.type_info_by_id(sentry) else {
        panic!("the sentry is a struct");
    };
    let target = info
        .field("target")
        .expect("the sentry reflects its target")
        .get_reflect(instance.as_ref())
        .expect("the target field reads");

    // The assembly's vtable reports the `TypeId` its copy of engine assigns to
    // `GameObjectRef`; it is the one the editor assigns.
    assert_eq!(target.as_any().type_id(), TypeId::of::<GameObjectRef>());
    assert!(target.as_any().downcast_ref::<GameObjectRef>().is_some());
    assert!(target.downcast_ref::<GameObjectRef>().is_some());
    assert!(target.downcast_ref::<f32>().is_none());

    // The project type itself is not one the editor knows.
    assert_ne!(instance.as_any().type_id(), TypeId::of::<GameObjectRef>());
    assert!(instance.downcast_ref::<GameObjectRef>().is_none());
}
