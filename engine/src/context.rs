use crate::assets::AssetRegistry;
use crate::class_registry::ComponentRegistry;
use crate::core::{ReadOnlyRef, Ref, Time};
use crate::error::BoxedError;
use crate::net::Network;
use crate::reflect::type_registry::TypeRegistry;
use crate::render::RenderContext;
use crate::resource::ResourceMap;
use crate::scene::{Scene, SceneManager};
use crate::ReflectRegistrationFn;
use std::path::PathBuf;
use std::sync::Arc;

/// Mutable registry bundle for assets, reflection types, and component
/// metadata.
#[derive(Clone)]
pub struct RegistryContext {
    /// Shared asset registry.
    pub assets: Ref<AssetRegistry>,
    /// Shared reflection type registry.
    pub types: Ref<TypeRegistry>,
    /// Shared component registry derived from the type registry.
    pub components: Ref<ComponentRegistry>,
}

impl RegistryContext {
    /// Converts this context into read-only wrapper handles.
    pub fn lock_read(&self) -> ReadOnlyRegistryContext {
        ReadOnlyRegistryContext {
            assets: self.assets.readonly(),
            types: self.types.readonly(),
            components: self.components.readonly(),
        }
    }
}

/// Read-only view over the core registries needed by scenes and assets.
#[derive(Clone)]
pub struct ReadOnlyRegistryContext {
    /// Read-only asset registry handle.
    pub assets: ReadOnlyRef<AssetRegistry>,
    /// Read-only reflection type registry handle.
    pub types: ReadOnlyRef<TypeRegistry>,
    /// Read-only component registry handle.
    pub components: ReadOnlyRef<ComponentRegistry>,
}

impl ReadOnlyRegistryContext {
    /// Creates an empty scene bound to these registries.
    pub fn scene(&self) -> Scene {
        Scene::new(self.clone())
    }
}

/// Asset-loading context containing render infrastructure and registries.
#[derive(Clone)]
pub struct AssetContext {
    /// Render context used while loading GPU-backed assets.
    pub render_context: Arc<RenderContext>,
    /// Registry bundle used to deserialize and register assets and components.
    pub registries: RegistryContext,
}

impl AssetContext {
    /// Builds a new asset context for a project rooted at `project_path`.
    pub fn new(
        cc: &eframe::CreationContext,
        project_path: impl Into<PathBuf>,
    ) -> Result<Self, BoxedError> {
        let project_path = project_path.into();
        log::info!("Creating asset context for {}", project_path.display());
        let render_context = Arc::new(RenderContext::from_eframe(cc));
        let mut type_registry = TypeRegistry::new();
        let mut registration_count = 0usize;
        for f in inventory::iter::<ReflectRegistrationFn>() {
            log::trace!(
                "Registering reflected types from {}::{} ({})",
                f.crate_name,
                f.module_path,
                f.name
            );
            (f.function)(&mut type_registry);
            registration_count += 1;
        }
        log::info!("Applied {registration_count} reflection registration callbacks");
        let type_registry = Ref::new(type_registry);
        let component_registry = Ref::new(ComponentRegistry::new(&type_registry.read()));
        let asset_registry = AssetRegistry::new(
            project_path,
            render_context.clone(),
            type_registry.clone(),
            component_registry.clone(),
        )?;
        Ok(Self {
            render_context,
            registries: RegistryContext {
                types: type_registry,
                components: component_registry,
                assets: asset_registry,
            },
        })
    }

    /// Converts this context into a read-only loading context.
    pub fn lock_read(&self) -> ReadOnlyAssetContext {
        ReadOnlyAssetContext {
            render_context: self.render_context.clone(),
            registries: self.registries.lock_read(),
        }
    }
}

/// Read-only asset-loading context passed into asset deserializers.
#[derive(Clone)]
pub struct ReadOnlyAssetContext {
    /// Shared render context for GPU-dependent asset loads.
    pub render_context: Arc<RenderContext>,
    /// Read-only registry bundle.
    pub registries: ReadOnlyRegistryContext,
}

impl ReadOnlyAssetContext {
    /// Creates an empty scene bound to this context's registries.
    pub fn scene(&self) -> Scene {
        Scene::new(self.registries.clone())
    }
}

/// Top-level runtime context used by the game loop.
pub struct GameContext {
    /// Asset-loading and registry context.
    pub assets: AssetContext,
    /// Loaded scenes and scene switching state.
    pub scenes: SceneManager,
    /// Mutable runtime resources.
    pub resources: ResourceMap,
}

impl GameContext {
    /// Creates a new game context with default runtime resources.
    pub fn new(assets: AssetContext) -> Self {
        Self {
            scenes: SceneManager::new(assets.registries.assets.readonly()),
            assets,
            resources: ResourceMap::new(),
        }
    }

    /// Advances time, networking, and the current scene's network sync.
    pub fn update(&mut self) {
        let Some((network, time)) = self.resources.resource_pair_mut::<Network, Time>() else {
            return;
        };
        time.update_time();
        network.update(time);
        let scene = self.scenes.current_scene_mut();
        network.update_scene(scene, &self.assets.lock_read());
    }
}
