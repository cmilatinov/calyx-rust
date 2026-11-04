//! Reading back the OBJ files Blender writes, to check them before they
//! become assets.
//!
//! The generator writes triangles in Calyx's left-handed, Y-up space, wound
//! so that `(b - a) x (c - a)` points out of the surface, as
//! `calyx_unity_import` writes its converted meshes.

/// A triangle mesh as `v`, `vn` and `f v//vn` lines give it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjMesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    /// Each corner's position and normal indices, zero-based.
    pub triangles: Vec<[(usize, usize); 3]>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(u: [f64; 3], v: [f64; 3]) -> [f64; 3] {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn dot(u: [f64; 3], v: [f64; 3]) -> f64 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}

impl ObjMesh {
    pub fn parse(text: &str) -> Result<ObjMesh, String> {
        let mut mesh = ObjMesh::default();
        let parse3 = |parts: &[&str]| -> Result<[f64; 3], String> {
            if parts.len() != 3 {
                return Err(format!("expected three numbers, got {}", parts.join(" ")));
            }
            let mut out = [0.0; 3];
            for (slot, part) in out.iter_mut().zip(parts) {
                *slot = part.parse().map_err(|_| format!("bad number {part}"))?;
            }
            Ok(out)
        };
        for (number, line) in text.lines().enumerate() {
            let at = |err: String| format!("line {}: {err}", number + 1);
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.first() {
                Some(&"v") => mesh.positions.push(parse3(&parts[1..]).map_err(at)?),
                Some(&"vn") => mesh.normals.push(parse3(&parts[1..]).map_err(at)?),
                Some(&"f") => {
                    if parts.len() != 4 {
                        return Err(at("faces must be triangles".into()));
                    }
                    let mut corners = [(0, 0); 3];
                    for (slot, corner) in corners.iter_mut().zip(&parts[1..]) {
                        let fields: Vec<&str> = corner.split('/').collect();
                        let index = |text: Option<&&str>, count: usize| -> Result<usize, String> {
                            text.and_then(|t| t.parse::<usize>().ok())
                                .filter(|i| (1..=count).contains(i))
                                .map(|i| i - 1)
                                .ok_or_else(|| format!("bad corner {corner}"))
                        };
                        *slot = (
                            index(fields.first(), mesh.positions.len()).map_err(at)?,
                            index(fields.get(2), mesh.normals.len()).map_err(at)?,
                        );
                    }
                    mesh.triangles.push(corners);
                }
                _ => {}
            }
        }
        if mesh.triangles.is_empty() {
            return Err("the mesh has no triangles".into());
        }
        Ok(mesh)
    }

    fn corner(&self, triangle: &[(usize, usize); 3], k: usize) -> [f64; 3] {
        self.positions[triangle[k].0]
    }

    /// The smallest and largest corner of the box around the mesh.
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for p in &self.positions {
            for k in 0..3 {
                low[k] = low[k].min(p[k]);
                high[k] = high[k].max(p[k]);
            }
        }
        (low, high)
    }

    /// The enclosed volume, positive when the triangles face out (the
    /// divergence theorem over the faces); an inverted hull's is negative.
    pub fn signed_volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let (a, b, c) = (self.corner(t, 0), self.corner(t, 1), self.corner(t, 2));
                dot(a, cross(b, c)) / 6.0
            })
            .sum()
    }

    /// Checks every corner normal is unit length and on the side its
    /// triangle faces; with `faceted`, that it is the triangle's own normal.
    pub fn check_normals(&self, faceted: bool) -> Result<(), String> {
        for t in &self.triangles {
            let (a, b, c) = (self.corner(t, 0), self.corner(t, 1), self.corner(t, 2));
            let face = cross(sub(b, a), sub(c, a));
            let length = dot(face, face).sqrt();
            if length < 1e-9 {
                continue; // a degenerate triangle has no face normal to compare
            }
            for &(_, n) in t {
                let normal = self.normals[n];
                if (dot(normal, normal).sqrt() - 1.0).abs() > 1e-3 {
                    return Err(format!("normal {normal:?} is not unit length"));
                }
                let along = dot(face, normal) / length;
                if faceted && along < 0.999 {
                    return Err("a corner normal is not its triangle's face normal".into());
                }
                if along <= 0.0 {
                    return Err("a corner normal points against its triangle's winding".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube from (0, 0, 0) to (1, 1, 1), wound to face out, with
    /// each face's normal.
    fn cube(inverted: bool) -> String {
        let mut text = String::new();
        for z in [0, 1] {
            for y in [0, 1] {
                for x in [0, 1] {
                    text += &format!("v {x} {y} {z}\n");
                }
            }
        }
        // Vertex index (1-based) is 1 + x + 2y + 4z.
        let faces: [([usize; 4], &str); 6] = [
            ([1, 3, 4, 2], "0 0 -1"),
            ([5, 6, 8, 7], "0 0 1"),
            ([1, 2, 6, 5], "0 -1 0"),
            ([3, 7, 8, 4], "0 1 0"),
            ([1, 5, 7, 3], "-1 0 0"),
            ([2, 4, 8, 6], "1 0 0"),
        ];
        for (n, (quad, normal)) in faces.iter().enumerate() {
            let normal = if inverted {
                normal.replace('1', "-1").replace("--", "")
            } else {
                normal.to_string()
            };
            text += &format!("vn {normal}\n");
            let mut tris = [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]];
            if inverted {
                tris.iter_mut().for_each(|t| t.reverse());
            }
            for t in tris {
                let n = n + 1;
                text += &format!("f {}//{n} {}//{n} {}//{n}\n", t[0], t[1], t[2]);
            }
        }
        text
    }

    #[test]
    fn a_cube_faces_out_with_its_face_normals() {
        let mesh = ObjMesh::parse(&cube(false)).unwrap();
        assert_eq!(mesh.bounds(), ([0.0; 3], [1.0; 3]));
        assert!((mesh.signed_volume() - 1.0).abs() < 1e-9);
        assert_eq!(mesh.check_normals(true), Ok(()));
    }

    #[test]
    fn an_inverted_cube_has_negative_volume() {
        let mesh = ObjMesh::parse(&cube(true)).unwrap();
        assert!((mesh.signed_volume() + 1.0).abs() < 1e-9);
        assert_eq!(mesh.check_normals(true), Ok(()));
    }

    #[test]
    fn a_normal_against_the_winding_fails() {
        let text = cube(false).replacen("vn 0 0 -1", "vn 0 0 1", 1);
        let mesh = ObjMesh::parse(&text).unwrap();
        assert!(mesh.check_normals(false).is_err());
    }

    #[test]
    fn a_smooth_normal_fails_only_the_faceted_check() {
        // The bottom face's corners lean towards the side faces.
        let text = cube(false).replacen("vn 0 0 -1", "vn 0.6 0 -0.8", 1);
        let mesh = ObjMesh::parse(&text).unwrap();
        assert_eq!(mesh.check_normals(false), Ok(()));
        assert!(mesh.check_normals(true).is_err());
    }

    #[test]
    fn a_face_must_be_a_triangle_with_known_indices() {
        assert!(ObjMesh::parse("v 0 0 0\nvn 0 1 0\nf 1//1 1//1 1//1 1//1\n").is_err());
        assert!(ObjMesh::parse("v 0 0 0\nvn 0 1 0\nf 1//1 2//1 1//1\n").is_err());
        assert!(ObjMesh::parse("v 0 0 0\n").is_err());
    }
}
