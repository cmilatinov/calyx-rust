//! An assembly is a prefab per map, with one mesh/material per named part.
//! Keeping the transforms on objects lets tools inspect and edit each part.

use serde_json::{json, Map, Value};

use crate::ids;
use crate::layout::{Layout, PrefabPlan, NAMESPACE};
use crate::palette;

const ID: &str = "02289c92-3412-406e-a7e5-3bbb15d7041e";
const TRANSFORM: &str = "c5b3b71f-1f14-4b5b-9881-436118684d29";
const MESH: &str = "93fd32b1-7127-4c88-8e89-893512af58de";

fn rotation(degrees: [f64; 3]) -> [f64; 4] {
    let [(sx, cx), (sy, cy), (sz, cz)] = degrees.map(|v| (v.to_radians() * 0.5).sin_cos());
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

impl PrefabPlan {
    pub fn data(&self, layout: &Layout) -> Value {
        let object_id = |name: &str| {
            ids::uuid_v5(
                &NAMESPACE,
                format!("{}/objects/{name}", self.asset.name).as_bytes(),
            )
            .to_string()
        };
        let root = object_id("root");
        let object = |id: &str, name: &str, position: [f64; 3], angles: [f64; 3]| {
            json!({
                ID: {"id": id, "name": name, "visible": true},
                TRANSFORM: {"transform": {"position": position, "rotation": rotation(angles), "scale": [1.0, 1.0, 1.0]}}
            })
        };
        let mut components = Map::new();
        components.insert(root.clone(), object(&root, &self.name, [0.0; 3], [0.0; 3]));
        let mut hierarchy = Map::new();
        hierarchy.insert(root.clone(), json!([]));
        for entry in &self.parts {
            let crate::layout::PrefabPart {
                part,
                mesh,
                material,
                outline,
            } = entry;
            let id = object_id(&format!("parts/{}", part.name));
            let mut value = object(&id, &part.name, part.offset, part.rotation);
            value[MESH] = json!({"mesh": mesh.id, "material": material.id});
            if !part.cast_shadows {
                value[MESH]["cast_shadows"] = json!(false);
            }
            components.insert(id.clone(), value);
            let parent = part.parent.as_ref().map_or_else(
                || root.clone(),
                |parent| object_id(&format!("parts/{parent}")),
            );
            hierarchy
                .entry(parent.clone())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("child list")
                .push(json!(id));
            if let Some(outline) = outline {
                let id = object_id(&format!("parts/{}/outline", part.name));
                let mut value = object(
                    &id,
                    &format!("{} outline", part.name),
                    part.offset,
                    part.rotation,
                );
                let ink = layout.material(self.map, palette::find(palette::INK).expect("ink role"));
                value[MESH] = json!({"mesh": outline.id, "material": ink.asset.id});
                if !part.cast_shadows {
                    value[MESH]["cast_shadows"] = json!(false);
                }
                components.insert(id.clone(), value);
                hierarchy[&parent]
                    .as_array_mut()
                    .expect("child list")
                    .push(json!(id));
            }
        }
        json!({"root": root, "data": {"components": components, "hierarchy": hierarchy, "roots": [root]}})
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rotations_follow_the_editors_fixed_xyz_euler_convention() {
        let actual = super::rotation([90.0, 90.0, 0.0]);
        for (actual, expected) in actual.into_iter().zip([0.5, 0.5, -0.5, 0.5]) {
            assert!((actual - expected).abs() < 1e-12);
        }
    }
}
