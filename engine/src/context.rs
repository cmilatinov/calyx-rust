use crate::assets::AssetRegistry;
use crate::class_registry::ComponentRegistry;
use crate::core::{ReadOnlyRef, Ref, Time};
use crate::error::BoxedError;
use crate::input::Input;
use crate::net::Network;
use crate::reflect::type_registry::TypeRegistry;
use crate::render::{ParticleSimulation, RenderContext};
use crate::resource::ResourceMap;
use crate::scene::{Scene, SceneManager};
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
        let type_registry = Ref::new(TypeRegistry::new());
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
    /// GPU particle simulation of the scene that updates, which every view of
    /// that scene draws from.
    pub particles: ParticleSimulation,
}

/// Which scene of a [`GameContext`] a frame updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatedScene {
    /// The simulation copy, while simulation runs: the editor during play.
    Simulation,
    /// The current scene: the player.
    Current,
}

impl GameContext {
    /// Creates a new game context with default runtime resources.
    pub fn new(assets: AssetContext) -> Self {
        let mut resources = ResourceMap::new();
        if let Err(error) = resources
            .input_mut()
            .load_project_action_map(&assets.registries.assets.read())
        {
            log::error!("Failed to load the project action map, binding no actions: {error}");
        }
        Self {
            scenes: SceneManager::new(assets.registries.assets.readonly()),
            assets,
            resources,
            particles: ParticleSimulation::new(),
        }
    }

    /// Runs one frame of `target`: [`update_scene`] on that scene with this
    /// context's resources and particle simulation. Does nothing for
    /// [`UpdatedScene::Simulation`] while simulation is paused or stopped.
    pub fn update_scene(&mut self, target: UpdatedScene, input: &Input) {
        let assets = self.assets.lock_read();
        let scene = match target {
            UpdatedScene::Simulation => self.scenes.running_simulation_scene_mut(),
            UpdatedScene::Current => Some(self.scenes.current_scene_mut()),
        };
        if let Some(scene) = scene {
            update_scene(
                scene,
                &assets.registries,
                &mut self.resources,
                input,
                &mut self.particles,
                &assets.render_context,
            );
        }
    }

    /// Prepares `target` for this frame, whether or not it advances:
    /// [`Scene::prepare`], then [`ParticleSimulation::prepare`], which
    /// uploads the curves and gradients edited since the last frame, so a
    /// paused simulation draws them.
    ///
    /// [`UpdatedScene::Simulation`] prepares the simulation copy whenever one
    /// exists, paused included, and the current scene otherwise, as the
    /// editor shows it.
    pub fn prepare_scene(&mut self, target: UpdatedScene) {
        let assets = self.assets.lock_read();
        let scene = match target {
            UpdatedScene::Simulation => self.scenes.simulation_scene_mut(),
            UpdatedScene::Current => self.scenes.current_scene_mut(),
        };
        scene.prepare();
        self.particles.prepare(scene, &assets.render_context);
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

/// Runs one frame of `scene`: its physics and component updates, then its
/// particles on the GPU, by the resources' delta time. The particle
/// simulation submits its work before any view draws the frame.
pub fn update_scene(
    scene: &mut Scene,
    registries: &ReadOnlyRegistryContext,
    resources: &mut ResourceMap,
    input: &Input,
    particles: &mut ParticleSimulation,
    render_context: &RenderContext,
) {
    scene.update(registries, resources, input);
    particles.simulate(scene, render_context, resources.time().delta_time());
}
