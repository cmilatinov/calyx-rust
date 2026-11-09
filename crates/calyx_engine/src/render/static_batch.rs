//! Static scenery merged into a few meshes, kept from frame to frame and
//! updated from the scene's changes.
//!
//! A scene such as the tank arena draws over a thousand mesh objects that
//! never move. Drawing each as its own instance makes every frame pay for
//! each one: collecting it, batching its instance and binding its mesh in
//! every pass. [`StaticBatches`] merges the static ones that share a
//! material, per spatial chunk, into one mesh each, in world space, so a
//! frame draws a few dozen batches instead, and follows the scene's
//! [`ChangeJournal`](crate::scene) to keep the batches current: an object
//! that moves, is edited, hidden, disabled or deleted leaves its batch,
//! which is merged again without it, and is drawn on its own from then on.

use std::collections::{BTreeMap, HashMap, HashSet};

use legion::storage::ComponentTypeId;
use legion::{Entity, IntoQuery};
use nalgebra_glm::{vec2, vec4, Mat4, Vec2, Vec3};
use rapier3d::dynamics::RigidBodyType;
use uuid::Uuid;

use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::{AssetAccess, AssetId, ResolvedRefs};
use crate::component::{ComponentID, ComponentMesh, ComponentRigidBody, ComponentTransform};
use crate::context::ReadOnlyRegistryContext;
use crate::core::Ref;
use crate::render::RenderContext;
use crate::scene::{GameObject, HierarchyVisibility, Scene, SceneId};

/// The object id a batch's one instance carries: its vertices carry each
/// merged object's own id instead, which the object id pass reads.
pub(crate) const BATCHED_OBJECT_ID: u32 = u32::MAX;

/// The side, in metres, of the square chunks of the XZ plane that static
/// objects batch by, so one batch covers one area of the scene. Chunk
/// `[0, 0]` is centred on the origin, so a small scene around it, as most
/// are, falls in one.
pub(crate) const CHUNK_SIZE: f32 = 32.0;

/// A world-space axis-aligned box, its least and greatest corners.
pub(crate) type Bounds = [Vec3; 2];

/// Whether the box `bounds` lies wholly outside the view of `clip_from_world`,
/// a camera's projection times its view: past one of the four side planes of
/// its frustum. A box that may show is never outside, so culling by it never
/// drops anything the view would draw.
pub(crate) fn outside_view(clip_from_world: &Mat4, [least, greatest]: &Bounds) -> bool {
    let corners = (0..8).map(|corner: u32| {
        let pick = |bit: u32, axis: usize| {
            if corner & (1 << bit) == 0 {
                least[axis]
            } else {
                greatest[axis]
            }
        };
        clip_from_world * vec4(pick(0, 0), pick(1, 1), pick(2, 2), 1.0)
    });
    let mut outside = [true; 4];
    for clip in corners {
        outside[0] &= clip.x < -clip.w;
        outside[1] &= clip.x > clip.w;
        outside[2] &= clip.y < -clip.w;
        outside[3] &= clip.y > clip.w;
    }
    outside.contains(&true)
}

/// What one static batch merges: the objects of one chunk that draw with one
/// material and all cast shadows or all do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct BatchKey {
    pub material: AssetId,
    pub cast_shadows: bool,
    pub chunk: [i32; 2],
}

/// The chunk of the XZ plane `transform`'s origin lies in.
pub(crate) fn chunk_of(transform: &[[f32; 4]; 4]) -> [i32; 2] {
    let [x, _, z, _] = transform[3];
    [x, z].map(|along| (along / CHUNK_SIZE + 0.5).floor() as i32)
}

/// Everything about a mesh object that decides how it draws.
#[derive(Clone, Debug, PartialEq)]
struct DrawInputs {
    mesh: AssetId,
    material: AssetId,
    cast_shadows: bool,
    transform: [[f32; 4]; 4],
}

/// Identifies the geometry a mesh holds: a reload writes new geometry into
/// the same mesh, in new allocations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GeometrySignature {
    vertices: (usize, usize),
    indices: (usize, usize),
}

impl GeometrySignature {
    fn of(mesh: &Mesh) -> Self {
        Self {
            vertices: (mesh.vertices.as_ptr() as usize, mesh.vertices.len()),
            indices: (mesh.indices.as_ptr() as usize, mesh.indices.len()),
        }
    }
}

/// An object merged into a batch.
struct Member {
    key: BatchKey,
    inputs: DrawInputs,
    mesh: Ref<Mesh>,
    geometry: GeometrySignature,
    object_id: u32,
}

/// One merged mesh and the objects it merges.
pub(crate) struct Batch {
    pub material: Ref<Material>,
    /// The merged mesh, in world space, made again whenever a member leaves
    /// or its geometry changes.
    pub mesh: Option<Ref<Mesh>>,
    /// The world-space box around the merged mesh, which a view culls the
    /// batch by.
    pub bounds: Option<Bounds>,
    /// The members, in the order they merge.
    members: Vec<Entity>,
    dirty: bool,
}

/// What [`StaticBatches::update`] reads and resolves through.
pub(crate) struct BatchContext<'a> {
    pub registries: &'a ReadOnlyRegistryContext,
    pub render_context: &'a RenderContext,
    pub resolved: &'a mut ResolvedRefs,
    /// The only shader whose materials batch: the PBR shader, which the
    /// G-buffer pass draws opaque, so merging its draws changes no blending
    /// order.
    pub batched_shader: Option<AssetId>,
    pub object_ids: &'a mut Vec<Uuid>,
    pub object_id_lookup: &'a mut HashMap<Uuid, u32>,
}

/// A scene's static mesh objects merged into batches, and the mesh objects
/// drawn one by one: those that moved, or changed in any way that changes
/// how they draw, since the batches were made, the ones made since, and the
/// ones that cannot batch.
///
/// An object batches when it is visible and its mesh is enabled, its
/// material uses the PBR shader, its mesh has no bones and no fourth UV
/// channel, which batches carry object ids in, and neither it nor an
/// ancestor has a rigid body other than a fixed one. Moving objects, such as
/// tanks, therefore draw on their own from the start. An object that leaves
/// a batch never joins one again until the batches are made again, which
/// happens for another scene, or when the scene's journal no longer holds
/// every change since the last update.
#[derive(Default)]
pub(crate) struct StaticBatches {
    scene: Option<SceneId>,
    cursor: u64,
    epoch: Option<u64>,
    members: HashMap<Entity, Member>,
    individual: Vec<Entity>,
    individual_set: HashSet<Entity>,
    batches: BTreeMap<BatchKey, Batch>,
    /// How many batches were merged since the collection was made.
    merges: usize,
}

impl StaticBatches {
    /// Brings the batches up to date with `scene`, making them from scratch
    /// for a scene other than the last one. Returns `true` when it made them
    /// from scratch.
    pub fn update(&mut self, scene: &Scene, context: &mut BatchContext) -> bool {
        let epoch = context.registries.assets.read().change_epoch();
        let journal = scene.change_journal();
        let changed = (self.scene == Some(scene.id()))
            .then(|| journal.since(self.cursor))
            .flatten()
            .map(|changes| self.touched(scene, changes));
        let rebuilt = changed.is_none();
        match changed {
            None => self.rebuild(scene, context),
            Some(touched) => {
                for entity in touched {
                    self.refresh(scene, entity, context);
                }
                if self.epoch != Some(epoch) {
                    self.revalidate(context);
                }
            }
        }
        self.scene = Some(scene.id());
        self.cursor = journal.end();
        self.epoch = Some(epoch);
        self.merge_dirty(context.render_context);
        if rebuilt {
            log::debug!(
                "Merged {} static mesh objects into {} batches; {} mesh objects draw on their own",
                self.members.len(),
                self.batches.len(),
                self.individual.len()
            );
        }
        rebuilt
    }

    /// The batches, by key.
    pub fn batches(&self) -> impl Iterator<Item = (&BatchKey, &Batch)> {
        self.batches.iter()
    }

    /// The mesh objects drawn one by one, in the order they joined.
    pub fn individual(&self) -> &[Entity] {
        &self.individual
    }

    /// Whether `entity` is merged into a batch.
    #[cfg(test)]
    pub fn is_batched(&self, entity: Entity) -> bool {
        self.members.contains_key(&entity)
    }

    /// The key of the batch `entity` is merged into.
    #[cfg(test)]
    pub fn batch_of(&self, entity: Entity) -> Option<BatchKey> {
        self.members.get(&entity).map(|member| member.key)
    }

    /// How many batches were merged since the collection was made.
    #[cfg(test)]
    pub fn merges(&self) -> usize {
        self.merges
    }

    /// Forgets everything, so the next update makes the batches from scratch.
    pub fn clear(&mut self) {
        *self = Self {
            merges: self.merges,
            ..Default::default()
        };
    }

    /// The objects whose drawing the journal's `changes` may change: an
    /// object whose mesh changed, and every object under one whose
    /// transform or id, which holds its visibility, changed or which left
    /// the scene.
    fn touched(
        &self,
        scene: &Scene,
        changes: impl Iterator<Item = (Entity, ComponentTypeId)>,
    ) -> Vec<Entity> {
        let transform = ComponentTypeId::of::<ComponentTransform>();
        let id = ComponentTypeId::of::<ComponentID>();
        let mesh = ComponentTypeId::of::<ComponentMesh>();
        let mut seen = HashSet::new();
        let mut touched = Vec::new();
        let mut touch = |entity: Entity| {
            if seen.insert(entity) {
                touched.push(entity);
            }
        };
        for (entity, component) in changes {
            if component == mesh {
                touch(entity);
            } else if component == transform || component == id {
                match scene.game_object_from_entity(entity) {
                    Some(game_object) => {
                        for object in scene.descendants(game_object) {
                            touch(object.entity);
                        }
                    }
                    None => touch(entity),
                }
            }
        }
        touched
    }

    /// Takes `entity` out of its batch when it no longer draws as it was
    /// merged, and draws it on its own from then on if it still has a mesh.
    fn refresh(&mut self, scene: &Scene, entity: Entity, context: &mut BatchContext) {
        let game_object = scene.game_object_from_entity(entity);
        if let Some(member) = self.members.get(&entity) {
            let inputs = game_object.and_then(|game_object| {
                let mut visibility = HierarchyVisibility::default();
                draw_inputs(scene, game_object, &mut visibility, context)
                    .map(|(inputs, _, _)| inputs)
            });
            if inputs.as_ref() == Some(&member.inputs) {
                return;
            }
            self.remove_member(entity);
        }
        let has_mesh = game_object.is_some_and(|game_object| {
            scene
                .read_component::<ComponentMesh, _, _>(game_object, |_| ())
                .is_some()
        });
        if has_mesh {
            if self.individual_set.insert(entity) {
                self.individual.push(entity);
            }
        } else if self.individual_set.remove(&entity) {
            self.individual.retain(|other| *other != entity);
        }
    }

    /// After the registry changed: checks that every member's mesh and every
    /// batch's material still resolve to what was merged. A member whose
    /// asset no longer resolves draws on its own; a batch whose member's
    /// geometry was reloaded merges again.
    fn revalidate(&mut self, context: &mut BatchContext) {
        let registries = context.registries;
        let mut lost = Vec::new();
        for (entity, member) in &mut self.members {
            let mesh = context
                .resolved
                .resolve_id::<Mesh>(registries, member.inputs.mesh);
            let material = context
                .resolved
                .resolve_id::<Material>(registries, member.inputs.material);
            let batch = self
                .batches
                .get_mut(&member.key)
                .expect("a member's batch exists");
            let (Some(mesh), Some(material)) = (mesh, material) else {
                lost.push(*entity);
                continue;
            };
            if material.ptr_id() != batch.material.ptr_id() {
                lost.push(*entity);
                continue;
            }
            let geometry = GeometrySignature::of(&mesh.read());
            if mesh.ptr_id() != member.mesh.ptr_id() || geometry != member.geometry {
                member.mesh = mesh;
                member.geometry = geometry;
                batch.dirty = true;
            }
        }
        for entity in lost {
            self.remove_member(entity);
            if self.individual_set.insert(entity) {
                self.individual.push(entity);
            }
        }
    }

    /// Makes the batches from scratch from every mesh object of `scene`.
    fn rebuild(&mut self, scene: &Scene, context: &mut BatchContext) {
        self.clear();
        let mut visibility = HierarchyVisibility::default();
        let mut query = <(Entity, &ComponentMesh)>::query();
        let entities: Vec<Entity> = query
            .iter(scene.world())
            .map(|(entity, _)| *entity)
            .collect();
        for entity in entities {
            let Some(game_object) = scene.game_object_from_entity(entity) else {
                continue;
            };
            match batchable(scene, game_object, &mut visibility, context) {
                Some((inputs, mesh, material)) => {
                    self.add_member(scene, game_object, inputs, mesh, material, context)
                }
                None => {
                    if self.individual_set.insert(entity) {
                        self.individual.push(entity);
                    }
                }
            }
        }
    }

    fn add_member(
        &mut self,
        scene: &Scene,
        game_object: GameObject,
        inputs: DrawInputs,
        mesh: Ref<Mesh>,
        material: Ref<Material>,
        context: &mut BatchContext,
    ) {
        let key = BatchKey {
            material: inputs.material,
            cast_shadows: inputs.cast_shadows,
            chunk: chunk_of(&inputs.transform),
        };
        let object_id = super::draw_collection::register_object_id(
            context.object_id_lookup,
            context.object_ids,
            scene.uuid(game_object),
        );
        let geometry = GeometrySignature::of(&mesh.read());
        let batch = self.batches.entry(key).or_insert_with(|| Batch {
            material,
            mesh: None,
            bounds: None,
            members: Vec::new(),
            dirty: true,
        });
        batch.members.push(game_object.entity);
        batch.dirty = true;
        self.members.insert(
            game_object.entity,
            Member {
                key,
                inputs,
                mesh,
                geometry,
                object_id,
            },
        );
    }

    fn remove_member(&mut self, entity: Entity) {
        let Some(member) = self.members.remove(&entity) else {
            return;
        };
        if let Some(batch) = self.batches.get_mut(&member.key) {
            batch.members.retain(|other| *other != entity);
            batch.dirty = true;
        }
    }

    /// Merges every batch whose members changed again, and drops the ones
    /// left empty.
    fn merge_dirty(&mut self, render_context: &RenderContext) {
        self.batches.retain(|_, batch| !batch.members.is_empty());
        for batch in self.batches.values_mut().filter(|batch| batch.dirty) {
            let mut merged = Mesh::new(render_context);
            for entity in &batch.members {
                let member = &self.members[entity];
                append_transformed(
                    &mut merged,
                    &member.mesh.read(),
                    &Mat4::from(member.inputs.transform),
                    member.object_id,
                );
            }
            merged.mark_dirty();
            batch.bounds = merged.vertices.iter().fold(None, |bounds, vertex| {
                let [least, greatest] = bounds.unwrap_or([*vertex, *vertex]);
                Some([least.inf(vertex), greatest.sup(vertex)])
            });
            // The draw list batches and looks meshes up by id, so each
            // merged mesh gets its own.
            batch.mesh = Some(Ref::from_id_value(Uuid::new_v4(), merged));
            batch.dirty = false;
            self.merges += 1;
        }
    }
}

/// How `game_object` draws now, with its mesh and material, or `None` when
/// it draws nothing: it is hidden, its mesh is disabled or missing, or an
/// asset does not resolve.
fn draw_inputs(
    scene: &Scene,
    game_object: GameObject,
    visibility: &mut HierarchyVisibility,
    context: &mut BatchContext,
) -> Option<(DrawInputs, Ref<Mesh>, Ref<Material>)> {
    if !visibility.is_visible(scene, game_object)
        || !scene.is_component_enabled::<ComponentMesh>(game_object)
    {
        return None;
    }
    let (mesh, material, cast_shadows) = scene
        .read_component::<ComponentMesh, _, _>(game_object, |c_mesh| {
            (c_mesh.mesh.id(), c_mesh.material.id(), c_mesh.cast_shadows)
        })?;
    let registries = context.registries;
    let mesh_ref = context.resolved.resolve_id::<Mesh>(registries, mesh)?;
    let material_ref = context
        .resolved
        .resolve_id::<Material>(registries, material)?;
    let inputs = DrawInputs {
        mesh,
        material,
        cast_shadows,
        transform: scene.world_transform(game_object).matrix().into(),
    };
    Some((inputs, mesh_ref, material_ref))
}

/// How `game_object` draws, when it can merge into a static batch.
fn batchable(
    scene: &Scene,
    game_object: GameObject,
    visibility: &mut HierarchyVisibility,
    context: &mut BatchContext,
) -> Option<(DrawInputs, Ref<Mesh>, Ref<Material>)> {
    let batched_shader = context.batched_shader?;
    let moves = |object: GameObject| {
        scene
            .read_component::<ComponentRigidBody, _, _>(object, |body| {
                body.ty != RigidBodyType::Fixed
            })
            .unwrap_or(false)
    };
    if moves(game_object) || scene.ancestors(game_object).any(moves) {
        return None;
    }
    let (inputs, mesh, material) = draw_inputs(scene, game_object, visibility, context)?;
    if material.read().shader.id() != batched_shader {
        return None;
    }
    {
        let mesh = mesh.read();
        if !mesh.bones.is_empty()
            || mesh.indices.is_empty()
            || mesh.uvs[3].iter().any(|uv| *uv != Vec2::zeros())
        {
            return None;
        }
    }
    Some((inputs, mesh, material))
}

/// Appends `source`'s geometry to `merged`, placed by `transform` as the
/// mesh shaders place an instance: positions by the whole transform, normals
/// and tangents by it as directions, not normalized, so the merged vertices
/// shade as the instance did. Each vertex's fourth UV channel carries
/// `object_id`.
fn append_transformed(merged: &mut Mesh, source: &Mesh, transform: &Mat4, object_id: u32) {
    let base = merged.vertices.len() as u32;
    let object_uv = vec2(object_id as f32, 0.0);
    for (index, position) in source.vertices.iter().enumerate() {
        let world = transform * vec4(position.x, position.y, position.z, 1.0);
        merged.vertices.push(world.xyz());
        let normal = source.normals.get(index).copied().unwrap_or(Vec3::zeros());
        merged
            .normals
            .push((transform * vec4(normal.x, normal.y, normal.z, 0.0)).xyz());
        let tangent = source
            .tangents
            .get(index)
            .copied()
            .unwrap_or(Mesh::DEFAULT_TANGENT);
        let direction = transform * vec4(tangent.x, tangent.y, tangent.z, 0.0);
        merged
            .tangents
            .push(vec4(direction.x, direction.y, direction.z, tangent.w));
        for channel in 0..3 {
            let uv = source.uvs[channel]
                .get(index)
                .copied()
                .unwrap_or(Vec2::zeros());
            merged.uvs[channel].push(uv);
        }
        merged.uvs[3].push(object_uv);
    }
    merged
        .indices
        .extend(source.indices.iter().map(|index| index + base));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetRef;
    use crate::context::ReadOnlyAssetContext;
    use crate::render::water::WATER_SHADER;
    use crate::render::{DrawCollection, Shader};
    use crate::scene::ChangeJournal;
    use crate::test_support::{SceneBuilder, TestAssets};
    use nalgebra_glm::{self as glm, vec3};

    fn material(context: &ReadOnlyAssetContext, name: &str, shader: &str) -> Ref<Material> {
        let registry = context.registries.assets.read();
        let shader = registry.load::<Shader>(shader).expect("shader loads");
        registry
            .create_or_update(name.into(), Material::from_shader(context, shader))
            .expect("material should be created")
    }

    fn spawn_mesh(
        builder: &mut SceneBuilder,
        parent: Option<GameObject>,
        mesh: &Ref<Mesh>,
        material: &Ref<Material>,
        position: Vec3,
    ) -> GameObject {
        let game_object = match parent {
            Some(parent) => builder.spawn_child(parent, "mesh"),
            None => builder.spawn("mesh"),
        };
        builder.add_component(
            game_object,
            ComponentMesh {
                mesh: AssetRef::from_id(mesh.id()),
                material: AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        builder.set_transform(game_object, glm::translation(&position));
        game_object
    }

    fn batch_mesh(draws: &DrawCollection, key: BatchKey) -> Ref<Mesh> {
        draws
            .static_batches
            .batches()
            .find(|(other, _)| **other == key)
            .and_then(|(_, batch)| batch.mesh.clone())
            .expect("the batch is merged")
    }

    #[test]
    fn static_pbr_meshes_merge_per_material_and_chunk_and_the_others_draw_alone() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let cube = context.registries.assets.read().cube().expect("cube");
        let first = material(&context, "test/static_first", "shaders/mesh/pbr");
        let second = material(&context, "test/static_second", "shaders/mesh/pbr");
        let water = material(&context, "test/static_water", WATER_SHADER);
        let mut builder = SceneBuilder::new(context.registries.clone());
        let a = spawn_mesh(&mut builder, None, &cube, &first, vec3(0.0, 0.0, 0.0));
        let b = spawn_mesh(&mut builder, None, &cube, &first, vec3(2.0, 0.0, 0.0));
        let far = spawn_mesh(&mut builder, None, &cube, &first, vec3(40.0, 0.0, 0.0));
        let other = spawn_mesh(&mut builder, None, &cube, &second, vec3(4.0, 0.0, 0.0));
        let unlit = spawn_mesh(&mut builder, None, &cube, &water, vec3(6.0, 0.0, 0.0));
        let fixed = builder.spawn("fixed");
        builder.add_component(
            fixed,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        let on_fixed = spawn_mesh(&mut builder, Some(fixed), &cube, &first, Vec3::zeros());
        let tank = builder.spawn("tank");
        builder.add_component(tank, ComponentRigidBody::default());
        let on_tank = spawn_mesh(&mut builder, Some(tank), &cube, &first, Vec3::zeros());
        let hidden = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
        let _ = builder
            .scene_mut()
            .write_component::<ComponentID, _>(hidden, |id| id.visible = false);
        let scene = builder.finish();

        let mut draws = DrawCollection::new(&context).expect("white texture loads");
        draws.collect(&scene);
        let batches = &draws.static_batches;
        for batched in [a, b, far, other, on_fixed] {
            assert!(batches.is_batched(batched.entity));
        }
        for alone in [unlit, on_tank, hidden] {
            assert!(!batches.is_batched(alone.entity));
            assert!(batches.individual().contains(&alone.entity));
        }
        let key = batches.batch_of(a.entity).unwrap();
        assert_eq!(batches.batch_of(b.entity), Some(key));
        assert_eq!(batches.batch_of(on_fixed.entity), Some(key));
        assert_ne!(batches.batch_of(far.entity), Some(key), "another chunk");
        assert_ne!(
            batches.batch_of(other.entity),
            Some(key),
            "another material"
        );
        assert_eq!(batches.batches().count(), 3);

        // The merged mesh holds each member's cube in world space, each
        // vertex carrying its object's id, and draws as one instance.
        let merged = batch_mesh(&draws, key);
        let merged = merged.read();
        let cube = cube.read();
        let count = cube.vertices.len();
        assert_eq!(merged.vertices.len(), 3 * count);
        assert_eq!(merged.indices.len(), 3 * cube.indices.len());
        assert_eq!(
            merged.vertices[count],
            cube.vertices[0] + vec3(2.0, 0.0, 0.0)
        );
        assert_eq!(
            merged.indices[cube.indices.len()],
            cube.indices[0] + count as u32
        );
        let id_of = |object: GameObject| draws.object_id_lookup[&scene.uuid(object)] as f32;
        assert!(merged.uvs[3][..count].iter().all(|uv| uv.x == id_of(a)));
        assert!(merged.uvs[3][count..2 * count]
            .iter()
            .all(|uv| uv.x == id_of(b)));
        let batch_draws = draws
            .draw_list
            .iter()
            .filter(|draw| draw.object_id == BATCHED_OBJECT_ID)
            .count();
        assert_eq!(batch_draws, 3);
        assert_eq!(
            draws.draw_list.len(),
            3 + 2,
            "the batches, the water and the tank"
        );
    }

    #[test]
    fn a_box_is_outside_the_view_only_when_wholly_past_one_side_of_the_frustum() {
        let camera = crate::render::Camera::new(1.0, 90f32.to_radians(), 0.1, 100.0);
        // The camera looks down positive z from the origin.
        let clip_from_world = camera.projection;
        let ahead = [vec3(-1.0, -1.0, 4.0), vec3(1.0, 1.0, 6.0)];
        let straddling = [vec3(-20.0, -1.0, 4.0), vec3(-4.0, 1.0, 6.0)];
        let left = [vec3(-20.0, -1.0, 4.0), vec3(-8.0, 1.0, 6.0)];
        let above = [vec3(-1.0, 8.0, 4.0), vec3(1.0, 9.0, 6.0)];
        let around = [vec3(-1.0, -1.0, -1.0), vec3(1.0, 1.0, 1.0)];
        assert!(!outside_view(&clip_from_world, &ahead));
        assert!(!outside_view(&clip_from_world, &straddling));
        assert!(!outside_view(&clip_from_world, &around));
        assert!(outside_view(&clip_from_world, &left));
        assert!(outside_view(&clip_from_world, &above));
    }

    #[test]
    fn moving_or_deleting_a_batched_object_merges_its_batch_again_without_it() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let cube = context.registries.assets.read().cube().expect("cube");
        let first = material(&context, "test/static_moving", "shaders/mesh/pbr");
        let vertices = cube.read().vertices.len();
        let mut builder = SceneBuilder::new(context.registries.clone());
        let a = spawn_mesh(&mut builder, None, &cube, &first, vec3(0.0, 0.0, 0.0));
        let b = spawn_mesh(&mut builder, None, &cube, &first, vec3(2.0, 0.0, 0.0));
        let parent = builder.spawn("parent");
        let c = spawn_mesh(
            &mut builder,
            Some(parent),
            &cube,
            &first,
            vec3(4.0, 0.0, 0.0),
        );
        let mut scene = builder.finish();
        let mut draws = DrawCollection::new(&context).expect("white texture loads");
        draws.collect(&scene);
        let key = draws.static_batches.batch_of(a.entity).unwrap();
        let merges = draws.static_batches.merges();
        let merged = batch_mesh(&draws, key);
        assert_eq!(merged.read().vertices.len(), 3 * vertices);

        // Frames that change nothing, or write a transform the object
        // already had, keep the merged mesh and the draw list.
        let draw_list = draws.draw_list.len();
        draws.collect(&scene);
        scene.set_transform(a, &glm::translation(&vec3(0.0, 0.0, 0.0)));
        draws.collect(&scene);
        assert_eq!(draws.static_batches.merges(), merges);
        assert_eq!(batch_mesh(&draws, key).id(), merged.id());
        assert!(draws.static_batches.is_batched(a.entity));
        assert_eq!(draws.draw_list.len(), draw_list);

        // A moved object leaves its batch, which merges again without it,
        // and draws on its own where it moved to, under the same object id.
        let a_id = draws.object_id_lookup[&scene.uuid(a)];
        scene.set_transform(a, &glm::translation(&vec3(1.0, 5.0, 0.0)));
        draws.collect(&scene);
        assert!(!draws.static_batches.is_batched(a.entity));
        assert!(draws.static_batches.individual().contains(&a.entity));
        assert_eq!(draws.static_batches.merges(), merges + 1);
        assert_eq!(batch_mesh(&draws, key).read().vertices.len(), 2 * vertices);
        assert_eq!(draws.object_id_lookup[&scene.uuid(a)], a_id);
        let moved = draws
            .draw_list
            .iter()
            .find(|draw| draw.object_id == a_id)
            .expect("the moved object draws on its own");
        assert_eq!(moved.transform[3], [1.0, 5.0, 0.0, 1.0]);

        // Moving a parent moves its children out of their batches.
        scene.set_transform(parent, &glm::translation(&vec3(0.0, 1.0, 0.0)));
        draws.collect(&scene);
        assert!(!draws.static_batches.is_batched(c.entity));
        assert_eq!(batch_mesh(&draws, key).read().vertices.len(), vertices);

        // Deleting the last member drops the batch.
        scene.delete(b);
        scene.flush_deletes();
        draws.collect(&scene);
        assert!(!draws.static_batches.is_batched(b.entity));
        assert_eq!(draws.static_batches.batches().count(), 0);
        assert_eq!(
            draws.draw_list.len(),
            2,
            "the moved object and the moved child"
        );
    }

    #[test]
    fn hiding_disabling_or_repainting_a_batched_object_takes_it_out() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let cube = context.registries.assets.read().cube().expect("cube");
        let first = material(&context, "test/static_edited", "shaders/mesh/pbr");
        let second = material(&context, "test/static_edited_second", "shaders/mesh/pbr");
        let mut builder = SceneBuilder::new(context.registries.clone());
        let hidden = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
        let disabled = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
        let repainted = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
        let kept = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
        let mut scene = builder.finish();
        let mut draws = DrawCollection::new(&context).expect("white texture loads");
        draws.collect(&scene);
        assert!(draws.static_batches.individual().is_empty());

        let _ = scene.write_component::<ComponentID, _>(hidden, |id| id.visible = false);
        scene.set_component_enabled::<ComponentMesh>(disabled, false);
        let _ = scene.write_component::<ComponentMesh, _>(repainted, |mesh| {
            mesh.material = AssetRef::from_id(second.id());
        });
        draws.collect(&scene);
        for edited in [hidden, disabled, repainted] {
            assert!(!draws.static_batches.is_batched(edited.entity));
        }
        assert!(draws.static_batches.is_batched(kept.entity));
        let drawn: Vec<_> = draws.draw_list.iter().map(|draw| draw.mat_id).collect();
        assert_eq!(drawn.len(), 2, "the batch and the repainted object");
        assert!(drawn.contains(&second.id()));
    }

    #[test]
    fn another_scene_or_a_journal_overrun_merges_the_batches_from_scratch() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let cube = context.registries.assets.read().cube().expect("cube");
        let first = material(&context, "test/static_overrun", "shaders/mesh/pbr");
        let mut builder = SceneBuilder::new(context.registries.clone());
        let a = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
        let b = spawn_mesh(&mut builder, None, &cube, &first, Vec3::zeros());
        let mut scene = builder.finish();
        let mut draws = DrawCollection::new(&context).expect("white texture loads");
        draws.collect(&scene);
        scene.set_transform(a, &glm::translation(&vec3(1.0, 0.0, 0.0)));
        draws.collect(&scene);
        assert!(!draws.static_batches.is_batched(a.entity));

        // More changes than the journal holds: the batches start over, and
        // the moved object, which now holds still, batches again.
        for _ in 0..=ChangeJournal::CAPACITY {
            let _ = scene.write_component::<ComponentID, _>(b, |_| ());
        }
        draws.collect(&scene);
        assert!(draws.static_batches.is_batched(a.entity));
        assert!(draws.static_batches.is_batched(b.entity));

        let copy = scene.restore_snapshot(scene.snapshot());
        draws.collect(&copy);
        let copied = copy.find(scene.uuid(a)).unwrap();
        assert!(draws.static_batches.is_batched(copied.entity));
        assert_eq!(draws.static_batches.batches().count(), 1);
    }
}
