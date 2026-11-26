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
