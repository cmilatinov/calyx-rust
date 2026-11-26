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
mod tests;
