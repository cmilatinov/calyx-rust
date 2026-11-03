//! Converting a Unity scene (`.unity`) to a Calyx scene: its hierarchy of
//! groups and prefab instances, with names, transforms, meshes and
//! materials. Unity and Calyx are both left-handed, Y-up and +Z forward, so
//! transforms carry over unchanged.
//!
//! Cameras and lights are collected rather than converted, so the pack's
//! config decides how a scene is lit and framed in Calyx.

use std::collections::HashMap;

use indexmap::IndexMap;
use uuid::Uuid;

use crate::json::Json;
use crate::package::UnityPackage;
use crate::prefab::{self, quat, vec3};
use crate::yaml::{Document, Value};

pub const C_ID: &str = "02289c92-3412-406e-a7e5-3bbb15d7041e";
pub const C_TRANSFORM: &str = "c5b3b71f-1f14-4b5b-9881-436118684d29";
pub const C_MESH: &str = "93fd32b1-7127-4c88-8e89-893512af58de";

/// Builds a Calyx scene file's objects, hierarchy and roots in order.
pub struct SceneWriter {
    namespace: Uuid,
    components: Vec<(String, Json)>,
    hierarchy: IndexMap<String, Vec<String>>,
    roots: Vec<String>,
}

impl SceneWriter {
    /// Object ids derive from `namespace` and each object's key, so a
    /// regenerated scene keeps them.
    pub fn new(namespace: Uuid) -> Self {
        Self {
            namespace,
            components: Vec::new(),
            hierarchy: IndexMap::new(),
            roots: Vec::new(),
        }
    }

    /// Adds an object under `parent` (or at the root) and returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        key: &str,
        name: &str,
        parent: Option<&str>,
        position: Json,
        rotation: Json,
        scale: Json,
        components: Vec<(String, Json)>,
        visible: bool,
    ) -> String {
        let id =
            crate::ids::uuid_v5(&self.namespace, format!("unity:{key}").as_bytes()).to_string();
        let mut data = vec![
            (
                C_ID.to_string(),
                Json::object([
                    ("id", Json::Str(id.clone())),
                    ("name", Json::Str(name.to_string())),
                    ("visible", Json::Bool(visible)),
                ]),
            ),
            (
                C_TRANSFORM.to_string(),
                Json::object([(
                    "transform",
                    Json::object([
                        ("position", position),
                        ("rotation", rotation),
                        ("scale", scale),
                    ]),
                )]),
            ),
        ];
        data.extend(components);
        self.components.push((id.clone(), Json::Object(data)));
        match parent {
            Some(parent) => self
                .hierarchy
                .entry(parent.to_string())
                .or_default()
                .push(id.clone()),
            None => self.roots.push(id.clone()),
        }
        id
    }

    /// The scene file's contents.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("components", Json::Object(self.components.clone())),
            (
                "hierarchy",
                Json::Object(
                    self.hierarchy
                        .iter()
                        .map(|(parent, children)| {
                            (
                                parent.clone(),
                                Json::Array(children.iter().cloned().map(Json::Str).collect()),
                            )
                        })
                        .collect(),
                ),
            ),
            (
                "roots",
                Json::Array(self.roots.iter().cloned().map(Json::Str).collect()),
            ),
        ])
    }
}

/// A Unity camera the scene holds, by its object's name.
#[derive(Clone, Debug)]
pub struct UnityCamera {
    pub name: String,
    pub position: [Json; 3],
    pub rotation: [Json; 4],
    pub camera: Value,
}

/// A Unity light the scene holds.
#[derive(Clone, Debug)]
pub struct UnityLight {
    pub name: String,
    pub position: [Json; 3],
    pub rotation: [Json; 4],
    pub light: Value,
}

/// What a converted scene holds besides its objects.
#[derive(Debug, Default)]
pub struct Converted {
    pub cameras: Vec<UnityCamera>,
    pub lights: Vec<UnityLight>,
    pub prefab_instances: usize,
    pub meshes: usize,
    pub groups: usize,
    /// The scene's `RenderSettings` (fog, ambient), if it has them.
    pub render_settings: Option<Value>,
}

/// How mesh and material GUIDs map to Calyx asset ids.
pub struct AssetIds<'a> {
    pub meshes: &'a HashMap<String, String>,
    pub materials: &'a HashMap<String, String>,
    /// The material a model file's own renderer uses when nothing overrides it.
    pub default_material: &'a str,
}

/// Converts the Unity scene `scene_guid` into `writer`.
pub fn convert(
    package: &UnityPackage,
    scene_guid: &str,
    ids: &AssetIds,
    writer: &mut SceneWriter,
) -> Result<Converted, String> {
    let documents = package.documents(scene_guid)?;
    let mut converter = Converter {
        package,
        ids,
        by_id: documents.iter().map(|d| (d.file_id, d)).collect(),
        instance_of_stripped: IndexMap::new(),
        children_of: IndexMap::new(),
        prefabs: HashMap::new(),
        converted: Converted::default(),
    };
    converter.index(&documents);
    converter.converted.render_settings = documents
        .iter()
        .find(|d| d.class_name == "RenderSettings")
        .map(|d| d.body.clone());
    let roots = documents
        .iter()
        .find(|d| d.class_name == "SceneRoots")
        .ok_or("the scene has no SceneRoots")?
        .body
        .get("m_Roots")
        .map(|r| r.items().to_vec())
        .unwrap_or_default();
    for root in roots {
        let id = root.i64_at("fileID").unwrap_or(0);
        match converter.instance_of_stripped.get(&id).copied() {
            Some(instance) => converter.emit_instance(instance, None, writer)?,
            None => converter.emit_transform(id, None, writer)?,
        }
    }
    Ok(converter.converted)
}

#[derive(Clone, Copy)]
enum Child {
    Instance(i64),
    Transform(i64),
}

struct Converter<'a> {
    package: &'a UnityPackage,
    ids: &'a AssetIds<'a>,
    by_id: IndexMap<i64, &'a Document>,
    /// The prefab instance each stripped transform stands for.
    instance_of_stripped: IndexMap<i64, i64>,
    children_of: IndexMap<i64, Vec<Child>>,
    prefabs: HashMap<String, prefab::Placed>,
    converted: Converted,
}

fn file_id(value: Option<&Value>) -> i64 {
    value.and_then(|v| v.i64_at("fileID")).unwrap_or(0)
}

impl<'a> Converter<'a> {
    fn index(&mut self, documents: &'a [Document]) {
        for d in documents {
            if d.class_name == "Transform" && d.stripped {
                self.instance_of_stripped
                    .insert(d.file_id, file_id(d.body.get("m_PrefabInstance")));
            }
        }
        for d in documents {
            if d.class_name == "PrefabInstance" {
                let parent = file_id(
                    d.body
                        .get("m_Modification")
                        .and_then(|m| m.get("m_TransformParent")),
                );
                self.children_of
                    .entry(parent)
                    .or_default()
                    .push(Child::Instance(d.file_id));
            }
        }
        for d in documents {
            if d.class_name == "Transform" && !d.stripped {
                let father = file_id(d.body.get("m_Father"));
                if self.instance_of_stripped.contains_key(&father) {
                    self.children_of
                        .entry(father)
                        .or_default()
                        .push(Child::Transform(d.file_id));
                }
                for child in d.body.get("m_Children").map(Value::items).unwrap_or(&[]) {
                    let child = child.i64_at("fileID").unwrap_or(0);
                    if self.instance_of_stripped.contains_key(&child) {
                        continue; // listed through its PrefabInstance
                    }
                    self.children_of
                        .entry(d.file_id)
                        .or_default()
                        .push(Child::Transform(child));
                }
            }
        }
    }

    fn emit_child(
        &mut self,
        child: Child,
        parent: &str,
        writer: &mut SceneWriter,
    ) -> Result<(), String> {
        match child {
            Child::Instance(id) => self.emit_instance(id, Some(parent), writer),
            Child::Transform(id) => self.emit_transform(id, Some(parent), writer),
        }
    }

    fn emit_instance(
        &mut self,
        instance_id: i64,
        parent: Option<&str>,
        writer: &mut SceneWriter,
    ) -> Result<(), String> {
        let body = &self.by_id[&instance_id].body;
        let source = body
            .get("m_SourcePrefab")
            .and_then(|s| s.str_at("guid"))
            .ok_or_else(|| format!("prefab instance {instance_id} has no source"))?;
        let mut placed = prefab::resolve(self.package, source, &mut self.prefabs)?;
        placed.apply(prefab::modifications(body));
        let mut components = Vec::new();
        if let Some(mesh) = &placed.mesh {
            let material = placed
                .material
                .as_deref()
                .unwrap_or(self.ids.default_material);
            components.push((
                C_MESH.to_string(),
                Json::object([
                    ("cast_shadows", Json::Bool(placed.shadows)),
                    (
                        "material",
                        self.ids
                            .materials
                            .get(material)
                            .cloned()
                            .map_or(Json::Null, Json::Str),
                    ),
                    (
                        "mesh",
                        Json::Str(
                            self.ids
                                .meshes
                                .get(mesh)
                                .cloned()
                                .ok_or_else(|| format!("no model {mesh} for {}", placed.name))?,
                        ),
                    ),
                ]),
            ));
            self.converted.meshes += 1;
        }
        self.converted.prefab_instances += 1;
        let id = writer.add(
            &instance_id.to_string(),
            &placed.name,
            parent,
            Json::Array(placed.position.to_vec()),
            Json::Array(placed.rotation.to_vec()),
            Json::Array(placed.scale.to_vec()),
            components,
            placed.active,
        );
        let stripped: Vec<i64> = self
            .instance_of_stripped
            .iter()
            .filter(|(_, instance)| **instance == instance_id)
            .map(|(stripped, _)| *stripped)
            .collect();
        for stripped in stripped {
            for child in self.children_of.get(&stripped).cloned().unwrap_or_default() {
                self.emit_child(child, &id, writer)?;
            }
        }
        Ok(())
    }

    fn emit_transform(
        &mut self,
        transform_id: i64,
        parent: Option<&str>,
        writer: &mut SceneWriter,
    ) -> Result<(), String> {
        let body = &self
            .by_id
            .get(&transform_id)
            .ok_or_else(|| format!("no transform {transform_id}"))?
            .body;
        let game_object = &self
            .by_id
            .get(&file_id(body.get("m_GameObject")))
            .ok_or_else(|| format!("transform {transform_id} has no game object"))?
            .body;
        let name = game_object.str_at("m_Name").unwrap_or("").to_string();
        let position = vec3(body.get("m_LocalPosition"));
        let rotation = quat(body.get("m_LocalRotation"));
        let scale = vec3(body.get("m_LocalScale"));
        let active = game_object.str_at("m_IsActive") != Some("0");
        let mut kinds: IndexMap<&str, &Value> = IndexMap::new();
        for component in game_object
            .get("m_Component")
            .map(Value::items)
            .unwrap_or(&[])
        {
            if let Some(document) = self.by_id.get(&file_id(component.get("component"))) {
                kinds.insert(document.class_name.as_str(), &document.body);
            }
        }
        if let Some(camera) = kinds.get("Camera") {
            self.converted.cameras.push(UnityCamera {
                name,
                position,
                rotation,
                camera: (*camera).clone(),
            });
            return Ok(());
        }
        if let Some(light) = kinds.get("Light") {
            self.converted.lights.push(UnityLight {
                name,
                position,
                rotation,
                light: (*light).clone(),
            });
            return Ok(());
        }
        self.converted.groups += 1;
        let id = writer.add(
            &transform_id.to_string(),
            &name,
            parent,
            Json::Array(position.to_vec()),
            Json::Array(rotation.to_vec()),
            Json::Array(scale.to_vec()),
            Vec::new(),
            active,
        );
        for child in self
            .children_of
            .get(&transform_id)
            .cloned()
            .unwrap_or_default()
        {
            self.emit_child(child, &id, writer)?;
        }
        Ok(())
    }
}
