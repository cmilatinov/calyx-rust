use engine::assets::{material::Material, mesh::Mesh, AssetRef};
use engine::component::{
    ColliderShape, Component, ComponentCollider, ComponentEventContext, ComponentID, ComponentMesh,
    ComponentRigidBody, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use engine::input::Input;
use engine::math::Transform;
use engine::reflect::{Reflect, ReflectDefault};
use engine::resource::ResourceMap;
use engine::scene::{GameObject, Scene};
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::vec3;
use rapier3d::dynamics::RigidBodyType;
use serde::{Deserialize, Serialize};

const WALL_HEIGHT: f32 = 2.0;
const COVER_COLUMNS: [u32; 3] = [4, 7, 10];
const COVER_ROWS: [u32; 3] = [6, 8, 12];

#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "8591392e-d5f6-4543-89eb-93bc865073ea"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Arena Generator")]
#[serde(default)]
#[repr(C)]
pub struct ComponentArena {
    pub mesh: AssetRef<Mesh>,
    pub material: AssetRef<Material>,
    pub columns: u32,
    pub rows: u32,
    pub cell_size: f32,
    #[serde(skip)]
    #[reflect_skip]
    generated: bool,
}

impl Default for ComponentArena {
    fn default() -> Self {
        Self {
            mesh: Default::default(),
            material: Default::default(),
            columns: 15,
            rows: 16,
            cell_size: 2.0,
            generated: false,
        }
    }
}

impl Component for ComponentArena {}

impl ComponentUpdate for ComponentArena {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        _resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let Some(arena) =
            scene.read_component::<ComponentArena, _, _>(game_object, |arena| arena.clone())
        else {
            return;
        };
        if arena.generated {
            return;
        }

        generate_arena(scene, game_object, &arena);
        let _ = scene.write_component::<ComponentArena, _>(game_object, |arena| {
            arena.generated = true;
        });
    }
}

fn generate_arena(scene: &mut Scene, parent: GameObject, arena: &ComponentArena) {
    if arena.columns < 3 || arena.rows < 3 || arena.cell_size <= 0.0 {
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
    use engine::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
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
        let mut scene = engine::test_support::test_scene();
        let arena_object = scene.create(None, None);
        generate_arena(&mut scene, arena_object, &ComponentArena::default());

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
}
