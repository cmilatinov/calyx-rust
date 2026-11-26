use crate::assets::animation::Animation;
use crate::assets::error::AssetError;
use crate::assets::import::{self, ImportedNode};
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture;
use crate::assets::{Asset, AssetMeta, AssetRegistry, LoadedAsset};
use crate::component::{
    ComponentBone, ComponentID, ComponentMesh, ComponentSkinnedMesh, ComponentTransform,
};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::Transform;
use crate::render::Shader;
use crate::scene::SceneData;
use crate::utils::TypeUuid;
use crate::{self as calyx_engine, utils};
use nalgebra_glm::Mat4;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::io::BufReader;
use std::path::Path;
use uuid::Uuid;

/// Material of an imported mesh whose primitive names no material.
const DEFAULT_MATERIAL_NAME: &str = "materials/default";
/// Shader of the materials imported from a model file.
const IMPORTED_MATERIAL_SHADER_NAME: &str = "shaders/mesh/pbr";

/// Serialized subtree asset that can be instantiated into a [`crate::scene::Scene`].
///
/// A model file (FBX, glTF or GLB) loads as a prefab of its node tree. Its meshes, animations,
/// and for glTF its materials and textures, become sub-assets of the model, named after their
/// source names and made unique, with ids that stay the same while the names do. Each mesh
/// component takes the material its primitive names, or `materials/default`.
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

            // Every sub-asset of the model shares one namespace, so names are taken in a fixed
            // order, meshes first, and each keeps its id across reimports of the same file.
            let groups = [
                (
                    scene
                        .meshes
                        .iter()
                        .map(|m| m.name.as_str())
                        .collect::<Vec<_>>(),
                    "mesh",
                    Mesh::type_uuid(),
                ),
                (
                    scene.animations.iter().map(|a| a.name.as_str()).collect(),
                    "animation",
                    Animation::type_uuid(),
                ),
                (
                    scene.materials.iter().map(|m| m.name.as_str()).collect(),
                    "material",
                    Material::type_uuid(),
                ),
                (
                    scene.textures.iter().map(|t| t.name.as_str()).collect(),
                    "texture",
                    Texture::type_uuid(),
                ),
            ];
            let mut sub_asset_names = SubAssetNames::new(
                &asset_registry,
                &meta,
                groups.iter().flat_map(|(names, fallback, _)| {
                    names.iter().map(|name| sub_asset_stem(name, fallback))
                }),
            );
            let [mesh_names, animation_names, material_names, texture_names] =
                groups.map(|(names, fallback, type_uuid)| {
                    names
                        .into_iter()
                        .map(|name| sub_asset_names.allocate(name, fallback, type_uuid))
                        .collect::<Vec<_>>()
                });

            let mut textures = Vec::new();
            for (texture, name) in scene.textures.iter().zip(&texture_names) {
                textures.push(match Texture::from_imported(game, name, texture) {
                    Ok(texture) => Some(
                        asset_registry
                            .create_or_update_sub_asset(meta.id, &meta.name, name, texture)?
                            .id(),
                    ),
                    Err(error) => {
                        log::warn!(
                            "Texture {name} of {} did not upload, so the slots sampling it keep \
                             their default: {error}",
                            meta.name
                        );
                        None
                    }
                });
            }

            let mut materials = Vec::new();
            if !scene.materials.is_empty() {
                let shader = asset_registry.load::<Shader>(IMPORTED_MATERIAL_SHADER_NAME)?;
                for (material, name) in scene.materials.iter().zip(&material_names) {
                    let material =
                        Material::from_imported(game, shader.clone(), material, &textures);
                    materials.push(
                        asset_registry
                            .create_or_update_sub_asset(meta.id, &meta.name, name, material)?
                            .id(),
                    );
                }
            }

            let mut bones = HashSet::new();
            let mut meshes = Vec::new();
            for (mesh, name) in scene.meshes.iter().zip(&mesh_names) {
                let mesh_ref = asset_registry.create_or_update_sub_asset(
                    meta.id,
                    &meta.name,
                    name,
                    Mesh::from_imported(&game.render_context, mesh),
                )?;
                let material_id = match mesh.material.and_then(|index| materials.get(index)) {
                    Some(id) => *id,
                    None => default_material_id(&asset_registry)?,
                };
                meshes.push((mesh_ref, material_id));
                bones.extend(mesh.bones.iter().map(|bone| bone.name.clone()));
            }

            let (data, root) = Self::scene_data(&bones, &meshes, &scene.root);

            let mut animations = Vec::new();
            for (anim, name) in scene.animations.into_iter().zip(&animation_names) {
                let anim_ref = asset_registry.create_or_update_sub_asset(
                    meta.id,
                    &meta.name,
                    name,
                    Animation::from_imported(anim),
                )?;
                animations.push(anim_ref.id());
            }

            Ok(LoadedAsset {
                asset: Self { data, root },
                sub_assets: meshes
                    .into_iter()
                    .map(|(mesh_ref, _)| mesh_ref.id())
                    .chain(animations)
                    .chain(materials)
                    .chain(textures.into_iter().flatten())
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

/// The name a sub-asset takes before it is made unique: its source name, or `fallback_name`.
fn sub_asset_stem<'a>(source_name: &'a str, fallback_name: &'a str) -> &'a str {
    if source_name.is_empty() {
        fallback_name
    } else {
        source_name
    }
}

/// Allocates the unique local names of a model's sub-assets for one import.
///
/// A name is the [`sub_asset_stem`] with the lowest `_N` suffix that no earlier sub-asset of this
/// import took and that the registry does not hold as another type, under another parent, or as
/// a file asset, since the registry would refuse it. When that name is new to the model, a
/// sub-asset of the same type that the model had under a suffix of this stem keeps that name, and
/// so its id: a texture suffixed `Paint_1` because a material held `Paint` stays `Paint_1` after
/// the material is renamed. A suffixed name that is also the stem of another sub-asset in this
/// import is left to that sub-asset. When the model's sub-assets already have the names this
/// import gives them, every one keeps its name.
struct SubAssetNames<'a> {
    registry: &'a AssetRegistry,
    parent: &'a AssetMeta,
    /// The model's sub-assets before this import.
    previous: Vec<AssetMeta>,
    /// The stem of every sub-asset in this import.
    stems: HashSet<String>,
    used: HashSet<String>,
}

impl<'a> SubAssetNames<'a> {
    fn new<'s>(
        registry: &'a AssetRegistry,
        parent: &'a AssetMeta,
        stems: impl IntoIterator<Item = &'s str>,
    ) -> Self {
        Self {
            registry,
            parent,
            previous: parent
                .children
                .iter()
                .filter_map(|id| registry.asset_meta_from_id(*id))
                .collect(),
            stems: stems.into_iter().map(str::to_string).collect(),
            used: HashSet::new(),
        }
    }

    fn allocate(&mut self, source_name: &str, fallback_name: &str, type_uuid: Uuid) -> String {
        let stem = sub_asset_stem(source_name, fallback_name);
        let available = (0..)
            .map(|suffix| match suffix {
                0 => stem.to_string(),
                suffix => format!("{stem}_{suffix}"),
            })
            .find(|name| !self.used.contains(name) && self.is_free(name, type_uuid))
            .expect("some suffix is free");
        let is_previous = |name: &str| {
            self.previous
                .iter()
                .any(|meta| meta.type_uuid == type_uuid && meta.name == name)
        };
        let name = if is_previous(&available) {
            available
        } else {
            let suffix = |name: &str| -> Option<usize> {
                let digits = name.strip_prefix(stem)?.strip_prefix('_')?;
                let suffix = digits.parse::<usize>().ok()?;
                (suffix > 0 && suffix.to_string() == digits).then_some(suffix)
            };
            self.previous
                .iter()
                .filter(|meta| {
                    meta.type_uuid == type_uuid
                        && !self.used.contains(&meta.name)
                        && !self.stems.contains(&meta.name)
                })
                .filter_map(|meta| suffix(&meta.name).map(|suffix| (suffix, &meta.name)))
                .min_by_key(|(suffix, _)| *suffix)
                .map_or(available, |(_, name)| name.clone())
        };
        self.used.insert(name.clone());
        name
    }

    /// Whether the registry would accept `name` for a sub-asset of this model of `type_uuid`.
    fn is_free(&self, name: &str, type_uuid: Uuid) -> bool {
        self.registry
            .asset_meta(&format!("{}/{name}", self.parent.name))
            .is_none_or(|meta| {
                meta.type_uuid == type_uuid
                    && meta.parent == Some(self.parent.id)
                    && meta.path.is_none()
            })
    }
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
    /// The component showing `mesh_ref` with the material `material_id`: a skinned mesh driven
    /// by the bones under `root_id` when the mesh has bones, else a static mesh.
    fn mesh_component(
        mesh_ref: &Ref<Mesh>,
        material_id: Uuid,
        root_id: Uuid,
    ) -> (Uuid, serde_json::Value) {
        if mesh_ref.read().bones.is_empty() {
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
        }
    }

    /// Builds the scene data of an imported node tree and returns it with the root's id.
    /// `meshes` holds each imported mesh with the id of its material. Each object's id is
    /// seeded from its node's path, so nodes that share a name or have none stay distinct, and
    /// the same file always produces the same ids.
    fn scene_data(
        bones: &HashSet<String>,
        meshes: &[(Ref<Mesh>, Uuid)],
        root: &ImportedNode,
    ) -> (SceneData, Uuid) {
        let root_id = utils::uuid_from_str(&root.name);
        let mut data = SceneData {
            roots: vec![root_id],
            ..Default::default()
        };
        Self::traverse(bones, meshes, root_id, root, &root.name, None, &mut data);
        (data, root_id)
    }

    fn traverse(
        bones: &HashSet<String>,
        meshes: &[(Ref<Mesh>, Uuid)],
        root_id: Uuid,
        node: &ImportedNode,
        path: &str,
        mut parent: Option<Uuid>,
        data: &mut SceneData,
    ) {
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
        if let Some((mesh_ref, material_id)) = node_meshes.first() {
            let (type_uuid, component) = Self::mesh_component(mesh_ref, *material_id, root_id);
            entry.insert(type_uuid, component);
        }
        for (part, (mesh_ref, material_id)) in node_meshes.iter().enumerate().skip(1) {
            let part_id = utils::uuid_from_str(&format!("{path}#{part}"));
            data.hierarchy.entry(id).or_default().push(part_id);
            let (type_uuid, component) = Self::mesh_component(mesh_ref, *material_id, root_id);
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
            Self::traverse(bones, meshes, root_id, child, &child_path, parent, data);
        }
    }
}

#[cfg(test)]
mod tests;
