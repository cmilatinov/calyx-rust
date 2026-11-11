use calyx_engine::assets::{material::Material, mesh::Mesh, AssetRef};
use calyx_engine::component::{
    ColliderShape, Component, ComponentCollider, ComponentEventContext, ComponentID, ComponentMesh,
    ComponentRef, ComponentRigidBody, ComponentStart, ReflectComponent, ReflectComponentStart,
};
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::{vec3, Vec2};
use rapier3d::dynamics::RigidBodyType;
use serde::{Deserialize, Serialize};

const WALL_HEIGHT: f32 = 2.0;
const COVER_COLUMNS: [u32; 3] = [4, 7, 10];
const COVER_ROWS: [u32; 3] = [6, 8, 12];

#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "8591392e-d5f6-4543-89eb-93bc865073ea"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentStart)]
#[reflect_attr(name = "Arena Generator")]
#[serde(default)]
pub struct ComponentArena {
    /// Generate the simple grid when Play starts. Authored kit scenes turn
    /// this off and keep this component only for their arena bounds.
    pub generate_on_start: bool,
    pub mesh: AssetRef<Mesh>,
    pub material: AssetRef<Material>,
    pub columns: u32,
    pub rows: u32,
    pub cell_size: f32,
    /// The inner face of the wall round an authored arena, as one closed
    /// ring of ground points `(x, z)` in the arena's frame, which fog of
    /// war bounds sight by along with [`Self::border_outer`]; empty when
    /// the border's solids bound sight one by one, as a generated arena's
    /// do. The arena's generator writes both from the outline it builds
    /// the wall along.
    pub border_inner: Vec<Vec2>,
    /// The outer face of that wall, round the inner one.
    pub border_outer: Vec<Vec2>,
}

impl Default for ComponentArena {
    fn default() -> Self {
        Self {
            generate_on_start: true,
            mesh: Default::default(),
            material: Default::default(),
            columns: 15,
            rows: 16,
            cell_size: 2.0,
            border_inner: Vec::new(),
            border_outer: Vec::new(),
        }
    }
}

impl Component for ComponentArena {}

impl ComponentArena {
    /// Whether the arena is large enough to generate anything.
    fn is_valid(&self) -> bool {
        self.columns >= 3 && self.rows >= 3 && self.cell_size > 0.0
    }

    /// Width along X and depth along Z of the generated floor, walls
    /// included, centred on the arena object.
    pub(crate) fn extent(&self) -> Option<(f32, f32)> {
        self.is_valid().then_some((
            self.columns as f32 * self.cell_size,
            self.rows as f32 * self.cell_size,
        ))
    }

    /// Centres of the generated cover blocks, relative to the arena object.
    pub(crate) fn cover_positions(&self) -> Vec<nalgebra_glm::Vec3> {
        if !self.is_valid() {
            return Vec::new();
        }
        cover_cells(self.columns, self.rows)
            .into_iter()
            .map(|(column, row)| {
                grid_position(column, row, self.columns, self.rows, self.cell_size)
            })
            .collect()
    }
}

impl ComponentStart for ComponentArena {
    fn start(
        this: ComponentRef<Self>,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
    ) {
        let Some(arena) = this.read(scene, |arena| arena.clone()) else {
            return;
        };
        generate_arena(scene, game_object, &arena);
    }
}

fn generate_arena(scene: &mut Scene, parent: GameObject, arena: &ComponentArena) {
    if !arena.generate_on_start || !arena.is_valid() {
        return;
    }

    let width = arena.columns as f32 * arena.cell_size;
    let depth = arena.rows as f32 * arena.cell_size;
    spawn_cube(
        scene,
        parent,
        "Arena Floor".to_string(),
        vec3(0.0, -0.5, 0.0),
        vec3(width, 1.0, depth),
        vec3(width * 0.5, 0.5, depth * 0.5),
        arena,
    );

    for (column, row) in perimeter_cells(arena.columns, arena.rows) {
        spawn_grid_cube(scene, parent, "Arena Wall", column, row, arena);
    }
    for (column, row) in cover_cells(arena.columns, arena.rows) {
        spawn_grid_cube(scene, parent, "Arena Cover", column, row, arena);
    }
}

fn spawn_grid_cube(
    scene: &mut Scene,
    parent: GameObject,
    prefix: &str,
    column: u32,
    row: u32,
    arena: &ComponentArena,
) {
    let position = grid_position(column, row, arena.columns, arena.rows, arena.cell_size);
    let scale = vec3(arena.cell_size, WALL_HEIGHT, arena.cell_size);
    spawn_cube(
        scene,
        parent,
        format!("{prefix} {column}-{row}"),
        position,
        scale,
        scale * 0.5,
        arena,
    );
}

fn spawn_cube(
    scene: &mut Scene,
    parent: GameObject,
    name: String,
    position: nalgebra_glm::Vec3,
    scale: nalgebra_glm::Vec3,
    half_extents: nalgebra_glm::Vec3,
    arena: &ComponentArena,
) {
    let object = scene.create(
        Some(ComponentID {
            name,
            ..Default::default()
        }),
        Some(parent),
    );
    scene.set_transform(
        object,
        &Transform::from_components(position, Default::default(), scale).matrix(),
    );
    scene.add_component(
        object,
        ComponentMesh {
            mesh: arena.mesh.clone(),
            material: arena.material.clone(),
            ..Default::default()
        },
    );
    scene.add_component(
        object,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            gravity_scale: 0.0,
            can_sleep: false,
            ..Default::default()
        },
    );
    scene.add_component(
        object,
        ComponentCollider {
            shape: ColliderShape::Cuboid { half_extents },
            friction: 0.8,
            ..Default::default()
        },
    );
}

fn grid_position(
    column: u32,
    row: u32,
    columns: u32,
    rows: u32,
    cell_size: f32,
) -> nalgebra_glm::Vec3 {
    vec3(
        (column as f32 - (columns - 1) as f32 * 0.5) * cell_size,
        WALL_HEIGHT * 0.5,
        (row as f32 - (rows - 1) as f32 * 0.5) * cell_size,
    )
}

fn perimeter_cells(columns: u32, rows: u32) -> Vec<(u32, u32)> {
    let mut cells = Vec::with_capacity((columns * 2 + rows.saturating_sub(2) * 2) as usize);
    for column in 0..columns {
        cells.push((column, 0));
        cells.push((column, rows - 1));
    }
    for row in 1..rows - 1 {
        cells.push((0, row));
        cells.push((columns - 1, row));
    }
    cells
}

fn cover_cells(columns: u32, rows: u32) -> Vec<(u32, u32)> {
    COVER_COLUMNS
        .into_iter()
        .flat_map(|column| COVER_ROWS.into_iter().map(move |row| (column, row)))
        .filter(|(column, row)| *column > 0 && *column < columns - 1 && *row > 0 && *row < rows - 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{cover_cells, generate_arena, perimeter_cells, ComponentArena};
    use calyx_engine::assets::AssetAccess;
    use calyx_engine::component::ComponentMesh;
    use calyx_engine::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
    use rapier3d::dynamics::RigidBodyType;
    use std::collections::HashSet;

    #[test]
    fn arena_layout_uses_unique_grid_cells() {
        let perimeter = perimeter_cells(15, 16);
        let covers = cover_cells(15, 16);

        assert_eq!(perimeter.len(), 58);
        assert_eq!(perimeter.iter().copied().collect::<HashSet<_>>().len(), 58);
        assert_eq!(covers.len(), 9);
        assert!(covers.iter().all(|cell| !perimeter.contains(cell)));
    }

    #[test]
    fn generator_creates_independent_fixed_grid_cubes() {
        let mut scene = calyx_engine::test_support::test_scene();
        let arena_object = scene.create(None, None);
        scene.add_component(arena_object, ComponentArena::default());
        scene.start();
        scene.start();

        let children: Vec<_> = scene.children(arena_object).collect();
        let wall_count = children
            .iter()
            .filter(|object| scene.name(**object).starts_with("Arena Wall"))
            .count();
        let cover_count = children
            .iter()
            .filter(|object| scene.name(**object).starts_with("Arena Cover"))
            .count();

        assert_eq!(children.len(), 68);
        assert_eq!(wall_count, 58);
        assert_eq!(cover_count, 9);
        for object in children
            .into_iter()
            .filter(|object| scene.name(*object) != "Arena Floor")
        {
            let rigid_body = scene
                .read_component::<ComponentRigidBody, _, _>(object, |body| body.ty)
                .expect("grid cube should have a rigid body");
            assert_eq!(rigid_body, RigidBodyType::Fixed);

            let shape = scene
                .read_component::<ComponentCollider, _, _>(object, |collider| collider.shape)
                .expect("grid cube should have a collider");
            let ColliderShape::Cuboid { half_extents } = shape else {
                panic!("grid cube collider should be a cuboid");
            };
            assert_eq!(half_extents, nalgebra_glm::vec3(1.0, 1.0, 1.0));
        }
    }

    #[test]
    fn authored_scene_keeps_its_layout_when_started_and_reloaded() {
        let copy = calyx_engine::test_support::TestAssets::game();
        let assets = copy.asset_context();
        let scene_ref = assets
            .registries
            .assets
            .read()
            .reload_by_path::<calyx_engine::scene::Scene>(
                &copy.sandbox_root().join("game/scenes/arena.cxscene"),
            )
            .expect("arena should load");
        let mut scene = scene_ref.read().clone();
        let arena = scene
            .objects()
            .find(|object| scene.name(*object) == "Arena")
            .unwrap();
        assert_eq!(
            scene.read_component::<ComponentArena, _, _>(arena, |arena| arena.generate_on_start),
            Some(false)
        );
        let count = scene.objects().count();
        let colliders = scene.descendants_with::<ComponentCollider>(arena).count();
        assert!(colliders > 50, "the authored pen and cover have collision");
        // Saved before colliders had groups, every collider keeps the default
        // groups, stopping hulls, rounds and sight alike, and saves unchanged;
        // only the farmland creek's blockers, the bridge's rails and the
        // barn yard's paddock fence stop tanks alone.
        let mut tank_blockers = 0;
        // Brush volumes, which stop nothing, have their own.
        for object in scene.descendants_with::<ComponentCollider>(arena) {
            if scene
                .ancestor_with::<crate::brush::ComponentBrush>(object)
                .is_some()
            {
                scene.read_component::<ComponentCollider, _, _>(object, |collider| {
                    assert_eq!(collider.groups, crate::collision::BRUSH);
                });
                continue;
            }
            let name = scene.name(object);
            scene.read_component::<ComponentCollider, _, _>(object, |collider| {
                if name.starts_with("Arena Creek Blocker")
                    || name.starts_with("Arena Bridge Rail")
                    || name.starts_with("Arena Paddock Fence")
                {
                    assert_eq!(collider.groups, crate::collision::TANK_BLOCKER, "{name}");
                    tank_blockers += 1;
                } else {
                    assert!(collider.groups.is_all(), "{name}");
                    let saved = serde_json::to_value(collider).unwrap();
                    assert!(saved.get("groups").is_none(), "{saved}");
                }
            });
        }
        assert!(tank_blockers > 10, "the creek has blockers");
        let registries = assets.registries.lock_read();
        let mut checked = HashSet::new();
        for object in scene.descendants_with::<ComponentMesh>(arena) {
            scene.read_component::<ComponentMesh, _, _>(object, |renderer| {
                if checked.insert((renderer.mesh.id(), renderer.material.id())) {
                    assert!(
                        renderer.mesh.get_ref(&registries).is_some(),
                        "missing mesh {}",
                        renderer.mesh.id()
                    );
                    assert!(
                        renderer.material.get_ref(&registries).is_some(),
                        "missing material {}",
                        renderer.material.id()
                    );
                }
            });
        }
        scene.start();
        scene.start();
        assert_eq!(scene.objects().count(), count);
        assert_eq!(
            scene.descendants_with::<ComponentCollider>(arena).count(),
            colliders
        );
        let mut reloaded = scene.restore_snapshot(scene.snapshot());
        reloaded.start();
        assert_eq!(reloaded.objects().count(), count);
    }

    #[test]
    fn a_decorative_child_does_not_disable_the_grid_generator() {
        let mut scene = calyx_engine::test_support::test_scene();
        let arena = scene.create(None, None);
        scene.create(None, Some(arena));
        generate_arena(&mut scene, arena, &ComponentArena::default());
        assert_eq!(scene.children(arena).count(), 69);
    }
}
