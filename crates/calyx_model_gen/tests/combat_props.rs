//! Contracts shared by generated combat props and the gameplay scene.
use std::{collections::BTreeSet, path::Path};

use calyx_model_gen::{layout::Layout, obj::ObjMesh, palette::Map, params::Model};
use serde_json::Value;

const ID: &str = "02289c92-3412-406e-a7e5-3bbb15d7041e";
const MESH: &str = "93fd32b1-7127-4c88-8e89-893512af58de";
const TRANSFORM: &str = "c5b3b71f-1f14-4b5b-9881-436118684d29";

fn sandbox() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox")
}

fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn named<'a>(data: &'a Value, name: &str) -> (&'a str, &'a Value) {
    data["components"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, components)| components[ID]["name"] == name)
        .map(|(id, components)| (id.as_str(), components))
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn assert_prefab_geometry(data: &Value, instance_name: &str, model: &str) {
    let source = read(sandbox().join(format!("assets/generated/prefabs/sunny/{model}.cxprefab")));
    let (instance, _) = named(data, instance_name);
    let mut pairs = vec![(source["root"].as_str().unwrap(), instance)];
    let mut index = 0;
    while let Some(&(original, actual)) = pairs.get(index) {
        let original_parts = source["data"]["hierarchy"][original]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let actual_parts = data["hierarchy"][actual]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert_eq!(original_parts.len(), actual_parts.len());
        // Read the strings from the documents, so the references outlive this loop.
        for part in 0..original_parts.len() {
            pairs.push((
                source["data"]["hierarchy"][original][part]
                    .as_str()
                    .unwrap(),
                data["hierarchy"][actual][part].as_str().unwrap(),
            ));
        }
        if index > 0 {
            let expected = &source["data"]["components"][original];
            let actual = &data["components"][actual];
            assert_eq!(expected[ID]["name"], actual[ID]["name"]);
            for field in ["mesh", "material", "cast_shadows"] {
                assert_eq!(expected[MESH][field], actual[MESH][field]);
            }
            for field in ["position", "rotation", "scale"] {
                for (expected, actual) in expected[TRANSFORM]["transform"][field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(actual[TRANSFORM]["transform"][field].as_array().unwrap())
                {
                    assert!((expected.as_f64().unwrap() - actual.as_f64().unwrap()).abs() < 1e-5);
                }
            }
        }
        index += 1;
    }
    assert_eq!(
        pairs.len(),
        source["data"]["components"].as_object().unwrap().len()
    );
}

#[test]
fn combat_prop_prefabs_and_stable_references_match_their_sources_in_every_palette() {
    let layout = Layout::new(&sandbox().join("assets/generated")).unwrap();
    let models = ["sentry", "explosive_barrel", "aim_reticle"].map(|name| {
        Model::load(&sandbox().join(format!("asset_sources/models/{name}.toml"))).unwrap()
    });
    let plan = layout.plan(&models);
    let ids: BTreeSet<_> = plan.prefabs.iter().map(|p| p.asset.id).collect();
    assert_eq!(ids.len(), 9);
    for prefab in &plan.prefabs {
        assert_eq!(read(&prefab.asset.path), prefab.data(&layout));
        assert_eq!(
            read(prefab.asset.meta_path())["main"]["id"],
            prefab.asset.id.to_string()
        );
        for part in &prefab.parts {
            assert_eq!(
                read(part.mesh.meta_path())["main"]["id"],
                part.mesh.id.to_string()
            );
            assert_eq!(
                read(part.material.meta_path())["main"]["id"],
                part.material.id.to_string()
            );
            assert!(part.mesh.path.is_file());
            assert!(part.material.path.is_file());
        }
    }
}

#[test]
fn sentry_barrel_follows_the_head_and_ends_at_the_gameplay_muzzle() {
    let prefab = read(sandbox().join("assets/generated/prefabs/sunny/sentry.cxprefab"));
    let data = &prefab["data"];
    let (head_id, head) = named(data, "head");
    let (barrel_id, barrel) = named(data, "barrel");
    let (muzzle_id, muzzle) = named(data, "muzzle");
    let (base_id, _) = named(data, "base");
    let children = data["hierarchy"][head_id].as_array().unwrap();
    assert!(children.contains(&Value::from(barrel_id)));
    assert!(children.contains(&Value::from(muzzle_id)));
    assert!(!children.contains(&Value::from(base_id)));
    let head_y = head[TRANSFORM]["transform"]["position"][1]
        .as_f64()
        .unwrap();
    let barrel_y = barrel[TRANSFORM]["transform"]["position"][1]
        .as_f64()
        .unwrap();
    assert!((head_y + barrel_y - 0.5).abs() < 1e-6);
    let mesh = ObjMesh::parse(
        &std::fs::read_to_string(sandbox().join("assets/generated/models/sentry__muzzle.obj"))
            .unwrap(),
    )
    .unwrap();
    let height = mesh.bounds().1[1];
    let end = muzzle[TRANSFORM]["transform"]["position"][2]
        .as_f64()
        .unwrap()
        + height;
    assert!((end - 1.2).abs() < 1e-6);
}

#[test]
fn barrel_lid_is_recessed_between_the_drum_and_rim_without_coplanar_tops() {
    let prefab = read(sandbox().join("assets/generated/prefabs/sunny/explosive_barrel.cxprefab"));
    let top = |part: &str| {
        let (_, components) = named(&prefab["data"], part);
        let mesh = ObjMesh::parse(
            &std::fs::read_to_string(sandbox().join(format!(
                "assets/generated/models/explosive_barrel__{part}.obj"
            )))
            .unwrap(),
        )
        .unwrap();
        components[TRANSFORM]["transform"]["position"][1]
            .as_f64()
            .unwrap()
            + mesh.bounds().1[1]
    };
    assert!(top("drum") + 0.01 < top("lid"));
    assert!(top("lid") + 0.01 < top("top_rim"));
}

#[test]
fn reticle_has_an_open_center_and_no_ink_in_every_palette() {
    let mesh = ObjMesh::parse(
        &std::fs::read_to_string(
            sandbox().join("assets/generated/models/aim_reticle__aim_ring.obj"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(mesh.signed_volume() > 0.0);
    assert!(mesh.bounds().1[1] < 0.02);
    for triangle in &mesh.triangles {
        // Every triangle remains on the annulus; even the inner edge stays
        // away from the aim point, which the player must still see.
        let center = triangle.iter().fold([0.0; 3], |mut sum, vertex| {
            for (axis, value) in sum.iter_mut().enumerate() {
                *value += mesh.positions[vertex.0][axis] / 3.0;
            }
            sum
        });
        assert!(center[0].hypot(center[2]) > 0.4);
    }
    for map in Map::ALL {
        let data = read(sandbox().join(format!(
            "assets/generated/prefabs/{}/aim_reticle.cxprefab",
            map.folder()
        )));
        let parts = data["data"]["components"].as_object().unwrap();
        assert_eq!(parts.values().filter(|c| c.get(MESH).is_some()).count(), 10);
        assert!(parts
            .values()
            .filter_map(|c| c.get(MESH))
            .all(|mesh| mesh["cast_shadows"] == false));
        for role in ["reticle_light", "reticle_dark"] {
            let material = read(sandbox().join(format!(
                "assets/generated/materials/{}/{role}.cxmat",
                map.folder()
            )));
            let ink = material["variables"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == "outline_style")
                .unwrap();
            assert_eq!(ink["value"]["Uint"], 0);
        }
    }
}

#[test]
fn invalid_rings_fail_before_blender_runs() {
    for dimensions in [
        "radius=1\ninner_radius=1\nheight=0.02",
        "radius=1\ninner_radius=-0.5\nheight=0.02",
        "radius=1\ninner_radius=0.5\nheight=0",
        "radius=1\ninner_radius=0.5\nheight=0.02\nvertices=2",
    ] {
        assert!(Model::parse(
            "ring",
            &format!("kind='ring'\nrole='reticle_light'\n{dimensions}"),
            Path::new("")
        )
        .is_err());
    }
}

#[test]
fn arena_and_player_prefab_use_the_reticle_and_arena_head_reference_resolves() {
    for path in ["game/scenes/arena.cxscene", "game/prefabs/tank.cxprefab"] {
        let file = read(sandbox().join(format!("assets/{path}")));
        let data = file.get("data").unwrap_or(&file);
        let (crosshair_id, crosshair) = named(data, "Crosshair");
        assert!(
            crosshair.get(MESH).is_none(),
            "the placeholder sphere is gone"
        );
        assert_eq!(
            crosshair[TRANSFORM]["transform"]["scale"],
            serde_json::json!([1.0, 1.0, 1.0])
        );
        assert_eq!(
            data["hierarchy"][crosshair_id].as_array().unwrap().len(),
            10
        );
        let (_, tank) = named(data, "Tank");
        assert_eq!(
            tank["21b467c0-9409-4a7f-b750-418809609eb1"]["crosshair"],
            crosshair_id
        );
        assert_prefab_geometry(data, "Crosshair", "aim_reticle");
    }
    let arena = read(sandbox().join("assets/game/scenes/arena.cxscene"));
    let (_, sentry) = named(&arena, "Sentry");
    assert!(sentry.get(MESH).is_none());
    let head = sentry["3e9d6c1a-7b52-4f08-a4c3-95e1d2b7f604"]["head"]
        .as_str()
        .unwrap();
    assert_eq!(arena["components"][head][ID]["name"], "head");
    assert_prefab_geometry(&arena, "Sentry Model", "sentry");
    let (instance, _) = named(&arena, "Sentry Model");
    let prefab = read(sandbox().join("assets/generated/prefabs/sunny/sentry.cxprefab"));
    let meta = read(sandbox().join("assets/generated/prefabs/sunny/sentry.meta"));
    let (source_head, _) = named(&prefab["data"], "head");
    assert_eq!(
        arena["prefab_links"][instance]["prefab"],
        meta["main"]["id"]
    );
    assert_eq!(arena["prefab_links"][head]["instance"], instance);
    assert_eq!(arena["prefab_links"][head]["object"], source_head);
}
