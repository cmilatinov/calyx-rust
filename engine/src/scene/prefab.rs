use crate::assets::animation::Animation;
use crate::assets::error::AssetError;
use crate::assets::mesh::Mesh;
use crate::assets::{Asset, AssetRegistry, LoadedAsset};
use crate::component::{
    ComponentBone, ComponentID, ComponentMesh, ComponentSkinnedMesh, ComponentTransform,
};
use crate::core::Ref;
use crate::math::{self, Transform};
use crate::scene::{Scene, SceneData};
use crate::utils::TypeUuid;
use crate::{self as engine, utils};
use glm::Mat4;
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

#[derive(Serialize, Deserialize, TypeUuid)]
#[uuid = "960f1d60-3ad4-4f1d-92d3-cceb0e0623d7"]
#[serde(from = "PrefabShadow")]
pub struct Prefab {
    #[serde(skip_serializing, skip_deserializing)]
    pub scene: Scene,
    pub data: SceneData,
}

#[derive(Deserialize)]
pub struct PrefabShadow {
    pub data: SceneData,
}

impl From<PrefabShadow> for Prefab {
    fn from(value: PrefabShadow) -> Self {
        Self {
            data: value.data.clone(),
            scene: value.data.into(),
        }
    }
}

impl Asset for Prefab {
    fn get_file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxprefab", "fbx", "dae"]
    }

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        let ext = path.extension().and_then(|ext| ext.to_str()).unwrap();
        let registry = AssetRegistry::get();
        let meta = registry.asset_meta_from_path(path).unwrap();
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
                let mesh_ref = registry
                    .create(name, Mesh::from_russimp_mesh(mesh))
                    .unwrap();
                meshes.push((
                    mesh_ref.clone(),
                    registry.asset_id_from_ref_t(&mesh_ref).unwrap(),
                ));
                bones.extend(mesh.bones.iter().enumerate().map(|(i, b)| {
                    let offset_matrix = math::mat4_from_russimp(&b.offset_matrix);
                    (b.name.clone(), (i, offset_matrix))
                }));
            }

            let mut data: SceneData = Default::default();
            if let Some(root) = &scene.root {
                Self::traverse(&bones, &meshes, &**root, &**root, None, &mut data);
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
                let anim_ref = registry
                    .create(name.clone(), Animation::from_russimp_animation(anim))
                    .unwrap();
                animations.push(registry.asset_id_from_ref_t(&anim_ref).unwrap());
            }

            Ok(LoadedAsset {
                asset: Self {
                    data: data.clone(),
                    scene: data.into(),
                },
                sub_assets: meshes
                    .into_iter()
                    .map(|(_, id)| id)
                    .chain(animations.into_iter())
                    .collect(),
            })
        } else {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(|_| AssetError::LoadError)?;
            let reader = BufReader::new(file);
            Ok(LoadedAsset::new(
                serde_json::from_reader(reader).map_err(|_| AssetError::LoadError)?,
            ))
        }
    }
}

impl Prefab {
    fn traverse(
        bones: &HashMap<String, (usize, Mat4)>,
        meshes: &Vec<(Ref<Mesh>, Uuid)>,
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
        if node.meshes.len() > 0 && node.meshes[0] < meshes.len() as u32 {
            let (mesh_ref, mesh_id) = meshes[node.meshes[0] as usize].clone();
            let material_id = AssetRegistry::get().asset_id("materials/default").unwrap();
            let mesh = mesh_ref.read();
            if mesh.bones.len() > 0 {
                entry.insert(
                    ComponentSkinnedMesh::type_uuid(),
                    json!({
                        "material": material_id.to_string(),
                        "mesh": mesh_id.to_string(),
                        "root_bone": utils::uuid_from_str(root.name.as_str())
                    }),
                );
            } else {
                entry.insert(
                    ComponentMesh::type_uuid(),
                    json!({
                        "material": material_id.to_string(),
                        "mesh": mesh_id.to_string()
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
            Self::traverse(bones, meshes, root, &*child.borrow(), parent, data);
        }
    }
}
