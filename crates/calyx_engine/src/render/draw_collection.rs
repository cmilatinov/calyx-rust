//! The draws of a scene, collected once a frame and shared by every view
//! that renders the scene in that frame.

use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture;
use crate::assets::{AssetAccess, AssetId, ResolvedRefs};
use crate::component::{
    ComponentFoliage, ComponentMesh, ComponentSkinnedMesh, ComponentTerrain, ComponentWater,
};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::Transform;
use crate::render::asset_render_state::AssetRenderState;
use crate::render::foliage::FoliageCache;
use crate::render::static_batch::{BatchContext, Bounds, StaticBatches, BATCHED_OBJECT_ID};
use crate::render::terrain::TerrainCache;
use crate::render::water::{WaterCache, WaterLight};
use crate::render::{SceneRendererError, Shader};
use crate::scene::{GameObject, HierarchyVisibility, Scene, SceneId};
use legion::{Entity, EntityStore, IntoQuery};
use nalgebra_glm::Mat4;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// The shader whose materials merge into static batches.
const PBR_SHADER: &str = "shaders/mesh/pbr";

/// One draw-list entry emitted while collecting scene meshes.
pub(crate) struct DrawListElement {
    pub shader_id: AssetId,
    pub mat_id: AssetId,
    pub mesh_id: AssetId,
    pub bone_transform_index: i32,
    pub object_id: u32,
    pub transform: [[f32; 4]; 4],
    pub cast_shadows: bool,
}

/// The meshes, foliage, terrain, water and skinned meshes one scene draws, with the
/// assets they draw with, collected for one frame.
///
/// What a scene draws does not depend on the camera, so the views that render
/// the same scene in a frame, as the editor's Scene and Game views do, share
/// one collection: [`Self::collect`] it once the scene has stopped changing
/// for the frame, then hand it to each view's
/// [`SceneRenderer::render_scene_with`](crate::render::SceneRenderer::render_scene_with).
/// Each view keeps only what is its own: its sky, its gizmos and the
/// batching of these draws into its instance lists.
///
/// The collection keeps its caches (resolved asset references, foliage
/// scatter, terrain and water meshes) from one frame to the next, as a renderer
/// does, and keeps the scene's static mesh objects merged into
/// [`StaticBatches`], which it updates from the scene's changes: a frame
/// walks the batches and the few mesh objects drawn on their own, not every
/// mesh object of the scene. Object ids stay the same from frame to frame
/// while the scene does, since the batches' vertices carry them.
pub struct DrawCollection {
    asset_context: ReadOnlyAssetContext,
    white_texture: Ref<Texture>,
    /// The scene of the last [`Self::collect`].
    scene: Option<SceneId>,
    /// The meshes, materials, shaders and prefabs the scene's components
    /// resolved to, kept across frames while the registry holds still.
    resolved: ResolvedRefs,
    /// The shader each material drawn this frame resolved to.
    material_shaders: HashMap<AssetId, Option<Ref<Shader>>>,
    /// Which objects of the scene drawn this frame are visible.
    visibility: HierarchyVisibility,
    /// The scene's static mesh objects, merged, and the mesh objects drawn
    /// on their own.
    pub(crate) static_batches: StaticBatches,
    /// The PBR shader, whose materials batch.
    batched_shader: Option<AssetId>,
    pub(super) foliage: FoliageCache,
    terrain: TerrainCache,
    water: WaterCache,
    /// This frame's draws, sorted by shader, material, mesh and whether they
    /// cast shadows.
    pub(crate) draw_list: Vec<DrawListElement>,
    /// The world-space box around each static batch's merged mesh drawn
    /// this frame, by mesh, which each view culls the batch by.
    pub(crate) batch_bounds: HashMap<AssetId, Bounds>,
    /// The game object of each object id the draws carry, the first at id 1.
    pub(crate) object_ids: Vec<Uuid>,
    pub(crate) object_id_lookup: HashMap<Uuid, u32>,
    /// The meshes, materials, shaders and textures the draws use. It holds
    /// no skybox: the sky is each view's own.
    pub(crate) assets: AssetRenderState,
}

impl DrawCollection {
    /// An empty collection drawing with `context`'s assets. It fails when
    /// the default white texture, which materials fall back to, does not
    /// load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let white_texture = SceneRendererError::load::<Texture>(context, "textures/white")?;
        Ok(Self::with_white_texture(context.clone(), white_texture))
    }

    /// The PBR shader's id, or `None` when it does not load, in which case
    /// nothing batches.
    fn pbr_shader(context: &ReadOnlyAssetContext) -> Option<AssetId> {
        context
            .registries
            .assets
            .read()
            .load::<Shader>(PBR_SHADER)
            .ok()
            .map(|shader| shader.id())
    }

    pub(crate) fn with_white_texture(
        asset_context: ReadOnlyAssetContext,
        white_texture: Ref<Texture>,
    ) -> Self {
        let batched_shader = Self::pbr_shader(&asset_context);
        Self {
            asset_context,
            white_texture,
            scene: None,
            resolved: Default::default(),
            material_shaders: Default::default(),
            visibility: Default::default(),
            static_batches: Default::default(),
            batched_shader,
            foliage: Default::default(),
            terrain: Default::default(),
            water: Default::default(),
            draw_list: Default::default(),
            batch_bounds: Default::default(),
            object_ids: Default::default(),
            object_id_lookup: Default::default(),
            assets: Default::default(),
        }
    }

    /// The scene the draws were last collected from.
    pub fn scene(&self) -> Option<SceneId> {
        self.scene
    }

    /// Collects what `scene` draws now, replacing the previous collection.
    pub fn collect(&mut self, scene: &Scene) {
        if self.scene != Some(scene.id()) {
            self.object_ids.clear();
            self.object_id_lookup.clear();
        }
        self.scene = Some(scene.id());
        self.assets.clear();
        self.draw_list.clear();
        self.batch_bounds.clear();
        self.material_shaders.clear();
        self.visibility.clear();
        self.resolved.begin(&self.asset_context.registries);
        {
            crate::profile_scope!("Static batches");
            self.static_batches.update(
                scene,
                &mut BatchContext {
                    registries: &self.asset_context.registries,
                    render_context: &self.asset_context.render_context,
                    resolved: &mut self.resolved,
                    batched_shader: self.batched_shader,
                    object_ids: &mut self.object_ids,
                    object_id_lookup: &mut self.object_id_lookup,
                },
            );
            let batches: Vec<_> = self
                .static_batches
                .batches()
                .filter_map(|(key, batch)| {
                    Some((
                        batch.mesh.clone()?,
                        batch.material.clone(),
                        key.cast_shadows,
                        batch.bounds?,
                    ))
                })
                .collect();
            for (mesh, material, cast_shadows, bounds) in batches {
                self.batch_bounds.insert(mesh.id(), bounds);
                self.insert(
                    &mesh,
                    &material,
                    BATCHED_OBJECT_ID,
                    None,
                    Mat4::identity().into(),
                    cast_shadows,
                );
            }
        }
        {
            crate::profile_scope!("Mesh components");
            let world = scene.world();
            let individual = self.static_batches.individual().to_vec();
            for entity in individual {
                let Some(game_object) = scene.game_object_from_entity(entity) else {
                    continue;
                };
                let Ok(entry) = world.entry_ref(entity) else {
                    continue;
                };
                let Ok(c_mesh) = entry.get_component::<ComponentMesh>() else {
                    continue;
                };
                if !self.visibility.is_visible(scene, game_object)
                    || !scene.is_component_enabled::<ComponentMesh>(game_object)
                {
                    continue;
                }
                let registries = &self.asset_context.registries;
                let Some(mesh_ref) = self.resolved.resolve(registries, &c_mesh.mesh) else {
                    continue;
                };
                let Some(mat_ref) = self.resolved.resolve(registries, &c_mesh.material) else {
                    continue;
                };
                let transform = scene.world_transform(game_object);
                let cast_shadows = c_mesh.cast_shadows;
                let object_id = self.object_id(scene.uuid(game_object));
                self.insert(
                    &mesh_ref,
                    &mat_ref,
                    object_id,
                    None,
                    transform.matrix().into(),
                    cast_shadows,
                );
            }
        }
        self.collect_foliage(scene);
        self.collect_terrain(scene);
        self.collect_water(scene);
        self.collect_skinned_meshes(scene);
        {
            crate::profile_scope!("Material buffers");
            let AssetRenderState {
                materials,
                textures,
                ..
            } = &mut self.assets;
            let registries = &self.asset_context.registries;
            let resolved = &mut self.resolved;
            for (material_id, material) in materials.iter() {
                let mut material = material.write();
                let shader = self.material_shaders.get(material_id).cloned().flatten();
                material.follow_resolved_shader(&self.asset_context, shader.as_ref());
                material.load_buffers(&self.asset_context.render_context);
                material.collect_textures_with(
                    &self.asset_context,
                    textures,
                    self.white_texture.clone(),
                    |texture| resolved.resolve(registries, texture),
                );
            }
        }
        self.resolved.end();
        self.draw_list.sort_by_key(
            |DrawListElement {
                 shader_id,
                 mat_id,
                 mesh_id,
                 cast_shadows,
                 ..
             }| (*shader_id, *mat_id, *mesh_id, *cast_shadows),
        );
    }

    /// Drops every asset the collection holds, and the meshes and prefab
    /// parts of its terrain, water and foliage caches. The next collection resolves
    /// them again.
    pub fn release_assets(&mut self) {
        self.scene = None;
        self.assets.clear();
        self.draw_list.clear();
        self.batch_bounds.clear();
        self.object_ids.clear();
        self.object_id_lookup.clear();
        self.material_shaders.clear();
        self.resolved = ResolvedRefs::default();
        self.static_batches.clear();
        self.foliage = FoliageCache::default();
        self.terrain = TerrainCache::default();
        self.water = WaterCache::default();
    }

    fn collect_foliage(&mut self, scene: &Scene) {
        self.foliage.begin_frame(scene.id());
        let mut query = <(Entity, &ComponentFoliage)>::query();
        for (entity, foliage) in query.iter(scene.world()) {
            let Some(owner) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            if !self.visibility.is_visible(scene, owner)
                || !scene.is_component_enabled::<ComponentFoliage>(owner)
            {
                continue;
            }
            let uuid = scene.uuid(owner);
            let instances = self.foliage.instances(uuid, foliage);
            let parts_by_prefab: Vec<Vec<_>> = foliage
                .prefabs
                .iter()
                .map(|source| {
                    let registries = &self.asset_context.registries;
                    let Some(prefab) = self.resolved.resolve(registries, source) else {
                        return Vec::new();
                    };
                    let resolved = &mut self.resolved;
                    self.foliage
                        .parts(&prefab)
                        .iter()
                        .filter_map(|part| {
                            Some((
                                resolved.resolve(registries, &part.mesh)?,
                                resolved.resolve(registries, &part.material)?,
                                part.transform,
                                part.cast_shadows,
                            ))
                        })
                        .collect()
                })
                .collect();
            let world = scene.world_transform(owner).matrix();
            let object_id = self.object_id(uuid);
            for instance in instances.iter() {
                let placement = world * instance.transform;
                for (mesh, material, transform, cast_shadows) in &parts_by_prefab[instance.prefab] {
                    self.insert(
                        mesh,
                        material,
                        object_id,
                        None,
                        (placement * transform).into(),
                        foliage.cast_shadows && *cast_shadows,
                    );
                }
            }
        }
        self.foliage.end_frame();
    }

    /// Adds every visible, enabled terrain, through the ordinary mesh
    /// batches.
    fn collect_terrain(&mut self, scene: &Scene) {
        self.terrain.begin_frame(scene.id());
        let mut query = <(Entity, &ComponentTerrain)>::query();
        for (entity, terrain) in query.iter(scene.world()) {
            let Some(owner) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            if !self.visibility.is_visible(scene, owner)
                || !scene.is_component_enabled::<ComponentTerrain>(owner)
            {
                continue;
            }
            let registries = &self.asset_context.registries;
            let heightfield = self
                .resolved
                .resolve(registries, &terrain.heightmap)
                .and_then(|heightmap| heightmap.read().heightfield());
            let (Some(heightfield), Some(material)) = (
                heightfield,
                self.resolved.resolve(registries, &terrain.material),
            ) else {
                continue;
            };
            let uuid = scene.uuid(owner);
            let Some(mesh) = self.terrain.mesh(
                &self.asset_context.render_context,
                uuid,
                terrain,
                &heightfield,
            ) else {
                continue;
            };
            let transform = scene.world_transform(owner).matrix().into();
            let object_id = self.object_id(uuid);
            self.insert(
                &mesh,
                &material,
                object_id,
                None,
                transform,
                terrain.cast_shadows,
            );
        }
        self.terrain.end_frame();
    }

    /// Adds every visible, enabled water, through the ordinary mesh
    /// batches, with the material its cache fills from the component, and
    /// its banks with their material. Water casts no shadows; banks do.
    fn collect_water(&mut self, scene: &Scene) {
        self.water.begin_frame(&self.asset_context, scene);
        let mut light = None;
        let mut query = <(Entity, &ComponentWater)>::query();
        for (entity, water) in query.iter(scene.world()) {
            let Some(owner) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            if !self.visibility.is_visible(scene, owner)
                || !scene.is_component_enabled::<ComponentWater>(owner)
            {
                continue;
            }
            let light = light.get_or_insert_with(|| WaterLight::of_scene(scene));
            let uuid = scene.uuid(owner);
            let Some(draw) = self.water.draw(
                &self.asset_context,
                scene,
                owner,
                uuid,
                water,
                light.as_ref(),
            ) else {
                continue;
            };
            let transform: [[f32; 4]; 4] = scene.world_transform(owner).matrix().into();
            let object_id = self.object_id(uuid);
            self.insert(
                &draw.mesh,
                &draw.material,
                object_id,
                None,
                transform,
                false,
            );
            let registries = &self.asset_context.registries;
            if let (Some(banks), Some(material)) = (
                draw.banks,
                self.resolved.resolve(registries, &water.bank_material),
            ) {
                self.insert(&banks, &material, object_id, None, transform, true);
            }
        }
        self.water.end_frame();
    }

    /// Adds every visible, enabled skinned mesh, filling each mesh's bone
    /// palette with its instances' bone transforms.
    fn collect_skinned_meshes(&mut self, scene: &Scene) {
        let mut skinned_meshes: HashSet<Uuid> = Default::default();
        let mut rest_poses: HashMap<GameObject, HashMap<String, Mat4>> = HashMap::new();
        let no_bones = HashMap::new();
        let mut rest_palette = Vec::new();
        let mut query = <(Entity, &ComponentSkinnedMesh)>::query();
        for (entity, c_skinned_mesh) in query.iter(scene.world()) {
            let Some(game_object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            if !self.visibility.is_visible(scene, game_object)
                || !scene.is_component_enabled::<ComponentSkinnedMesh>(game_object)
            {
                continue;
            }
            let registries = &self.asset_context.registries;
            let Some(mesh_ref) = self.resolved.resolve(registries, &c_skinned_mesh.mesh) else {
                continue;
            };
            let Some(mat_ref) = self.resolved.resolve(registries, &c_skinned_mesh.material) else {
                continue;
            };
            let mesh_id = mesh_ref.id();
            let transform = skinned_mesh_world_transform(scene, game_object, c_skinned_mesh);
            let bone_transform_index;
            {
                let mut mesh = mesh_ref.write();
                if !skinned_meshes.contains(&mesh_id) {
                    mesh.bone_transforms.clear();
                }
                let num_bones = mesh.bones.len();
                let Some(index) = skinning_instance_index(mesh.bone_transforms.len(), num_bones)
                else {
                    crate::logging::warn_once(format!("boneless-skinned-mesh:{mesh_id}"), || {
                        format!("Skinned mesh {mesh_id} has no bones; skipping skinning upload")
                    });
                    continue;
                };
                skinned_meshes.insert(mesh_id);
                bone_transform_index = index;
                // Each instance takes exactly `num_bones` slots, so the next instance's index
                // holds. A palette the animator has not filled, as in edit mode, draws the rest
                // pose the bone objects hold instead of collapsing every vertex.
                if c_skinned_mesh.bone_transforms.len() >= num_bones {
                    mesh.bone_transforms
                        .extend_from_slice(&c_skinned_mesh.bone_transforms[..num_bones]);
                } else {
                    let bones = c_skinned_mesh
                        .root_bone
                        .game_object(scene)
                        .map(|root| {
                            &*rest_poses
                                .entry(root)
                                .or_insert_with(|| ComponentSkinnedMesh::bone_poses(scene, root))
                        })
                        .unwrap_or(&no_bones);
                    ComponentSkinnedMesh::skinning_palette(&mesh, bones, &mut rest_palette);
                    mesh.bone_transforms.extend_from_slice(&rest_palette);
                }
            }
            let object_id = self.object_id(scene.uuid(game_object));
            self.insert(
                &mesh_ref,
                &mat_ref,
                object_id,
                Some(bone_transform_index as i32),
                transform.matrix().into(),
                c_skinned_mesh.cast_shadows,
            );
        }
    }

    /// The object id of the game object `game_object_id`, which it keeps
    /// while the collection draws the same scene.
    fn object_id(&mut self, game_object_id: Uuid) -> u32 {
        register_object_id(
            &mut self.object_id_lookup,
            &mut self.object_ids,
            game_object_id,
        )
    }

    fn insert(
        &mut self,
        mesh_ref: &Ref<Mesh>,
        mat_ref: &Ref<Material>,
        object_id: u32,
        bone_transform_index: Option<i32>,
        transform: [[f32; 4]; 4],
        cast_shadows: bool,
    ) {
        let registries = &self.asset_context.registries;
        let resolved = &mut self.resolved;
        let Some(shader_ref) = self
            .material_shaders
            .entry(mat_ref.id())
            .or_insert_with(|| {
                let shader_id = mat_ref.read().shader.id();
                resolved.resolve_id(registries, shader_id)
            })
            .clone()
        else {
            return;
        };
        self.draw_list.push(DrawListElement {
            shader_id: shader_ref.id(),
            mat_id: mat_ref.id(),
            mesh_id: mesh_ref.id(),
            bone_transform_index: bone_transform_index.unwrap_or(-1),
            object_id,
            transform,
            cast_shadows,
        });
        self.assets
            .meshes
            .entry(mesh_ref.id())
            .or_insert_with(|| mesh_ref.clone());
        self.assets
            .materials
            .entry(mat_ref.id())
            .or_insert_with(|| mat_ref.clone());
        self.assets
            .shaders
            .entry(shader_ref.id())
            .or_insert(shader_ref);
    }
}

/// The object id of `game_object_id`, registering it as the next id when it
/// has none yet. Ids start at 1; 0 is no object.
pub(crate) fn register_object_id(
    object_id_lookup: &mut HashMap<Uuid, u32>,
    object_ids: &mut Vec<Uuid>,
    game_object_id: Uuid,
) -> u32 {
    if let Some(object_id) = object_id_lookup.get(&game_object_id) {
        return *object_id;
    }
    object_ids.push(game_object_id);
    let object_id = object_ids.len() as u32;
    object_id_lookup.insert(game_object_id, object_id);
    object_id
}

/// Instance index of the next skinned draw in a mesh's shared bone buffer, or `None` for a mesh without bones.
pub(crate) fn skinning_instance_index(
    uploaded_bone_transforms: usize,
    num_bones: usize,
) -> Option<usize> {
    uploaded_bone_transforms.checked_div(num_bones)
}

/// World transform that places a skinned mesh, as [`ComponentSkinnedMesh::world_transform`] says.
pub(crate) fn skinned_mesh_world_transform(
    scene: &Scene,
    game_object: GameObject,
    skinned_mesh: &ComponentSkinnedMesh,
) -> Transform {
    skinned_mesh.world_transform(scene, game_object)
}
