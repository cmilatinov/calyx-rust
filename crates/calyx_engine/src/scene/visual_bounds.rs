//! Geometry-derived bounds for screen-space annotations.
use super::{GameObject, Scene};
use crate::component::ComponentMesh;
use nalgebra_glm::Vec3;
use std::collections::HashMap;
impl Scene {
    /// World-space corners of the visible static meshes under an object.
    /// Each mesh contributes its own transformed local box, preserving gaps
    /// and rotations when callers project the corners into a camera view.
    /// Shared mesh vertices are scanned once per query. Bounds are not cached
    /// across queries, so in-place mesh edits and asset reloads take effect.
    pub fn visual_bound_corners(&self, root: GameObject) -> Vec<Vec3> {
        let mut result = Vec::new();
        let mut pending = vec![root];
        let mut bounds = HashMap::new();
        while let Some(object) = pending.pop() {
            if !self.is_visible_in_hierarchy(object) {
                continue;
            }
            pending.extend(self.children(object));
            if !self.is_component_enabled::<ComponentMesh>(object) {
                continue;
            }
            let asset = self
                .read_component::<ComponentMesh, _, _>(object, |c| {
                    c.mesh.get_ref(self.registries())
                })
                .flatten();
            let Some(asset) = asset else {
                continue;
            };
            let Some((min, max)) = *bounds.entry(asset.id()).or_insert_with(|| {
                let mesh = asset.read();
                let mut vertices = mesh
                    .vertices
                    .iter()
                    .copied()
                    .filter(|point| point.iter().all(|coordinate| coordinate.is_finite()));
                let first = vertices.next()?;
                Some(vertices.fold((first, first), |(min, max), point| {
                    (min.inf(&point), max.sup(&point))
                }))
            }) else {
                continue;
            };
            let transform = self.world_transform(object);
            for x in [min.x, max.x] {
                for y in [min.y, max.y] {
                    for z in [min.z, max.z] {
                        result.push(transform.transform_position(&Vec3::new(x, y, z)));
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{mesh::Mesh, AssetRef};
    use crate::component::ComponentID;
    use crate::core::Ref;
    use crate::math::Transform;
    use crate::test_support::test_asset_context_with_assets;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::vec3;

    fn fixture() -> (Scene, Ref<Mesh>) {
        let assets = test_asset_context_with_assets(Vec::new());
        let mut mesh = Mesh::new(&assets.render_context);
        mesh.vertices = vec![vec3(-1.0, -2.0, -3.0), vec3(1.0, 2.0, 3.0)];
        let mesh = assets
            .registries
            .assets
            .read()
            .create("visual-bounds-box".into(), mesh)
            .unwrap();
        (assets.registries.lock_read().scene(), mesh)
    }

    fn add_mesh(scene: &mut Scene, object: GameObject, mesh: &Ref<Mesh>) {
        scene.add_component(
            object,
            ComponentMesh {
                mesh: AssetRef::from_id(mesh.id()),
                ..Default::default()
            },
        );
    }

    fn assert_extents(points: &[Vec3], expected_min: Vec3, expected_max: Vec3) {
        let min = points
            .iter()
            .fold(Vec3::repeat(f32::INFINITY), |a, b| a.inf(b));
        let max = points
            .iter()
            .fold(Vec3::repeat(f32::NEG_INFINITY), |a, b| a.sup(b));
        assert!((min - expected_min).norm() < 1e-4, "min={min:?}");
        assert!((max - expected_max).norm() < 1e-4, "max={max:?}");
    }

    #[test]
    fn visual_bounds_follow_nested_rotation_scale_and_live_mesh_edits() {
        let (mut scene, mesh) = fixture();
        let root = scene.create(None, None);
        let child = scene.create(None, Some(root));
        scene.set_transform(
            root,
            &Transform::from_components(
                vec3(10.0, 3.0, -5.0),
                UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0),
                vec3(2.0, 3.0, 4.0),
            )
            .matrix(),
        );
        scene.set_transform(child, &Transform::from_xyz(2.0, 1.0, 0.0).matrix());
        add_mesh(&mut scene, child, &mesh);
        let corners = scene.visual_bound_corners(root);
        assert_eq!(corners.len(), 8);
        assert_extents(&corners, vec3(-2.0, 0.0, -11.0), vec3(22.0, 12.0, -7.0));
        mesh.write().vertices[1].y = 4.0;
        assert_extents(
            &scene.visual_bound_corners(root),
            vec3(-2.0, 0.0, -11.0),
            vec3(22.0, 18.0, -7.0),
        );
    }

    #[test]
    fn visual_bounds_skip_disabled_meshes_but_keep_their_children() {
        let (mut scene, mesh) = fixture();
        let root = scene.create(None, None);
        let child = scene.create(None, Some(root));
        let leaf = scene.create(None, Some(child));
        for object in [root, child, leaf] {
            add_mesh(&mut scene, object, &mesh);
        }
        assert_eq!(scene.visual_bound_corners(root).len(), 24);
        assert!(scene.set_component_enabled::<ComponentMesh>(child, false));
        assert_eq!(scene.visual_bound_corners(root).len(), 16);
        scene.write_component::<ComponentID, _>(child, |id| id.visible = false);
        assert_eq!(scene.visual_bound_corners(root).len(), 8);
        assert!(
            scene.visual_bound_corners(leaf).is_empty(),
            "hidden ancestor hides queried leaf"
        );
        scene.write_component::<ComponentID, _>(root, |id| id.visible = false);
        assert!(scene.visual_bound_corners(root).is_empty());
    }

    #[test]
    fn visual_bounds_ignore_missing_empty_and_nonfinite_mesh_geometry() {
        let (mut scene, mesh) = fixture();
        let root = scene.create(None, None);
        scene.add_component(root, ComponentMesh::default());
        assert!(scene.visual_bound_corners(root).is_empty());
        add_mesh(&mut scene, root, &mesh);
        mesh.write().vertices.clear();
        assert!(scene.visual_bound_corners(root).is_empty());
        mesh.write().vertices.push(Vec3::repeat(f32::NAN));
        assert!(scene.visual_bound_corners(root).is_empty());
        mesh.write().vertices.push(vec3(1.0, 2.0, 3.0));
        assert_eq!(
            scene.visual_bound_corners(root),
            vec![vec3(1.0, 2.0, 3.0); 8]
        );
    }
}
