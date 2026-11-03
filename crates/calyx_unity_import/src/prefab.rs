//! Resolving prefabs, prefab variants and model prefabs to the object they
//! place, with prefab instance modifications applied.

use std::collections::HashMap;

use crate::json::Json;
use crate::package::{stem, UnityPackage};
use crate::yaml::Value;

/// A single-object prefab as an instance places it: its name, mesh and
/// material GUIDs, local transform, and flags. Transform components are kept
/// as the file gives them, integers where a default fills them in.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub name: String,
    /// The GUID of the model file whose mesh the object draws.
    pub mesh: Option<String>,
    /// The GUID of its material, if the prefab names one.
    pub material: Option<String>,
    pub position: [Json; 3],
    pub rotation: [Json; 4],
    pub scale: [Json; 3],
    pub active: bool,
    pub shadows: bool,
}

/// A Unity `{x, y, z}` mapping as floats.
pub fn vec3(value: Option<&Value>) -> [Json; 3] {
    ["x", "y", "z"].map(|axis| Json::Float(value.and_then(|v| v.f64_at(axis)).unwrap_or(0.0)))
}

/// A Unity `{x, y, z, w}` quaternion as floats.
pub fn quat(value: Option<&Value>) -> [Json; 4] {
    ["x", "y", "z", "w"].map(|axis| Json::Float(value.and_then(|v| v.f64_at(axis)).unwrap_or(0.0)))
}

impl Placed {
    /// Applies a prefab instance's modifications. The prefab holds one
    /// object, so every transform modification targets its root.
    pub fn apply(&mut self, modifications: &[Value]) {
        for modification in modifications {
            let property = modification.str_at("propertyPath").unwrap_or("");
            let value = modification.str_at("value");
            let (field, axis) = property.split_once('.').unwrap_or((property, ""));
            let number = || Json::Float(value.and_then(|v| v.trim().parse().ok()).unwrap_or(0.0));
            let index = "xyzw".find(axis).filter(|_| axis.len() == 1);
            match (field, index) {
                ("m_LocalPosition", Some(i)) if i < 3 => self.position[i] = number(),
                ("m_LocalRotation", Some(i)) => self.rotation[i] = number(),
                ("m_LocalScale", Some(i)) if i < 3 => self.scale[i] = number(),
                _ => match property {
                    "m_Name" => self.name = value.unwrap_or("").to_string(),
                    "m_IsActive" => self.active = value != Some("0"),
                    "m_CastShadows" => self.shadows = value != Some("0"),
                    "m_Materials.Array.data[0]" => {
                        self.material = modification
                            .get("objectReference")
                            .and_then(|r| r.str_at("guid"))
                            .map(str::to_string)
                    }
                    _ => {}
                },
            }
        }
    }
}

/// The modifications of a `PrefabInstance` body.
pub fn modifications(instance: &Value) -> &[Value] {
    instance
        .get("m_Modification")
        .and_then(|m| m.get("m_Modifications"))
        .map(Value::items)
        .unwrap_or(&[])
}

/// Resolves what the prefab, prefab variant or model file (FBX) `guid`
/// places, caching each prefab.
pub fn resolve(
    package: &UnityPackage,
    guid: &str,
    cache: &mut HashMap<String, Placed>,
) -> Result<Placed, String> {
    if let Some(placed) = cache.get(guid) {
        return Ok(placed.clone());
    }
    let path = package.path(guid)?;
    let placed = if path.to_ascii_lowercase().ends_with(".fbx") {
        Placed {
            name: stem(path).to_string(),
            mesh: Some(guid.to_string()),
            material: None,
            position: [Json::Int(0), Json::Int(0), Json::Int(0)],
            rotation: [Json::Int(0), Json::Int(0), Json::Int(0), Json::Int(1)],
            scale: [Json::Int(1), Json::Int(1), Json::Int(1)],
            active: true,
            shadows: true,
        }
    } else {
        let documents = package.documents(guid)?;
        let body = |class: &str| {
            documents
                .iter()
                .rev()
                .find(|d| d.class_name == class)
                .map(|d| &d.body)
        };
        if let Some(instance) = body("PrefabInstance") {
            let source = instance
                .get("m_SourcePrefab")
                .and_then(|s| s.str_at("guid"))
                .ok_or_else(|| format!("{path}: prefab instance without a source"))?;
            let mut placed = resolve(package, source, cache)?;
            placed.apply(modifications(instance));
            placed
        } else {
            let game_object = body("GameObject").ok_or_else(|| format!("{path}: no GameObject"))?;
            let transform = body("Transform").ok_or_else(|| format!("{path}: no Transform"))?;
            let renderer = body("MeshRenderer");
            let material = renderer
                .and_then(|r| r.get("m_Materials"))
                .and_then(|m| m.items().first())
                .and_then(|m| m.str_at("guid"))
                .map(str::to_string);
            Placed {
                name: game_object.str_at("m_Name").unwrap_or("").to_string(),
                mesh: body("MeshFilter")
                    .and_then(|f| f.get("m_Mesh"))
                    .and_then(|m| m.str_at("guid"))
                    .map(str::to_string),
                material,
                position: vec3(transform.get("m_LocalPosition")),
                rotation: quat(transform.get("m_LocalRotation")),
                scale: vec3(transform.get("m_LocalScale")),
                active: game_object.str_at("m_IsActive") != Some("0"),
                shadows: renderer
                    .and_then(|r| r.str_at("m_CastShadows"))
                    .unwrap_or("1")
                    != "0",
            }
        }
    };
    cache.insert(guid.to_string(), placed.clone());
    Ok(placed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::tests::package_bytes;

    const PREFAB: &str = "--- !u!1 &10
GameObject:
  m_Component:
  - component: {fileID: 40}
  m_Name: crate
  m_IsActive: 1
--- !u!4 &40
Transform:
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0.5, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Father: {fileID: 0}
--- !u!33 &330
MeshFilter:
  m_Mesh: {fileID: 4300000, guid: fbx1, type: 3}
--- !u!23 &230
MeshRenderer:
  m_CastShadows: 1
  m_Materials:
  - {fileID: 2100000, guid: mat1, type: 2}
";

    /// A prefab variant of the model file itself, as Unity writes one: it
    /// zeroes the root transform and sets the material.
    const VARIANT: &str = "--- !u!1001 &7
PrefabInstance:
  m_Modification:
    m_TransformParent: {fileID: 0}
    m_Modifications:
    - target: {fileID: -8679921383154817045, guid: fbx2, type: 3}
      propertyPath: m_LocalPosition.x
      value: 0
      objectReference: {fileID: 0}
    - target: {fileID: -7511558181221131132, guid: fbx2, type: 3}
      propertyPath: 'm_Materials.Array.data[0]'
      value:
      objectReference: {fileID: 2100000, guid: mat1, type: 2}
    - target: {fileID: 919132149155446097, guid: fbx2, type: 3}
      propertyPath: m_Name
      value: barrel
      objectReference: {fileID: 0}
  m_SourcePrefab: {fileID: 100100000, guid: fbx2, type: 3}
";

    fn package() -> UnityPackage {
        let bytes = package_bytes(&[
            ("fbx1", "Assets/P/Models/crate.fbx", Some("fbx"), None),
            ("fbx2", "Assets/P/Models/barrel_raw.fbx", Some("fbx"), None),
            ("pre1", "Assets/P/Prefabs/crate.prefab", Some(PREFAB), None),
            (
                "var1",
                "Assets/P/Prefabs/barrel.prefab",
                Some(VARIANT),
                None,
            ),
        ]);
        UnityPackage::from_reader(bytes.as_slice()).unwrap()
    }

    fn modification(path: &str, value: &str) -> Value {
        crate::yaml::parse_documents(&format!(
            "--- !u!1 &1\nX:\n  list:\n  - propertyPath: {path}\n    value: {value}\n"
        ))[0]
            .body
            .get("list")
            .unwrap()
            .items()[0]
            .clone()
    }

    #[test]
    fn a_prefab_places_its_mesh_material_and_transform() {
        let placed = resolve(&package(), "pre1", &mut HashMap::new()).unwrap();
        assert_eq!(placed.name, "crate");
        assert_eq!(placed.mesh.as_deref(), Some("fbx1"));
        assert_eq!(placed.material.as_deref(), Some("mat1"));
        assert_eq!(placed.position[1], Json::Float(0.5));
        assert!(placed.active && placed.shadows);
    }

    #[test]
    fn a_variant_applies_its_modifications_over_the_model_it_derives_from() {
        let placed = resolve(&package(), "var1", &mut HashMap::new()).unwrap();
        assert_eq!(placed.name, "barrel");
        assert_eq!(placed.mesh.as_deref(), Some("fbx2"));
        assert_eq!(placed.material.as_deref(), Some("mat1"));
        // Set by the variant: a float; left as the model's default: integers.
        assert_eq!(placed.position[0], Json::Float(0.0));
        assert_eq!(placed.position[1], Json::Int(0));
        assert_eq!(placed.scale, [Json::Int(1), Json::Int(1), Json::Int(1)]);
    }

    #[test]
    fn a_scene_instance_moves_renames_hides_and_unshadows_its_copy() {
        let mut cache = HashMap::new();
        let mut placed = resolve(&package(), "pre1", &mut cache).unwrap();
        placed.apply(&[
            modification("m_LocalPosition.z", "12.5"),
            modification("m_LocalRotation.w", "0.5"),
            modification("m_LocalScale.y", "2"),
            modification("m_Name", "crate (3)"),
            modification("m_IsActive", "0"),
            modification("m_CastShadows", "0"),
            modification("m_LocalEulerAnglesHint.y", "90"),
        ]);
        assert_eq!(placed.position[2], Json::Float(12.5));
        assert_eq!(placed.rotation[3], Json::Float(0.5));
        assert_eq!(placed.scale[1], Json::Float(2.0));
        assert_eq!(placed.name, "crate (3)");
        assert!(!placed.active && !placed.shadows);
        // The cached prefab is untouched.
        assert_eq!(cache["pre1"].name, "crate");
    }
}
