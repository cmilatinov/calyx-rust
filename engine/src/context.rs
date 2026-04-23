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

#[derive(Clone)]
pub struct RegistryContext {
    pub assets: Ref<AssetRegistry>,
    pub types: Ref<TypeRegistry>,
    pub components: Ref<ComponentRegistry>,
}

impl RegistryContext {
    pub fn lock_read(&self) -> ReadOnlyRegistryContext {
        ReadOnlyRegistryContext {
            assets: self.assets.readonly(),
            types: self.types.readonly(),
            components: self.components.readonly(),
        }
    }
}

#[derive(Clone)]
pub struct ReadOnlyRegistryContext {
    pub assets: ReadOnlyRef<AssetRegistry>,
    pub types: ReadOnlyRef<TypeRegistry>,
    pub components: ReadOnlyRef<ComponentRegistry>,
}

impl ReadOnlyRegistryContext {
    pub fn scene(&self) -> Scene {
        Scene::new(self.clone())
    }
}

#[derive(Clone)]
pub struct AssetContext {
    pub render_context: Arc<RenderContext>,
    pub registries: RegistryContext,
}

impl AssetContext {
    pub fn new(
        cc: &eframe::CreationContext,
        project_path: impl Into<PathBuf>,
    ) -> Result<Self, BoxedError> {
        let render_context = Arc::new(RenderContext::from_eframe(cc));
        let mut type_registry = TypeRegistry::new();
        for f in inventory::iter::<ReflectRegistrationFn>() {
            (f.function)(&mut type_registry);
        }
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

    pub fn lock_read(&self) -> ReadOnlyAssetContext {
        ReadOnlyAssetContext {
            render_context: self.render_context.clone(),
            registries: self.registries.lock_read(),
        }
    }
}

#[derive(Clone)]
pub struct ReadOnlyAssetContext {
    pub render_context: Arc<RenderContext>,
    pub registries: ReadOnlyRegistryContext,
}

impl ReadOnlyAssetContext {
    pub fn scene(&self) -> Scene {
        Scene::new(self.registries.clone())
    }
}

pub struct GameContext {
    pub assets: AssetContext,
    pub scenes: SceneManager,
    pub resources: ResourceMap,
}

impl GameContext {
    pub fn new(assets: AssetContext) -> Self {
        Self {
            scenes: SceneManager::new(assets.registries.assets.readonly()),
            assets,
            resources: ResourceMap::new(),
        }
    }

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
