//! Contracts for the raised area's ramp and platform: they meet the walls'
//! height on the one-metre grid, the ramp's slope and kerbs match the
//! colliders the arena gives them, and the generated prefabs and the arena
//! keep resolving their assets in every palette.
use std::{collections::BTreeSet, path::Path};

use calyx_model_gen::{
    layout::Layout,
    obj::ObjMesh,
    params::{Model, Shape},
};
use serde_json::Value;

const ID: &str = "02289c92-3412-406e-a7e5-3bbb15d7041e";
const TRANSFORM: &str = "c5b3b71f-1f14-4b5b-9881-436118684d29";

fn sandbox() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox")
}

fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn mesh(name: &str) -> ObjMesh {
    ObjMesh::parse(
        &std::fs::read_to_string(sandbox().join(format!("assets/generated/models/{name}.obj")))
            .unwrap(),
    )
    .unwrap()
}

/// The highest point of `mesh` within 0.25 m of `z`.
fn top_near(mesh: &ObjMesh, z: f64) -> f64 {
    mesh.positions
        .iter()
        .filter(|p| (p[2] - z).abs() < 0.25)
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
}

/// A part's box footprint, `(min x, max x, min z, max z)`, in the model's frame.
fn footprint(model: &Model, part: &str) -> (f64, f64, f64, f64) {
    let Shape::Parts { parts } = &model.shape else {
        panic!("{} is assembled from parts", model.name)
    };
    let part = parts.iter().find(|p| p.name == part).unwrap();
    let (Shape::Box { size, .. } | Shape::Wedge { size, .. }) = part.shape else {
        panic!("{} is a block", part.name)
    };
    let [x, _, z] = part.offset;
    (
        x - size[0] / 2.0,
        x + size[0] / 2.0,
        z - size[2] / 2.0,
        z + size[2] / 2.0,
    )
}

#[test]
fn the_ramp_climbs_to_the_wall_height_at_one_in_five_between_raised_kerbs() {
    let wall_height = mesh("wall_short__body").bounds().1[1];
    let deck = mesh("ramp__deck");
    let (low, high) = deck.bounds();
    assert!((high[0] - low[0] - 4.0).abs() < 1e-4, "a 4 m wide lane");
    assert!(high[2] <= 4.0 + 1e-4 && low[2] >= -4.0 - 1e-4);
    assert!(low[1].abs() < 1e-4, "stands on the ground");
    // It rises along +Z, from the ground at its foot to the wall height at
    // its crest, and nothing stands above that 1:5 slope, which is where
    // the arena's drive surface lies.
    assert!(top_near(&deck, -3.9) < 0.1);
    assert!((top_near(&deck, 3.9) - wall_height).abs() < 0.03);
    let slope = |z: f64| wall_height * (z + 4.0) / 8.0;
    assert!(deck.positions.iter().all(|p| p[1] <= slope(p[2]) + 1e-3));
    for kerb in ["ramp__kerb_left", "ramp__kerb_right"] {
        let kerb = mesh(kerb);
        assert!((top_near(&kerb, -4.0) - 0.5).abs() < 0.05);
        assert!((top_near(&kerb, 4.0) - (wall_height + 0.5)).abs() < 0.05);
        assert!(kerb
            .positions
            .iter()
            .all(|p| p[1] <= slope(p[2]) + 0.5 + 1e-3));
    }
    let ramp = Model::load(&sandbox().join("asset_sources/models/ramp.toml")).unwrap();
    assert_eq!(footprint(&ramp, "deck"), (-2.0, 2.0, -4.0, 4.0));
    assert_eq!(footprint(&ramp, "kerb_left"), (-2.5, -2.0, -4.0, 4.0));
    assert_eq!(footprint(&ramp, "kerb_right"), (2.0, 2.5, -4.0, 4.0));
}

#[test]
fn the_platform_is_wall_height_on_the_grid_with_ramp_openings_in_its_kerbs() {
    let wall_height = mesh("wall_short__body").bounds().1[1];
    let (low, high) = mesh("platform__body").bounds();
    assert!(
        (high[1] - wall_height).abs() < 1e-4,
        "its edges meet the walls' top"
    );
    for (axis, size) in [(0, 10.0), (2, 8.0)] {
        assert!((high[axis] - low[axis] - size).abs() < 1e-4);
        // Whole metres either side of a root on the one-metre grid.
        assert!((low[axis] - low[axis].round()).abs() < 1e-4);
    }
    let platform = Model::load(&sandbox().join("asset_sources/models/platform.toml")).unwrap();
    // The south opening sits between the two south kerbs, centred on X = 0;
    // the west opening between the two west kerbs, centred on Z = 1. Each is
    // as wide as the ramp's lane, so the ramp's kerbs meet the rim kerbs.
    let south = (
        footprint(&platform, "kerb_south_west").1,
        footprint(&platform, "kerb_south_east").0,
    );
    assert_eq!(south, (-2.0, 2.0));
    let west = (
        footprint(&platform, "kerb_west_south").3,
        footprint(&platform, "kerb_west_north").2,
    );
    assert_eq!(west, (-1.0, 3.0));
    // The rim kerbs stand on the deck, inside the footprint.
    let Shape::Parts { parts } = &platform.shape else {
        panic!("the platform is assembled from parts")
    };
    for part in parts.iter().filter(|p| p.name.starts_with("kerb_")) {
        assert!((part.offset[1] - wall_height).abs() < 1e-9, "{}", part.name);
        let (x0, x1, z0, z1) = footprint(&platform, &part.name);
        assert!(
            x0 >= -5.0 && x1 <= 5.0 && z0 >= -4.0 && z1 <= 4.0,
            "{}",
            part.name
        );
    }
}

#[test]
fn invalid_wedges_fail_before_blender_runs() {
    let wedge = |keys: &str| {
        Model::parse(
            "wedge",
            &format!("kind='wedge'\nrole='walls'\n{keys}"),
            Path::new(""),
        )
    };
    assert!(wedge("size=[4,1.6,8]").is_ok());
    assert!(wedge("size=[0.5,2.1,8]\nlow=0.5\nbevel=0.12").is_ok());
    for keys in [
        "size=[4,1.6,8]\nlow=1.6",
        "size=[4,1.6,8]\nlow=-0.1",
        "size=[4,0,8]",
        "size=[0.5,2.1,8]\nlow=0.5\nbevel=0.25",
        "size=[4,1.6,8]\nlow=0.2\nbevel=0.1",
    ] {
        assert!(wedge(keys).is_err(), "{keys}");
    }
}

#[test]
fn ramp_and_platform_prefabs_keep_stable_references_in_every_palette() {
    let layout = Layout::new(&sandbox().join("assets/generated")).unwrap();
    let models = ["ramp", "platform"].map(|name| {
        Model::load(&sandbox().join(format!("asset_sources/models/{name}.toml"))).unwrap()
    });
    let plan = layout.plan(&models);
    let ids: BTreeSet<_> = plan.prefabs.iter().map(|p| p.asset.id).collect();
    assert_eq!(ids.len(), 6, "two models in three palettes");
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
    // The arena's instances link to the Sunny prefabs and keep their parts.
    let arena = read(sandbox().join("assets/game/scenes/arena.cxscene"));
    for (instance_name, model, parts) in [
        ("Arena Platform", "platform", 8),
        ("Arena Ramp South", "ramp", 3),
        ("Arena Ramp West", "ramp", 3),
    ] {
        let (instance, _) = arena["components"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, c)| c[ID]["name"] == instance_name)
            .unwrap_or_else(|| panic!("the arena has {instance_name}"));
        let meta = read(sandbox().join(format!("assets/generated/prefabs/sunny/{model}.meta")));
        assert_eq!(
            arena["prefab_links"][instance]["prefab"],
            meta["main"]["id"]
        );
        let linked = arena["prefab_links"]
            .as_object()
            .unwrap()
            .values()
            .filter(|link| {
                link["instance"] == instance.as_str() && link["object"] != link["instance"]
            })
            .count();
        assert!(linked >= parts, "{instance_name} keeps {linked} parts");
        assert!(arena["components"][instance][TRANSFORM].is_object());
    }
}
