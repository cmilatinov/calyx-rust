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
mod tests;
