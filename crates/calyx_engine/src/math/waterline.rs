//! Where things standing in water meet its surface: the waterline a mesh
//! cuts through a water's rest plane, and the footprints the water's foam
//! and shallows ring.
use nalgebra_glm::{vec2, Mat4, Vec2, Vec3};
use std::collections::HashMap;

/// The widest a waterline may be, in local units, to count as something
/// standing in the water: past this it is ground or a shore, which the
/// water's banks already draw, not a rock or a post.
pub const MAX_FOOTPRINT_SIZE: f32 = 8.0;
/// Footprints nearer each other than this, in local units, merge into one:
/// a tuft of grass hanging off a rock into the water rings with the rock,
/// not on its own beside it. Each facet of the water knows one footprint,
/// so two this close would split the water between them along a seam.
const MERGE_GAP: f32 = 0.4;
/// Points a circle's outline is drawn through when it merges.
const CIRCLE_POINTS: usize = 32;

/// What something standing in the water covers of its surface, as the foam
/// and shallows see it: a rounded rectangle in the water's local XZ plane,
/// fitted around the outline where it meets the surface. A circle is one
/// whose corners are rounded all the way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterFootprint {
    pub center: Vec2,
    /// Unit direction of the rectangle's first side.
    pub axis: Vec2,
    /// Half the rectangle's extent along `axis`, then across it.
    pub half: Vec2,
    /// The radius its corners are rounded by, at most the smaller of
    /// `half`.
    pub rounding: f32,
}

impl WaterFootprint {
    /// A circle of `radius` about `center`.
    pub fn circle(center: Vec2, radius: f32) -> Self {
        let radius = radius.max(0.0);
        Self {
            center,
            axis: vec2(1.0, 0.0),
            half: Vec2::repeat(radius),
            rounding: radius,
        }
    }

    /// `point` in the footprint's own frame: along `axis` and across it,
    /// from its centre.
    pub fn local(&self, point: Vec2) -> Vec2 {
        let d = point - self.center;
        vec2(d.dot(&self.axis), d.x * -self.axis.y + d.y * self.axis.x)
    }

    /// The signed distance from `point` to the footprint's edge: negative
    /// inside it.
    pub fn distance(&self, point: Vec2) -> f32 {
        rounded_box_distance(self.local(point), self.half, self.rounding)
    }

    /// The rounded rectangle that best fits the outline through `points`:
    /// the smallest rectangle around their convex hull, its corners rounded
    /// so it covers as much area as the hull. `None` without a finite point.
    pub fn fit(points: &[Vec2]) -> Option<Self> {
        let hull = convex_hull(points);
        let first = *hull.first()?;
        if hull.len() < 3 {
            // A point or a straight cut, such as a thin wall's.
            let last = *hull.last()?;
            let along = last - first;
            let length = along.norm();
            let axis = if length > 1e-6 {
                along / length
            } else {
                vec2(1.0, 0.0)
            };
            return Some(Self {
                center: (first + last) * 0.5,
                axis,
                half: vec2(length * 0.5, 0.0),
                rounding: 0.0,
            });
        }
        // The smallest rectangle around a convex polygon has a side along
        // one of its edges.
        let mut best: Option<(f32, Self)> = None;
        for i in 0..hull.len() {
            let edge = hull[(i + 1) % hull.len()] - hull[i];
            let length = edge.norm();
            if length < 1e-6 {
                continue;
            }
            let axis = edge / length;
            let across = vec2(-axis.y, axis.x);
            let (mut least, mut greatest) = (Vec2::repeat(f32::MAX), Vec2::repeat(f32::MIN));
            for point in &hull {
                let p = vec2(point.dot(&axis), point.dot(&across));
                least = least.inf(&p);
                greatest = greatest.sup(&p);
            }
            let size = greatest - least;
            let area = size.x * size.y;
            if best.is_none_or(|(smallest, _)| area < smallest) {
                let middle = (least + greatest) * 0.5;
                best = Some((
                    area,
                    Self {
                        center: axis * middle.x + across * middle.y,
                        axis,
                        half: size * 0.5,
                        rounding: 0.0,
                    },
                ));
            }
        }
        let (area, mut footprint) = best?;
        // A rectangle rounded by r loses (4 - pi) r^2 of its area.
        let cut = (area - polygon_area(&hull)).max(0.0);
        footprint.rounding = (cut / (4.0 - std::f32::consts::PI))
            .sqrt()
            .min(footprint.half.min());
        Some(footprint)
    }

    /// Points around the footprint's edge: its rectangle's corners, or a
    /// circle's outline when it is rounded all the way.
    pub fn outline(&self) -> Vec<Vec2> {
        let across = vec2(-self.axis.y, self.axis.x);
        let at = |x: f32, y: f32| self.center + self.axis * x + across * y;
        if self.rounding >= self.half.min() && self.rounding > 0.0 {
            let inner = self.half - Vec2::repeat(self.rounding);
            return (0..CIRCLE_POINTS)
                .map(|i| {
                    let angle = i as f32 / CIRCLE_POINTS as f32 * std::f32::consts::TAU;
                    // An outline through the circle's points runs inside it,
                    // so its points sit a little outside, as far as the
                    // chords between them dip.
                    let r = self.rounding / (std::f32::consts::PI / CIRCLE_POINTS as f32).cos();
                    at(
                        inner.x * angle.cos().signum() + r * angle.cos(),
                        inner.y * angle.sin().signum() + r * angle.sin(),
                    )
                })
                .collect();
        }
        let h = self.half;
        vec![at(-h.x, -h.y), at(h.x, -h.y), at(h.x, h.y), at(-h.x, h.y)]
    }
}

/// The signed distance from `p` to the edge of the rectangle of half extents
/// `half` about the origin whose corners are rounded by `rounding`.
pub fn rounded_box_distance(p: Vec2, half: Vec2, rounding: f32) -> f32 {
    let q = p.abs() - (half - Vec2::repeat(rounding));
    q.sup(&Vec2::zeros()).norm() + q.x.max(q.y).min(0.0) - rounding
}

/// The waterline the triangles `indices` of a mesh through `positions` cut
/// through the plane Y = 0 of the space `to_water` carries them into: the
/// points where their edges cross it, in that space's X and Z, grouped into
/// the separate pieces of the mesh that cross it, such as a bridge's posts.
/// Faces only touching the plane from one side cut nothing.
pub fn waterline(positions: &[Vec3], indices: &[u32], to_water: &Mat4) -> Vec<Vec<Vec2>> {
    let moved: Vec<Vec3> = positions
        .iter()
        .map(|p| (to_water * p.push(1.0)).xyz())
        .collect();
    // Each crossing is named by its edge's ends, by position, so faces that
    // share an edge without sharing its vertices still join.
    let key = |p: Vec3| (p * 1e4).map(|c| c.round() as i64);
    let mut crossings: HashMap<[[i64; 3]; 2], usize> = HashMap::new();
    let mut points: Vec<Vec2> = Vec::new();
    let mut parent: Vec<usize> = Vec::new();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for triangle in indices.chunks_exact(3) {
        let corners =
            [triangle[0], triangle[1], triangle[2]].map(|i| moved.get(i as usize).copied());
        let [Some(a), Some(b), Some(c)] = corners else {
            continue;
        };
        if ![a, b, c].iter().all(|p| p.iter().all(|v| v.is_finite())) {
            continue;
        }
        let mut cut = Vec::with_capacity(2);
        for (p, q) in [(a, b), (b, c), (c, a)] {
            if (p.y >= 0.0) == (q.y >= 0.0) {
                continue;
            }
            let (ka, kb) = (key(p), key(q));
            let edge = if [ka.x, ka.y, ka.z] <= [kb.x, kb.y, kb.z] {
                [[ka.x, ka.y, ka.z], [kb.x, kb.y, kb.z]]
            } else {
                [[kb.x, kb.y, kb.z], [ka.x, ka.y, ka.z]]
            };
            let index = *crossings.entry(edge).or_insert_with(|| {
                let f = p.y / (p.y - q.y);
                let at = p + (q - p) * f;
                points.push(vec2(at.x, at.z));
                parent.push(parent.len());
                points.len() - 1
            });
            cut.push(index);
        }
        if let [first, second] = cut[..] {
            let (i, j) = (root(&mut parent, first), root(&mut parent, second));
            parent[i] = j;
        }
    }
    let mut groups: HashMap<usize, Vec<Vec2>> = HashMap::new();
    let mut order = Vec::new();
    for i in 0..points.len() {
        let group = root(&mut parent, i);
        groups
            .entry(group)
            .or_insert_with(|| {
                order.push(group);
                Vec::new()
            })
            .push(points[i]);
    }
    order
        .into_iter()
        .filter_map(|group| groups.remove(&group))
        .collect()
}

/// The footprints of `outlines`, each the points around one thing standing
/// in the water: one fitted to each, those that overlap or nearly touch
/// merged into one fitted around both, so a rock and a circle authored
/// around it ring once.
pub fn merge_footprints(outlines: Vec<Vec<Vec2>>) -> Vec<WaterFootprint> {
    let mut pieces: Vec<(Vec<Vec2>, WaterFootprint)> = outlines
        .into_iter()
        .filter_map(|points| {
            let hull = convex_hull(&points);
            let footprint = WaterFootprint::fit(&hull)?;
            Some((hull, footprint))
        })
        .collect();
    let touching = |a: &(Vec<Vec2>, WaterFootprint), b: &(Vec<Vec2>, WaterFootprint)| {
        a.0.iter().any(|p| b.1.distance(*p) < MERGE_GAP)
            || b.0.iter().any(|p| a.1.distance(*p) < MERGE_GAP)
    };
    'merging: loop {
        for i in 0..pieces.len() {
            for j in i + 1..pieces.len() {
                if touching(&pieces[i], &pieces[j]) {
                    let (other, _) = pieces.remove(j);
                    let mut points = std::mem::take(&mut pieces[i].0);
                    points.extend(other);
                    let hull = convex_hull(&points);
                    if let Some(footprint) = WaterFootprint::fit(&hull) {
                        pieces[i] = (hull, footprint);
                    } else {
                        pieces.remove(i);
                    }
                    continue 'merging;
                }
            }
        }
        break;
    }
    pieces.into_iter().map(|(_, footprint)| footprint).collect()
}

/// The convex hull of the finite `points`, counter-clockwise, without
/// repeated or collinear points.
pub fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut sorted: Vec<Vec2> = points
        .iter()
        .copied()
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .collect();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<Vec2> = Vec::with_capacity(sorted.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let points: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 {
            Box::new(sorted.iter())
        } else {
            Box::new(sorted.iter().rev())
        };
        for &p in points {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    if hull.len() < 3 {
        // Every point on one line: its two ends.
        return vec![sorted[0], sorted[sorted.len() - 1]];
    }
    hull
}

/// The area of the simple polygon through `points`.
fn polygon_area(points: &[Vec2]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f32>()
        .abs()
        * 0.5
}

#[cfg(test)]
mod tests;
