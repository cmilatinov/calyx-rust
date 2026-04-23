use crate::assets::AssetRegistry;
use crate::class_registry::ComponentRegistry;
use crate::context::{AssetContext, GameContext, ReadOnlyRegistryContext, RegistryContext};
use crate::core::Ref;
use crate::reflect::type_registry::TypeRegistry;
use crate::render::RenderContext;
use crate::scene::Scene;
use egui_wgpu::wgpu;
use std::path::PathBuf;
use std::sync::Arc;

pub fn test_render_context() -> Arc<RenderContext> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("no wgpu adapter found");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default(), None))
            .expect("failed to create wgpu device");
    Arc::new(RenderContext::headless(Arc::new(device), Arc::new(queue)))
}

fn build_type_registry() -> Ref<TypeRegistry> {
    let mut type_registry = TypeRegistry::new();
    for f in inventory::iter::<crate::ReflectRegistrationFn> {
        (f.function)(&mut type_registry);
    }
    Ref::new(type_registry)
}

pub fn test_registries() -> ReadOnlyRegistryContext {
    let render_context = test_render_context();
    let type_registry = build_type_registry();
    let component_registry = Ref::new(ComponentRegistry::new(&type_registry.read()));
    let asset_registry = AssetRegistry::new_test(
        PathBuf::from("."),
        render_context,
        type_registry.clone(),
        component_registry.clone(),
    );
    ReadOnlyRegistryContext {
        assets: asset_registry.readonly(),
        types: type_registry.readonly(),
        components: component_registry.readonly(),
    }
}

pub fn test_registries_with_assets(asset_paths: Vec<PathBuf>) -> ReadOnlyRegistryContext {
    let render_context = test_render_context();
    let type_registry = build_type_registry();
    let component_registry = Ref::new(ComponentRegistry::new(&type_registry.read()));
    let asset_registry = AssetRegistry::new_test_with_assets(
        asset_paths,
        render_context,
        type_registry.clone(),
        component_registry.clone(),
    );
    ReadOnlyRegistryContext {
        assets: asset_registry.readonly(),
        types: type_registry.readonly(),
        components: component_registry.readonly(),
    }
}

pub fn test_scene() -> Scene {
    test_registries().scene()
}

pub fn test_game_context() -> GameContext {
    let render_context = test_render_context();
    let type_registry = build_type_registry();
    let component_registry = Ref::new(ComponentRegistry::new(&type_registry.read()));
    let asset_registry = AssetRegistry::new_test(
        PathBuf::from("."),
        render_context.clone(),
        type_registry.clone(),
        component_registry.clone(),
    );
    let assets = AssetContext {
        render_context,
        registries: RegistryContext {
            types: type_registry,
            components: component_registry,
            assets: asset_registry,
        },
    };
    GameContext::new(assets)
}
