//! The generator's output layout and ids, on the fixture models, with a
//! stand-in for Blender that writes a cuboid of each model's size.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use calyx_model_gen::layout::Layout;
use calyx_model_gen::params::{self, Shape};
use calyx_model_gen::{generate, MeshJob};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/models")
}

/// A fresh `<tmp>/<test>/assets/generated`.
fn out_dir(test: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("calyx_model_gen")
        .join(test);
    let _ = std::fs::remove_dir_all(&root);
    root.join("assets").join("generated")
}

/// An OBJ cuboid `w` x `h` x `d` standing on y = 0, centred on x and z,
/// facing out, or in for a hull.
fn cuboid(w: f64, h: f64, d: f64, inverted: bool) -> String {
    let mut text = String::new();
    for z in [0.0, 1.0] {
        for y in [0.0, 1.0] {
            for x in [0.0, 1.0] {
                text += &format!("v {} {} {}\n", (x - 0.5) * w, y * h, (z - 0.5) * d);
            }
        }
    }
    // Vertex index (1-based) is 1 + x + 2y + 4z.
    let faces: [([usize; 4], [f64; 3]); 6] = [
        ([1, 3, 4, 2], [0.0, 0.0, -1.0]),
        ([5, 6, 8, 7], [0.0, 0.0, 1.0]),
        ([1, 2, 6, 5], [0.0, -1.0, 0.0]),
        ([3, 7, 8, 4], [0.0, 1.0, 0.0]),
        ([1, 5, 7, 3], [-1.0, 0.0, 0.0]),
        ([2, 4, 8, 6], [1.0, 0.0, 0.0]),
    ];
    let sign = if inverted { -1.0 } else { 1.0 };
    for (n, (q, normal)) in faces.iter().enumerate() {
        let n = n + 1;
        text += &format!(
            "vn {} {} {}\n",
            sign * normal[0],
            sign * normal[1],
            sign * normal[2]
        );
        let mut tris = [[q[0], q[1], q[2]], [q[0], q[2], q[3]]];
        if inverted {
            tris.iter_mut().for_each(|t| t.reverse());
        }
        for t in tris {
            text += &format!("f {}//{n} {}//{n} {}//{n}\n", t[0], t[1], t[2]);
        }
    }
    text
}

/// Stands in for Blender: a cuboid of each box's size (with its band), a
/// unit cube for the other kinds, and a slightly larger inverted cuboid for
/// each hull.
fn fake_blender(jobs: &[MeshJob]) -> Result<(), String> {
    for job in jobs {
        let (w, h, d) = match &job.shape {
            Shape::Box { size, band, .. } => {
                let o = band.map_or(0.0, |b| b.overhang * 2.0);
                (size[0] + o, size[1], size[2] + o)
            }
            _ => (1.0, 1.0, 1.0),
        };
        std::fs::write(&job.obj, cuboid(w, h, d, false)).unwrap();
        if let Some(hull) = &job.outline_obj {
            std::fs::write(hull, cuboid(w + 0.1, h + 0.1, d + 0.1, true)).unwrap();
        }
    }
    Ok(())
}

/// Every file under `root`, relative and `/`-separated.
fn tree(root: &Path) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                files.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    files
}

fn meta(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn fixture_models() -> Vec<params::Model> {
    params::load_dir(&fixtures()).unwrap()
}

#[test]
fn the_fixture_folder_holds_three_models_sorted_by_name() {
    let models = fixture_models();
    let names: Vec<_> = models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["crate", "rock", "wall_low"]);
}

#[test]
fn a_run_writes_each_model_its_hull_and_the_used_roles_materials_on_both_maps() {
    let out = out_dir("layout");
    let layout = Layout::new(&out).unwrap();
    let report = generate(&layout, &fixture_models(), fake_blender).unwrap();
    assert_eq!(
        (report.models, report.outlines, report.materials),
        (3, 1, 8)
    );
    let mut expected: BTreeSet<String> = [
        "README.md",
        "models/crate.obj",
        "models/crate.meta",
        "models/outline/crate.obj",
        "models/outline/crate.meta",
        "models/rock.obj",
        "models/rock.meta",
        "models/wall_low.obj",
        "models/wall_low.meta",
    ]
    .map(String::from)
    .into();
    for map in ["sunny", "sunbaked"] {
        // Ink because the crate has a hull.
        for role in ["barriers", "crates", "rocks", "ink"] {
            expected.insert(format!("materials/{map}/{role}.cxmat"));
            expected.insert(format!("materials/{map}/{role}.meta"));
        }
    }
    assert_eq!(tree(&out), expected);
}

#[test]
fn ids_derive_from_asset_names_and_survive_a_rerun() {
    let out = out_dir("ids");
    let layout = Layout::new(&out).unwrap();
    let models = fixture_models();
    generate(&layout, &models, fake_blender).unwrap();
    // Python: uuid.uuid5(UUID("e6b60a1c-4e82-4a6f-9275-a09402916599"), name)
    let expected = [
        (
            "models/crate.meta",
            "generated/models/crate",
            "ab200de6-0ee5-55ab-885c-66964e1f6681",
        ),
        (
            "models/outline/crate.meta",
            "generated/models/outline/crate",
            "b706a217-a3a6-50fd-8353-02a97233021f",
        ),
        (
            "materials/sunny/crates.meta",
            "generated/materials/sunny/crates",
            "62004568-6c7f-5d40-b296-721f4dea7f3a",
        ),
        (
            "materials/sunbaked/ink.meta",
            "generated/materials/sunbaked/ink",
            "5063c3dd-a38c-5aa7-888d-a3f98a6d5b67",
        ),
    ];
    for (file, name, id) in expected {
        let meta = meta(&out.join(file));
        assert_eq!(meta["main"]["name"], name, "{file}");
        assert_eq!(meta["main"]["id"], id, "{file}");
    }
    assert_eq!(
        meta(&out.join("models/crate.meta"))["main"]["type_uuid"],
        "792d264b-de6f-4431-b59f-76f18fdb3bfe"
    );
    let first = std::fs::read_to_string(out.join("models/crate.meta")).unwrap();
    generate(&layout, &models, fake_blender).unwrap();
    assert_eq!(
        std::fs::read_to_string(out.join("models/crate.meta")).unwrap(),
        first
    );
}

#[test]
fn a_rerun_removes_what_it_no_longer_writes() {
    let out = out_dir("stale");
    let layout = Layout::new(&out).unwrap();
    let models = fixture_models();
    generate(&layout, &models, fake_blender).unwrap();
    // Drop the crate (the only hull, so ink goes too) and the rock.
    let rest: Vec<_> = models
        .into_iter()
        .filter(|m| m.name == "wall_low")
        .collect();
    let report = generate(&layout, &rest, fake_blender).unwrap();
    let files = tree(&out);
    assert!(!files.contains("models/crate.obj"));
    assert!(!files.contains("models/outline/crate.meta"));
    assert!(!files.contains("materials/sunny/ink.cxmat"));
    assert!(files.contains("materials/sunbaked/barriers.cxmat"));
    // Crate, its hull and rock: 2 files each; crates, rocks and ink
    // materials on both maps: 2 files each.
    assert_eq!(report.removed.len(), 3 * 2 + 3 * 2 * 2);
}

#[test]
fn a_mesh_of_the_wrong_size_is_refused_before_its_meta_is_written() {
    let out = out_dir("wrong_size");
    let layout = Layout::new(&out).unwrap();
    let models: Vec<_> = fixture_models()
        .into_iter()
        .filter(|m| m.name == "crate")
        .collect();
    let err = generate(&layout, &models, |jobs| {
        // Height and depth swapped, as a missed Z-up to Y-up conversion would.
        std::fs::write(&jobs[0].obj, cuboid(1.18, 1.18, 1.1, false)).unwrap();
        let hull = jobs[0].outline_obj.as_ref().unwrap();
        std::fs::write(hull, cuboid(1.3, 1.3, 1.3, true)).unwrap();
        Ok(())
    })
    .unwrap_err();
    assert!(err.contains("expected 1.1"), "{err}");
    assert!(!out.join("models/crate.meta").exists());
}

#[test]
fn a_hull_that_is_not_inverted_is_refused() {
    let out = out_dir("hull");
    let layout = Layout::new(&out).unwrap();
    let models: Vec<_> = fixture_models()
        .into_iter()
        .filter(|m| m.name == "crate")
        .collect();
    let err = generate(&layout, &models, |jobs| {
        std::fs::write(&jobs[0].obj, cuboid(1.18, 1.1, 1.18, false)).unwrap();
        let hull = jobs[0].outline_obj.as_ref().unwrap();
        std::fs::write(hull, cuboid(1.3, 1.3, 1.3, false)).unwrap();
        Ok(())
    })
    .unwrap_err();
    assert!(err.contains("not inverted"), "{err}");
}

#[test]
fn assemblies_write_stable_prefabs_and_remove_stale_parts_on_regeneration() {
    let text = r#"
[[parts]]
name = "body"
kind = "box"
role = "crate_panel"
size = [1, 1, 1]

[[parts]]
name = "frame"
kind = "box"
role = "crate_frame"
size = [0.2, 1.2, 0.2]
offset = [0.4, 0, 0.4]
rotation = [0, 45, 0]
"#;
    let out = out_dir("assembly");
    let layout = Layout::new(&out).unwrap();
    let model = params::Model::parse("framed", text, Path::new("")).unwrap();
    let report = generate(&layout, std::slice::from_ref(&model), fake_blender).unwrap();
    assert_eq!((report.models, report.materials), (1, 4));
    let prefab = out.join("prefabs/sunny/framed.cxprefab");
    let first = std::fs::read_to_string(&prefab).unwrap();
    assert_eq!(
        meta(&prefab.with_extension("meta"))["main"]["type_uuid"],
        calyx_model_gen::layout::TYPE_PREFAB
    );
    generate(&layout, &[model], fake_blender).unwrap();
    assert_eq!(std::fs::read_to_string(&prefab).unwrap(), first);
    let single = params::Model::parse(
        "framed",
        "kind = \"box\"\nrole = \"crates\"\nsize = [1, 1, 1]",
        Path::new(""),
    )
    .unwrap();
    generate(&layout, &[single], fake_blender).unwrap();
    assert!(!prefab.exists());
    assert!(!out.join("models/framed__frame.obj").exists());
    assert!(out.join("models/framed.obj").exists());
}

#[test]
fn a_part_name_collision_is_refused_before_blender() {
    let models = [
        params::Model::parse(
            "a",
            "[[parts]]\nname = \"b\"\nkind = \"box\"\nrole = \"crates\"\nsize = [1,1,1]",
            Path::new(""),
        )
        .unwrap(),
        params::Model::parse(
            "a__b",
            "kind = \"box\"\nrole = \"crates\"\nsize = [1,1,1]",
            Path::new(""),
        )
        .unwrap(),
    ];
    let layout = Layout::new(&out_dir("collision")).unwrap();
    let error = generate(&layout, &models, |_| panic!("must fail before building")).unwrap_err();
    assert!(error.contains("same mesh name"), "{error}");
}
