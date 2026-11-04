use std::path::Path;

use calyx_model_gen::{
    layout::Layout,
    palette,
    params::{Model, Shape},
};

const PARTS: &str = r#"
[[parts]]
name = "trunk"
kind = "cylinder"
role = "tree_trunks"
radius = 0.2
height = 1.2
flare = { radius = 0.4, height = 0.2 }

[[parts]]
name = "canopy"
kind = "blob"
role = "tree_canopies"
offset = [0, 1.2, 0]
rotation = [0, 45, 0]
lobes = [{center = [0, 0.6, 0], radius = 0.8}]
"#;

#[test]
fn parts_keep_their_shape_role_and_local_transform() {
    let model = Model::parse("tree", PARTS, Path::new("")).unwrap();
    let Shape::Parts { parts } = model.shape else {
        panic!("assembly")
    };
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].name, "trunk");
    assert_eq!(parts[0].offset, [0.0; 3]);
    assert_eq!(parts[1].offset, [0.0, 1.2, 0.0]);
    assert_eq!(parts[1].rotation, [0.0, 45.0, 0.0]);
    assert!(matches!(
        parts[0].shape,
        Shape::Cylinder { flare: Some(_), .. }
    ));
    assert_eq!(parts[1].role, "tree_canopies");
}

#[test]
fn invalid_parts_fail_before_building() {
    for text in [
        "parts = []".to_string(),
        PARTS.replace("name = \"canopy\"", "name = \"trunk\""),
        PARTS.replace("rotation = [0, 45, 0]", "rotation = [0, nan, 0]"),
        PARTS.replace("role = \"tree_canopies\"", "role = \"unknown\""),
        PARTS.replace("radius = 0.2", "radius = -1"),
        format!("kind = \"box\"\n{PARTS}"),
        format!("typo = true\n{PARTS}"),
    ] {
        assert!(
            Model::parse("tree", &text, Path::new("")).is_err(),
            "{text}"
        );
    }
}

#[test]
fn assembly_prefabs_reference_each_parts_mesh_material_and_transform() {
    let model = Model::parse("tree", PARTS, Path::new("")).unwrap();
    let layout = Layout::new(Path::new("sandbox/assets/generated")).unwrap();
    let plan = layout.plan(&[model]);
    assert_eq!(plan.meshes.len(), 2);
    assert_eq!(plan.prefabs.len(), 2);
    assert_eq!(plan.materials.len(), 4);
    let sunny = &plan.prefabs[0];
    let data = sunny.data(&layout);
    let root = data["root"].as_str().unwrap();
    let children = data["data"]["hierarchy"][root].as_array().unwrap();
    assert_eq!(children.len(), 2);
    for (index, child) in children.iter().enumerate() {
        let object = &data["data"]["components"][child.as_str().unwrap()];
        let mesh = &object["93fd32b1-7127-4c88-8e89-893512af58de"];
        assert_eq!(mesh["mesh"], plan.meshes[index].mesh.id.to_string());
        assert_eq!(mesh["material"], sunny.parts[index].material.id.to_string());
    }
    let canopy = &data["data"]["components"][children[1].as_str().unwrap()];
    let transform = &canopy["c5b3b71f-1f14-4b5b-9881-436118684d29"]["transform"];
    assert_eq!(transform["position"], serde_json::json!([0.0, 1.2, 0.0]));
    assert!(
        (transform["rotation"][1].as_f64().unwrap() - (22.5_f64.to_radians()).sin()).abs() < 1e-12
    );
    // A reordered source keeps every named mesh/object id and per-map material.
    let reordered = PARTS
        .split("[[parts]]")
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|p| format!("[[parts]]{p}"))
        .collect::<String>();
    let second = layout.plan(&[Model::parse("tree", &reordered, Path::new("")).unwrap()]);
    assert_eq!(second.prefabs[0].asset.id, sunny.asset.id);
    assert_eq!(second.meshes[1].mesh.id, plan.meshes[0].mesh.id);
    assert_eq!(
        second.prefabs[0].data(&layout)["data"]["components"],
        data["data"]["components"]
    );
}

#[test]
fn every_new_role_has_two_clay_materials_and_screen_space_ink() {
    for name in [
        "hazard_yellow",
        "hazard_black",
        "crate_frame",
        "crate_panel",
        "grass",
    ] {
        let role = palette::find(name).unwrap();
        for map in palette::Map::ALL {
            let material = role.material(map);
            let ink = material["variables"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == "outline_style")
                .unwrap();
            assert_eq!(ink["offset"], 60);
            assert_eq!(ink["value"]["Uint"], 1);
            assert!(palette::linear_rgb(role.hex(map)).is_some());
        }
    }
}

#[test]
fn a_part_named_root_cannot_overwrite_the_prefab_root() {
    let model = Model::parse(
        "tree",
        &PARTS.replace("name = \"trunk\"", "name = \"root\""),
        Path::new(""),
    )
    .unwrap();
    let layout = Layout::new(Path::new("sandbox/assets/generated")).unwrap();
    let plan = layout.plan(&[model]);
    let data = plan.prefabs[0].data(&layout);
    let root = data["root"].as_str().unwrap();
    assert_eq!(data["data"]["components"].as_object().unwrap().len(), 3);
    for child in data["data"]["hierarchy"][root].as_array().unwrap() {
        assert_ne!(child.as_str().unwrap(), root);
    }
}

#[test]
fn a_flat_part_overrides_model_smoothing_without_inheriting_its_angle() {
    let text = format!(
        "shading = \"smooth\"\nsmooth_angle = 40\n{}",
        PARTS.replace("name = \"canopy\"", "name = \"canopy\"\nshading = \"flat\"")
    );
    let model = Model::parse("tree", &text, Path::new("")).unwrap();
    let Shape::Parts { parts } = model.shape else {
        panic!("assembly")
    };
    assert_eq!(
        parts[0].shading,
        calyx_model_gen::params::Shading::Smooth { angle: 40.0 }
    );
    assert_eq!(parts[1].shading, calyx_model_gen::params::Shading::Flat);
    // Explicitly contradictory keys still fail rather than discarding user input.
    assert!(Model::parse(
        "tree",
        &text.replace(
            "shading = \"flat\"",
            "shading = \"flat\"\nsmooth_angle = 40"
        ),
        Path::new("")
    )
    .is_err());
}
