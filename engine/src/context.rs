use crate::assets::AssetRegistry;
use crate::background::Background;
use crate::class_registry::ComponentRegistry;
use crate::core::{ReadOnlyRef, Ref, Time};
use crate::error::BoxedError;
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::ReflectDefault;
use crate::render::RenderContext;
use crate::scene::{Scene, SceneManager};
use crate::ReflectRegistrationFn;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct AssetContext {
    pub render_context: Arc<RenderContext>,
    pub asset_registry: Ref<AssetRegistry>,
    pub type_registry: Ref<TypeRegistry>,
    pub component_registry: Ref<ComponentRegistry>,
}

impl AssetContext {
    pub fn new(
        cc: &eframe::CreationContext,
        project_path: impl Into<PathBuf>,
    ) -> Result<Self, BoxedError> {
        println!(
            "Loading engine: {:?}",
            std::any::TypeId::of::<ReflectDefault>()
        );
        let render_context = Arc::new(RenderContext::from_eframe(cc));
        let mut type_registry = TypeRegistry::new();
        for f in inventory::iter::<ReflectRegistrationFn>() {
            println!("{:?}", f.name);
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
            type_registry,
            component_registry,
            asset_registry,
        })
    }

    pub fn scene(&self) -> Scene {
        Scene::new(self.lock_read())
    }

    pub fn lock_read(&self) -> ReadOnlyAssetContext {
        ReadOnlyAssetContext {
            render_context: self.render_context.clone(),
            asset_registry: self.asset_registry.readonly(),
            type_registry: self.type_registry.readonly(),
            component_registry: self.component_registry.readonly(),
        }
    }
}

#[derive(Clone)]
pub struct ReadOnlyAssetContext {
    pub render_context: Arc<RenderContext>,
    pub asset_registry: ReadOnlyRef<AssetRegistry>,
    pub type_registry: ReadOnlyRef<TypeRegistry>,
    pub component_registry: ReadOnlyRef<ComponentRegistry>,
}

impl ReadOnlyAssetContext {
    pub fn scene(&self) -> Scene {
        Scene::new(self.clone())
    }
}

pub struct GameContext {
    pub assets: AssetContext,
    pub scenes: SceneManager,
    pub time: Time,
    pub background: Background,
}

impl GameContext {
    pub fn new(assets: AssetContext) -> Self {
        Self {
            scenes: SceneManager::new(assets.asset_registry.readonly()),
            assets,
            time: Default::default(),
            background: Default::default(),
        }
    }
}
