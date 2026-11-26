//! Extract only static visual data: never instantiate a prefab into a scene or
//! deserialize its scripts/physics. Content fingerprints detect both in-place
//! edits and reloads; reference identity alone cannot detect an in-place edit.
use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::AssetRef;
use crate::component::{ComponentID, ComponentMesh, ComponentTransform};
use crate::core::Ref;
use crate::scene::Prefab;
use crate::utils::TypeUuid;
use nalgebra_glm::Mat4;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use uuid::Uuid;

pub(in crate::render) struct FoliagePart {
    pub mesh: AssetRef<Mesh>,
    pub material: AssetRef<Material>,
    pub transform: Mat4,
    pub cast_shadows: bool,
}

struct CachedPrefab {
    fingerprint: u64,
    parts: Arc<[FoliagePart]>,
    seen: bool,
}

#[derive(Default)]
pub(super) struct PrefabParts {
    prefabs: HashMap<Uuid, CachedPrefab>,
}

impl PrefabParts {
    pub fn begin_frame(&mut self) {
        for prefab in self.prefabs.values_mut() {
            prefab.seen = false;
        }
    }

    pub fn parts(&mut self, source: &Ref<Prefab>) -> Arc<[FoliagePart]> {
        // A prefab used by several patches is checked only once per frame.
        if let Some(cached) = self.prefabs.get(&source.id()).filter(|cached| cached.seen) {
            return cached.parts.clone();
        }
        let prefab = source.read();
        let fingerprint = visual_fingerprint(&prefab);
        let cached = self
            .prefabs
            .entry(source.id())
            .or_insert_with(|| CachedPrefab {
                fingerprint,
                parts: extract_parts(&prefab).into(),
                seen: false,
            });
        if cached.fingerprint != fingerprint {
            cached.fingerprint = fingerprint;
            cached.parts = extract_parts(&prefab).into();
        }
        cached.seen = true;
        cached.parts.clone()
    }

    pub fn end_frame(&mut self) {
        self.prefabs.retain(|_, prefab| prefab.seen);
    }
}

fn visual_fingerprint(prefab: &Prefab) -> u64 {
    let mut hash = DefaultHasher::new();
    prefab.root.hash(&mut hash);
    prefab.data.hierarchy.hash(&mut hash);
    for (id, components) in &prefab.data.components {
        id.hash(&mut hash);
        for component in [
            ComponentID::type_uuid(),
            ComponentTransform::type_uuid(),
            ComponentMesh::type_uuid(),
        ] {
            components.get(&component).hash(&mut hash);
        }
        prefab
            .data
            .is_component_enabled(*id, ComponentMesh::type_uuid())
            .hash(&mut hash);
    }
    hash.finish()
}

fn extract_parts(prefab: &Prefab) -> Vec<FoliagePart> {
    let mut parts = Vec::new();
    let mut pending = vec![(prefab.root, Mat4::identity())];
    let mut visited = HashSet::new();
    while let Some((id, parent)) = pending.pop() {
        // Cycles, duplicate edges, dangling children and unreachable objects do
        // not produce extra copies. Hidden/malformed parents prune their branch.
        if !visited.insert(id) {
            continue;
        }
        let Some(components) = prefab.data.components.get(&id) else {
            continue;
        };
        let visible = components
            .get(&ComponentID::type_uuid())
            .and_then(|value| value.get("visible"));
        if visible.is_some_and(|value| value.as_bool() != Some(true)) {
            continue;
        }
        let local = match components.get(&ComponentTransform::type_uuid()) {
            Some(value) => match serde_json::from_value::<ComponentTransform>(value.clone()) {
                Ok(component) => component.transform.matrix(),
                Err(_) => continue,
            },
            None => Mat4::identity(),
        };
        let transform = parent * local;
        if !transform.iter().all(|value| value.is_finite()) {
            continue;
        }
        if prefab
            .data
            .is_component_enabled(id, ComponentMesh::type_uuid())
        {
            if let Some(mesh) = components
                .get(&ComponentMesh::type_uuid())
                .and_then(|value| serde_json::from_value::<ComponentMesh>(value.clone()).ok())
            {
                parts.push(FoliagePart {
                    mesh: mesh.mesh,
                    material: mesh.material,
                    transform,
                    cast_shadows: mesh.cast_shadows,
                });
            }
        }
        if let Some(children) = prefab.data.hierarchy.get(&id) {
            pending.extend(children.iter().rev().map(|child| (*child, transform)));
        }
    }
    parts
}

#[cfg(test)]
mod tests;
