use crate::assets::animation::Animation;
use crate::assets::error::AssetError;
use crate::assets::import::{self, ImportedNode};
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::{Asset, AssetRegistry, LoadedAsset};
use crate::component::{
    ComponentBone, ComponentID, ComponentMesh, ComponentSkinnedMesh, ComponentTransform,
};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::Transform;
use crate::scene::SceneData;
use crate::utils::TypeUuid;
use crate::{self as engine, utils};
use nalgebra_glm::Mat4;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::io::BufReader;
use std::path::Path;
use uuid::Uuid;

const DEFAULT_MATERIAL_NAME: &str = "materials/default";

/// Serialized subtree asset that can be instantiated into a [`crate::scene::Scene`].
#[derive(Serialize, TypeUuid)]
#[uuid = "960f1d60-3ad4-4f1d-92d3-cceb0e0623d7"]
pub struct Prefab {
    /// Serialized scene subtree.
    pub data: SceneData,
    /// UUID of the root object inside `data`.
    pub root: Uuid,
}

/// Helper type used when loading a prefab from JSON.
#[derive(Deserialize)]
pub struct PrefabData {
    /// Serialized scene subtree.
    pub data: SceneData,
    /// UUID of the root object inside `data`.
    pub root: Uuid,
}

impl From<PrefabData> for Prefab {
    fn from(value: PrefabData) -> Self {
        Self {
            data: value.data,
            root: value.root,
        }
    }
}

impl Asset for Prefab {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Prefab"
    }

    fn file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxprefab", "fbx", "gltf", "glb"]
    }

    fn from_file(game: &ReadOnlyAssetContext, path: &Path) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        let load_error = || {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
        };
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| load_error().with_source("path has no valid file extension"))?;
        let asset_registry = game.registries.assets.read();
        let meta = asset_registry
            .asset_meta_from_path(path)
            .ok_or_else(|| load_error().with_source("asset metadata not found"))?;
        if ext != "cxprefab" {
            let scene = import::import(path).map_err(|err| err.with_type(Self::asset_name()))?;

            let mut bones = HashSet::new();
            let mut mesh_names = HashSet::new();
            let mut meshes = Vec::new();
            for mesh in &scene.meshes {
                let name = imported_sub_asset_name(&mesh.name, "mesh", &mut mesh_names);
                let mesh_ref = asset_registry.create_or_update_sub_asset(
                    meta.id,
                    &meta.name,
                    &name,
                    Mesh::from_imported(&game.render_context, mesh),
                )?;
                meshes.push(mesh_ref.clone());
                bones.extend(mesh.bones.iter().map(|bone| bone.name.clone()));
            }

            let (data, root) = Self::scene_data(&asset_registry, &bones, &meshes, &scene.root)?;

            let mut animation_names = HashSet::new();
            let mut animations = Vec::new();
            for anim in scene.animations {
                let name =
                    imported_sub_asset_name(anim.name.as_str(), "animation", &mut animation_names);
                let anim_ref = asset_registry.create_or_update_sub_asset(
                    meta.id,
                    &meta.name,
                    &name,
                    Animation::from_imported(anim),
                )?;
                animations.push(anim_ref.id());
            }

            Ok(LoadedAsset {
                asset: Self { data, root },
                sub_assets: meshes
                    .into_iter()
                    .map(|mesh_ref| mesh_ref.id())
                    .chain(animations)
                    .collect(),
            })
        } else {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(|err| {
                    AssetError::LoadError
                        .with_path(path)
                        .with_type(Self::asset_name())
                        .with_source(err)
                })?;
            let reader = BufReader::new(file);
            let data: PrefabData = serde_json::from_reader(reader).map_err(|err| {
                AssetError::LoadError
                    .with_path(path)
                    .with_type(Self::asset_name())
                    .with_source(err)
            })?;
            Ok(LoadedAsset::new(data.into()))
        }
    }
}

fn imported_sub_asset_name(
    source_name: &str,
    fallback_name: &str,
    used_names: &mut HashSet<String>,
) -> String {
    let stem = if source_name.is_empty() {
        fallback_name
    } else {
        source_name
    };
    let mut candidate = stem.to_string();
    let mut suffix = 1;
    while !used_names.insert(candidate.clone()) {
        candidate = format!("{stem}_{suffix}");
        suffix += 1;
    }
    candidate
}

/// Resolves the default material assigned to imported meshes.
fn default_material_id(registry: &AssetRegistry) -> Result<Uuid, AssetError> {
    registry.asset_id(DEFAULT_MATERIAL_NAME).ok_or_else(|| {
        AssetError::NotFound
            .with_type(Material::asset_name())
            .with_source(format!(
                "default material '{DEFAULT_MATERIAL_NAME}' not found"
            ))
    })
}

impl Prefab {
    /// The component showing `mesh_ref` with the default material: a skinned mesh driven by the
    /// bones under `root_id` when the mesh has bones, else a static mesh.
    fn mesh_component(
        registry: &AssetRegistry,
        mesh_ref: &Ref<Mesh>,
        root_id: Uuid,
    ) -> Result<(Uuid, serde_json::Value), AssetError> {
        let material_id = default_material_id(registry)?;
        Ok(if mesh_ref.read().bones.is_empty() {
            (
                ComponentMesh::type_uuid(),
                json!({
                    "material": material_id.to_string(),
                    "mesh": mesh_ref.id().to_string()
                }),
            )
        } else {
            (
                ComponentSkinnedMesh::type_uuid(),
                json!({
                    "material": material_id.to_string(),
                    "mesh": mesh_ref.id().to_string(),
                    "root_bone": root_id
                }),
            )
        })
    }

    /// Builds the scene data of an imported node tree and returns it with the root's id. Each
    /// object's id is seeded from its node's path, so nodes that share a name or have none stay
    /// distinct, and the same file always produces the same ids.
    fn scene_data(
        registry: &AssetRegistry,
        bones: &HashSet<String>,
        meshes: &[Ref<Mesh>],
        root: &ImportedNode,
    ) -> Result<(SceneData, Uuid), AssetError> {
        let mut data = SceneData::default();
        let root_id = utils::uuid_from_str(&root.name);
        Self::traverse(
            registry, bones, meshes, root_id, root, &root.name, None, &mut data,
        )?;
        Ok((data, root_id))
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse(
        registry: &AssetRegistry,
        bones: &HashSet<String>,
        meshes: &[Ref<Mesh>],
        root_id: Uuid,
        node: &ImportedNode,
        path: &str,
        mut parent: Option<Uuid>,
        data: &mut SceneData,
    ) -> Result<(), AssetError> {
        let matrix: Mat4 = node.transform;
        let id = utils::uuid_from_str(path);
        if let Some(parent_id) = parent {
            data.hierarchy.entry(parent_id).or_default().push(id);
        }
        parent = Some(id);
        let entry = data.components.entry(id).or_default();
        entry.insert(
            ComponentID::type_uuid(),
            json!({
                "id": id.to_string(),
                "name": node.name.clone()
            }),
        );
        entry.insert(
            ComponentTransform::type_uuid(),
            json!({
                "transform": Transform::from(matrix)
            }),
        );
        // The node's first mesh goes on its own object; each further mesh, such as another
        // material's part, goes on a child object at the node's origin.
        let node_meshes = node
            .meshes
            .iter()
            .filter_map(|&index| meshes.get(index))
            .collect::<Vec<_>>();
        if let Some(mesh_ref) = node_meshes.first() {
            let (type_uuid, component) = Self::mesh_component(registry, mesh_ref, root_id)?;
            entry.insert(type_uuid, component);
        }
        for (part, mesh_ref) in node_meshes.iter().enumerate().skip(1) {
            let part_id = utils::uuid_from_str(&format!("{path}#{part}"));
            data.hierarchy.entry(id).or_default().push(part_id);
            let (type_uuid, component) = Self::mesh_component(registry, mesh_ref, root_id)?;
            data.components.insert(
                part_id,
                [
                    (
                        ComponentID::type_uuid(),
                        json!({
                            "id": part_id.to_string(),
                            "name": format!("{}_{part}", node.name)
                        }),
                    ),
                    (
                        ComponentTransform::type_uuid(),
                        json!({ "transform": Transform::default() }),
                    ),
                    (type_uuid, component),
                ]
                .into_iter()
                .collect(),
            );
        }
        let entry = data.components.entry(id).or_default();
        // Each skinned mesh keeps its own bone indices and inverse bind matrices, and finds its
        // bones by name.
        if bones.contains(&node.name) {
            entry.insert(
                ComponentBone::type_uuid(),
                json!({ "name": node.name.clone() }),
            );
        }
        for (index, child) in node.children.iter().enumerate() {
            let child_path = format!("{path}/{index}:{}", child.name);
            Self::traverse(
                registry,
                bones,
                meshes,
                root_id,
                child,
                &child_path,
                parent,
                data,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_game_context;

    #[test]
    fn from_file_returns_error_for_path_without_extension() {
        let context = test_game_context().assets.lock_read();
        let path = std::env::temp_dir().join(format!("prefab-{}", Uuid::new_v4()));

        let Err(err) = Prefab::from_file(&context, &path) else {
            panic!("path without extension should fail");
        };

        assert_eq!(err.asset_type, Some(Prefab::asset_name()));
        assert_eq!(err.path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn from_file_returns_error_for_unregistered_path() {
        let context = test_game_context().assets.lock_read();
        let path = std::env::temp_dir().join(format!("prefab-{}.fbx", Uuid::new_v4()));

        let Err(err) = Prefab::from_file(&context, &path) else {
            panic!("path without asset metadata should fail");
        };

        assert_eq!(err.asset_type, Some(Prefab::asset_name()));
        assert_eq!(err.path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn default_material_id_returns_error_when_missing() {
        let context = test_game_context().assets.lock_read();
        let registry = context.registries.assets.read();

        let Err(err) = default_material_id(&registry) else {
            panic!("missing default material should fail");
        };

        assert_eq!(err.kind, AssetError::NotFound.kind);
    }

    #[test]
    fn imported_sub_asset_names_are_unique() {
        let mut used_names = HashSet::new();

        assert_eq!(
            imported_sub_asset_name("Tracks", "mesh", &mut used_names),
            "Tracks"
        );
        assert_eq!(
            imported_sub_asset_name("Tracks", "mesh", &mut used_names),
            "Tracks_1"
        );
        assert_eq!(imported_sub_asset_name("", "mesh", &mut used_names), "mesh");
    }

    fn node(name: &str, children: Vec<ImportedNode>) -> ImportedNode {
        ImportedNode {
            name: name.to_string(),
            transform: Mat4::identity(),
            meshes: Vec::new(),
            children,
        }
    }

    #[test]
    fn nodes_with_shared_or_empty_names_stay_distinct_and_stable() {
        let context = test_game_context().assets.lock_read();
        let registry = context.registries.assets.read();
        let root = node(
            "RootNode",
            vec![
                node("Wheel", Vec::new()),
                node("Wheel", Vec::new()),
                node("", vec![node("", Vec::new())]),
            ],
        );

        let (data, root_id) = Prefab::scene_data(&registry, &HashSet::new(), &[], &root).unwrap();
        let (again, again_root_id) =
            Prefab::scene_data(&registry, &HashSet::new(), &[], &root).unwrap();

        assert_eq!(data.components.len(), 5);
        assert_eq!(data.hierarchy[&root_id].len(), 3);
        let unnamed = data.hierarchy[&root_id][2];
        assert_eq!(data.hierarchy[&unnamed].len(), 1);
        assert_eq!(again_root_id, root_id);
        assert_eq!(again.hierarchy, data.hierarchy);
    }

    #[test]
    fn malformed_model_file_returns_load_error_naming_the_file() {
        let assets = crate::test_support::TestAssets::models();
        let files: [(&str, &[u8]); 3] = [
            ("broken.fbx", b"Kaydara FBX Binary  \0 not really"),
            (
                "broken.gltf",
                br#"{"asset": {"version": "2.0"}, "nodes": ["#,
            ),
            ("broken.glb", b"glTF\x02\0\0\0\x18\0\0\0not really"),
        ];
        for (name, contents) in files {
            std::fs::write(assets.fixtures_root().join(name), contents).unwrap();
        }
        let context = assets.asset_context();
        let context = context.lock_read();

        for (name, _) in files {
            let path = assets.fixtures_root().join(name);
            let Err(err) = Prefab::from_file(&context, &path) else {
                panic!("malformed {name} should fail to import");
            };

            assert_eq!(err.kind, AssetError::LoadError.kind, "{name}");
            assert_eq!(err.asset_type, Some(Prefab::asset_name()), "{name}");
            assert_eq!(err.path.as_deref(), Some(path.as_path()), "{name}");
            assert!(err.source.is_some(), "{name}");
        }
    }

    /// Imports the sandbox tank, whose left and right parts share node names.
    fn import_tank() -> Prefab {
        let assets = crate::test_support::TestAssets::models();
        let context = assets.asset_context();
        let context = context.lock_read();
        let path = assets.sandbox_root().join("models/tank_game/Tank_002.fbx");
        Prefab::from_file(&context, &path)
            .unwrap_or_else(|e| panic!("failed to import the tank: {e}"))
            .asset
    }

    #[test]
    fn tank_parts_sharing_a_name_import_as_distinct_objects() {
        let tank = import_tank();
        let children = tank.data.hierarchy.values().flatten().collect::<Vec<_>>();
        let distinct = children.iter().collect::<HashSet<_>>();

        assert_eq!(distinct.len(), children.len(), "a node id appears twice");
        assert_eq!(tank.data.components.len(), children.len() + 1);
        let again = import_tank();
        assert_eq!(again.root, tank.root);
        assert_eq!(again.data.hierarchy, tank.data.hierarchy);
    }

    /// Name of each object holding a mesh, and whether the mesh is skinned, sorted by name.
    fn mesh_objects(prefab: &Prefab) -> Vec<(String, bool)> {
        let mut objects = prefab
            .data
            .components
            .values()
            .filter_map(|components| {
                let name = components[&ComponentID::type_uuid()]["name"]
                    .as_str()?
                    .to_string();
                if components.contains_key(&ComponentSkinnedMesh::type_uuid()) {
                    Some((name, true))
                } else if components.contains_key(&ComponentMesh::type_uuid()) {
                    Some((name, false))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        objects.sort();
        objects
    }

    #[test]
    fn every_part_of_a_shared_mesh_is_attached_with_its_node_skin() {
        // One mesh of two primitives, used by a skinned node and by an unskinned node.
        let assets = crate::test_support::TestAssets::models();
        let context = assets.asset_context();
        let context = context.lock_read();
        let path = assets.fixtures_root().join("gltf/SharedMesh.gltf");
        let prefab = Prefab::from_file(&context, &path)
            .unwrap_or_else(|e| panic!("failed to import the shared mesh: {e}"))
            .asset;

        assert_eq!(
            mesh_objects(&prefab),
            [
                ("Skinned".to_string(), true),
                ("Skinned_1".to_string(), true),
                ("Static".to_string(), false),
                ("Static_1".to_string(), false),
            ]
        );
    }

    #[test]
    fn every_material_part_of_an_fbx_node_is_attached() {
        // One cube node whose top face uses a second material.
        let assets = crate::test_support::TestAssets::models();
        let context = assets.asset_context();
        let context = context.lock_read();
        let path = assets.fixtures_root().join("fbx/TwoMaterials.fbx");
        let prefab = Prefab::from_file(&context, &path)
            .unwrap_or_else(|e| panic!("failed to import the two material cube: {e}"))
            .asset;

        assert_eq!(
            mesh_objects(&prefab),
            [("Cube".to_string(), false), ("Cube_1".to_string(), false)]
        );
    }
}
