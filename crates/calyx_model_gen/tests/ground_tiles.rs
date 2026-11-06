use std::path::Path;

use calyx_model_gen::obj::ObjMesh;
use serde_json::Value;

#[test]
fn ground_tile_surfaces_share_a_level_top_and_resolve_in_every_palette() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox/assets/generated");
    for kind in ["grass", "sand", "dirt", "paving", "road", "road_marked"] {
        let part = if kind == "paving" { "nw" } else { "surface" };
        let path = assets.join(format!("models/ground_{kind}__{part}.obj"));
        let mesh = ObjMesh::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let (low, high) = mesh.bounds();
        assert!(low[1].abs() < 1e-5);
        assert!((high[1] - 0.1).abs() < 1e-5, "{kind} creates a step");
        let width = if kind == "paving" { 0.49 } else { 1.0 };
        for axis in [0, 2] {
            assert!(
                (high[axis] - low[axis] - width).abs() < 1e-5,
                "{kind} does not tile"
            );
        }
        for map in ["sunny", "sunbaked", "moonlit"] {
            let prefab: Value = serde_json::from_str(
                &std::fs::read_to_string(
                    assets.join(format!("prefabs/{map}/ground_{kind}.cxprefab")),
                )
                .unwrap(),
            )
            .unwrap();
            let root = prefab["root"].as_str().unwrap();
            assert!(prefab["data"]["components"][root].is_object());
            for components in prefab["data"]["components"].as_object().unwrap().values() {
                if let Some(renderer) = components.get("93fd32b1-7127-4c88-8e89-893512af58de") {
                    for (field, folder) in [("mesh", "models"), ("material", "materials")] {
                        let dir = if folder == "materials" {
                            assets.join(folder).join(map)
                        } else {
                            assets.join(folder)
                        };
                        let found = std::fs::read_dir(dir)
                            .unwrap()
                            .flatten()
                            .filter(|entry| entry.path().extension().is_some_and(|e| e == "meta"))
                            .any(|entry| {
                                let meta: Value = serde_json::from_str(
                                    &std::fs::read_to_string(entry.path()).unwrap(),
                                )
                                .unwrap();
                                meta["main"]["id"] == renderer[field]
                            });
                        assert!(found, "missing {field} for {map}/{kind}");
                    }
                }
            }
        }
    }
}
