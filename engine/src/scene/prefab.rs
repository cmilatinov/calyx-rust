use crate::assets::animation::Animation;
use crate::assets::error::AssetError;
use crate::assets::mesh::Mesh;
use crate::assets::{Asset, AssetRegistry, LoadedAsset};
use crate::component::{
    ComponentBone, ComponentID, ComponentMesh, ComponentSkinnedMesh, ComponentTransform,
};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::{self, Transform};
use crate::scene::{Scene, SceneData};
use crate::utils::TypeUuid;
use crate::{self as engine, utils};
use nalgebra_glm::Mat4;
use russimp::property::{Property, PropertyStore};
use russimp::scene::PostProcess;
use russimp::sys::AI_CONFIG_IMPORT_FBX_PRESERVE_PIVOTS;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::io::BufReader;
use std::path::Path;
use uuid::Uuid;

#[derive(Serialize, TypeUuid)]
#[uuid = "960f1d60-3ad4-4f1d-92d3-cceb0e0623d7"]
pub struct Prefab {
    #[serde(skip)]
    pub scene: Scene,
    pub data: SceneData,
}

#[derive(Deserialize)]
pub struct PrefabData {
    pub data: SceneData,
}

impl From<(&ReadOnlyAssetContext, PrefabData)> for Prefab {
    fn from((game, value): (&ReadOnlyAssetContext, PrefabData)) -> Self {
        Self {
            data: value.data.clone(),
            scene: (game, value.data).into(),
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
        &["cxprefab", "fbx", "dae"]
    }

    fn from_file(game: &ReadOnlyAssetContext, path: &Path) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        let ext = path.extension().and_then(|ext| ext.to_str()).unwrap();
        let asset_registry = game.asset_registry.read();
        let meta = asset_registry.asset_meta_from_path(path).unwrap();
        if ext == "fbx" || ext == "dae" {
            let props: PropertyStore = [(
                AI_CONFIG_IMPORT_FBX_PRESERVE_PIVOTS as &[u8],
                Property::Integer(0),
            )]
            .into_iter()
            .into();
            let scene = russimp::scene::Scene::from_file_with_props(
                path.to_str().unwrap(),
                vec![
                    PostProcess::Triangulate,
                    PostProcess::GenerateSmoothNormals,
                    PostProcess::FlipUVs,
                    PostProcess::FlipWindingOrder,
                    PostProcess::JoinIdenticalVertices,
                ],
                &props,
            )?;

            let mut bones = HashMap::new();
            let mut meshes = Vec::new();
            for mesh in &scene.meshes {
                let name = format!("{}/{}", meta.name, mesh.name);
                let mesh_ref = asset_registry
                    .create(name, Mesh::from_russimp_mesh(&game.render_context, mesh))?;
                meshes.push(mesh_ref.clone());
                bones.extend(mesh.bones.iter().enumerate().map(|(i, b)| {
                    let offset_matrix = math::mat4_from_russimp(&b.offset_matrix);
                    (b.name.clone(), (i, offset_matrix))
                }));
            }

            let mut data: SceneData = Default::default();
            if let Some(root) = &scene.root {
                Self::traverse(
                    &asset_registry,
                    &bones,
                    &meshes,
                    root,
                    root,
                    None,
                    &mut data,
                );
            }

            let mut animations = Vec::new();
            for anim in &scene.animations {
                let name = format!(
                    "{}/{}",
                    meta.name,
                    if anim.name.is_empty() {
                        "animation"
                    } else {
                        anim.name.as_str()
                    }
                );
                let anim_ref =
                    asset_registry.create(name.clone(), Animation::from_russimp_animation(anim))?;
                animations.push(anim_ref.id());
            }

            Ok(LoadedAsset {
                asset: Self {
                    data: data.clone(),
                    scene: (game, data).into(),
                },
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
                .map_err(|_| AssetError::LoadError)?;
            let reader = BufReader::new(file);
            let data: PrefabData =
                serde_json::from_reader(reader).map_err(|_| AssetError::LoadError)?;
            Ok(LoadedAsset::new((game, data).into()))
        }
    }
}

impl Prefab {
    fn traverse(
        registry: &AssetRegistry,
        bones: &HashMap<String, (usize, Mat4)>,
        meshes: &Vec<Ref<Mesh>>,
        root: &russimp::node::Node,
        node: &russimp::node::Node,
        mut parent: Option<Uuid>,
        data: &mut SceneData,
    ) {
        let matrix: Mat4 = math::mat4_from_russimp(&node.transformation);
        let id = utils::uuid_from_str(node.name.as_str());
        if let Some(parent_id) = parent {
            data.hierarchy.insert(id, parent_id);
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
        if !node.meshes.is_empty() && node.meshes[0] < meshes.len() as u32 {
            let mesh_ref = meshes[node.meshes[0] as usize].clone();
            let material_id = registry.asset_id("materials/default").unwrap();
            let mesh = mesh_ref.read();
            if !mesh.bones.is_empty() {
                entry.insert(
                    ComponentSkinnedMesh::type_uuid(),
                    json!({
                        "material": material_id.to_string(),
                        "mesh": mesh_ref.id().to_string(),
                        "root_bone": utils::uuid_from_str(root.name.as_str())
                    }),
                );
            } else {
                entry.insert(
                    ComponentMesh::type_uuid(),
                    json!({
                        "material": material_id.to_string(),
                        "mesh": mesh_ref.id().to_string()
                    }),
                );
            }
        }
        if let Some((index, offset_matrix)) = bones.get(&node.name) {
            entry.insert(
                ComponentBone::type_uuid(),
                json!({
                    "name": node.name.clone(),
                    "index": index,
                    "offset_matrix": offset_matrix
                }),
            );
        }
        for child in &*node.children.borrow() {
            Self::traverse(registry, bones, meshes, root, child.borrow(), parent, data);
        }
    }
}
