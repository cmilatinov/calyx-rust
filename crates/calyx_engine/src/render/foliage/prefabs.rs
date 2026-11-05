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
mod tests {
    use super::*;
    use crate::math::Transform;
    use crate::test_support::test_scene;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec3, Vec3};

    fn fixture() -> (Ref<Prefab>, Uuid, Uuid, Mat4) {
        let mut scene = test_scene();
        let root = scene.create(None, None);
        let child = scene.create(None, Some(root));
        let leaf = scene.create(None, Some(child));
        let root_matrix = Transform::from_components(
            vec3(3.0, 2.0, -4.0),
            UnitQuaternion::from_axis_angle(&Vec3::y_axis(), 0.5),
            vec3(2.0, 1.0, 3.0),
        )
        .matrix();
        let child_matrix = Transform::from_components(
            vec3(1.0, 0.5, 2.0),
            UnitQuaternion::from_axis_angle(&Vec3::x_axis(), 0.3),
            Vec3::repeat(0.5),
        )
        .matrix();
        scene.set_transform(root, &root_matrix);
        scene.set_transform(child, &child_matrix);
        for object in [root, child, leaf] {
            scene.add_component(
                object,
                ComponentMesh {
                    mesh: AssetRef::from_id(Uuid::new_v4()),
                    material: AssetRef::from_id(Uuid::new_v4()),
                    cast_shadows: object != child,
                },
            );
        }
        let child_id = scene.uuid(child);
        let leaf_id = scene.uuid(leaf);
        (
            Ref::from_id_value(Uuid::new_v4(), scene.create_prefab(root)),
            child_id,
            leaf_id,
            root_matrix * child_matrix,
        )
    }

    #[test]
    fn foliage_prefab_keeps_all_static_parts_and_root_nested_transforms() {
        let (source, child, _, expected) = fixture();
        let mut cache = PrefabParts::default();
        cache.begin_frame();
        let parts = cache.parts(&source);
        assert_eq!(parts.len(), 3);
        assert!((parts[1].transform - expected).norm() < 0.0001);
        assert!((parts[2].transform - expected).norm() < 0.0001);
        assert!(parts[0].cast_shadows && !parts[1].cast_shadows && parts[2].cast_shadows);
        let mesh: ComponentMesh = serde_json::from_value(
            source.read().data.components[&child][&ComponentMesh::type_uuid()].clone(),
        )
        .unwrap();
        assert_eq!(parts[1].mesh, mesh.mesh);
        assert_eq!(parts[1].material, mesh.material);
        cache.end_frame();
        cache.begin_frame();
        assert!(
            Arc::ptr_eq(&parts, &cache.parts(&source)),
            "unchanged visual data reuses extraction"
        );
    }

    #[test]
    fn foliage_prefab_cache_follows_in_place_edits_and_replacement_reloads() {
        let (source, child, _, _) = fixture();
        let mut cache = PrefabParts::default();
        cache.begin_frame();
        let first = cache.parts(&source);
        source
            .write()
            .data
            .components
            .get_mut(&child)
            .unwrap()
            .get_mut(&ComponentMesh::type_uuid())
            .unwrap()["material"] = serde_json::json!(Uuid::new_v4());
        cache.begin_frame();
        let edited = cache.parts(&source);
        assert!(!Arc::ptr_eq(&first, &edited));
        assert_ne!(first[1].material, edited[1].material);
        source
            .write()
            .data
            .disabled_components
            .entry(child)
            .or_default()
            .insert(ComponentMesh::type_uuid());
        cache.begin_frame();
        assert_eq!(
            cache.parts(&source).len(),
            2,
            "disabling one mesh keeps its children"
        );
        source
            .write()
            .data
            .components
            .get_mut(&child)
            .unwrap()
            .get_mut(&ComponentID::type_uuid())
            .unwrap()["visible"] = serde_json::json!(false);
        cache.begin_frame();
        assert_eq!(
            cache.parts(&source).len(),
            1,
            "a hidden ancestor hides every descendant"
        );
        let prefab = source.read();
        let replacement = Ref::from_id_value(
            source.id(),
            Prefab {
                root: child,
                data: prefab.data.clone(),
            },
        );
        drop(prefab);
        replacement
            .write()
            .data
            .components
            .get_mut(&child)
            .unwrap()
            .get_mut(&ComponentID::type_uuid())
            .unwrap()["visible"] = serde_json::json!(true);
        cache.begin_frame();
        assert_eq!(
            cache.parts(&replacement).len(),
            1,
            "reload with same ID and new root drops old parts"
        );
        cache.end_frame();
        cache.begin_frame();
        cache.end_frame();
        assert!(cache.prefabs.is_empty());
    }

    #[test]
    fn foliage_prefab_ignores_unsupported_components_and_malformed_unreachable_branches() {
        let (source, child, leaf, _) = fixture();
        let mut prefab = source.write();
        let root = prefab.root;
        // Script payloads are never deserialized, and cycles cannot loop forever.
        prefab.data.components.get_mut(&root).unwrap().insert(
            Uuid::new_v4(),
            serde_json::json!({"script":"not executable"}),
        );
        prefab
            .data
            .hierarchy
            .entry(leaf)
            .or_default()
            .extend([root, Uuid::new_v4()]);
        let unreachable = Uuid::new_v4();
        let payload = prefab.data.components[&child].clone();
        prefab.data.components.insert(unreachable, payload);
        let mut cache = PrefabParts::default();
        drop(prefab);
        cache.begin_frame();
        assert_eq!(cache.parts(&source).len(), 3);
        source
            .write()
            .data
            .components
            .get_mut(&child)
            .unwrap()
            .insert(
                ComponentTransform::type_uuid(),
                serde_json::json!({"transform":"bad"}),
            );
        cache.begin_frame();
        assert_eq!(
            cache.parts(&source).len(),
            1,
            "invalid parent transforms prune the subtree"
        );
        source.write().root = Uuid::new_v4();
        cache.begin_frame();
        assert!(
            cache.parts(&source).is_empty(),
            "missing root does not render detached data"
        );
    }
}
