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
mod tests;
