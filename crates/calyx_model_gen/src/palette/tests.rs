use super::*;

fn variable<'a>(material: &'a Value, name: &str) -> &'a Value {
    &material["variables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("no {name}"))["value"]
}

#[test]
fn a_role_material_stores_its_hex_srgb_decoded() {
    // Sunny Battlefield crates are bark #A2855E: sRGB 162/255, 133/255,
    // 94/255 decode to 0.36131, 0.23455, 0.11193 linear.
    let material = find("crates").unwrap().material(Map::SunnyBattlefield);
    assert_eq!(material["shader"], PBR_SHADER);
    assert_eq!(
        variable(&material, "base_color_factor"),
        &json!({"Color": [0.36131, 0.23455, 0.11193, 1.0]})
    );
    assert_eq!(variable(&material, "roughness"), &json!({"Float": 0.95}));
    assert_eq!(variable(&material, "metallic"), &json!({"Float": 0.0}));
    assert_eq!(
        variable(&material, "emission_strength"),
        &json!({"Float": 0.0})
    );
}

#[test]
fn a_role_takes_each_maps_colour() {
    // Sunbaked crates are gunmetal #66635F.
    let material = find("crates").unwrap().material(Map::Sunbaked);
    assert_eq!(
        variable(&material, "base_color_factor"),
        &json!({"Color": [0.13287, 0.12477, 0.11444, 1.0]})
    );
}

#[test]
fn ground_tiles_never_draw_ink_or_emit_light() {
    for name in [
        "ground",
        "road",
        "ground_dirt",
        "ground_paving",
        "ground_asphalt",
        "ground_marking",
        "ground_grass_dark",
        "ground_grass_light",
        "ground_gravel",
        "ground_field",
        "ground_crop",
        "ground_track",
    ] {
        for map in Map::ALL {
            let material = find(name).unwrap().material(map);
            assert_eq!(
                variable(&material, "outline_style"),
                &json!({"Uint": 0}),
                "{name}"
            );
            assert_eq!(
                variable(&material, "emission_strength"),
                &json!({"Float": 0.0})
            );
            assert_eq!(variable(&material, "roughness"), &json!({"Float": 1.0}));
        }
    }
}

#[test]
fn terrain_and_props_draw_on_their_own_outline_layers() {
    for (names, style) in [
        (&["walls", "kerbs", "creek_bank"][..], TERRAIN_OUTLINE),
        (
            &[
                "barriers",
                "sandbags",
                "crates",
                "crate_frame",
                "explosive_red",
                "rocks",
                "tree_trunks",
                "metal",
                "sentry_armor",
                "tracks",
            ][..],
            PROP_OUTLINE,
        ),
    ] {
        for name in names {
            for map in Map::ALL {
                let material = find(name).unwrap().material(map);
                assert_eq!(
                    variable(&material, "outline_style"),
                    &json!({"Uint": style}),
                    "{name}"
                );
            }
        }
    }
    assert_ne!(TANK_OUTLINE, TERRAIN_OUTLINE);
    assert_ne!(TANK_OUTLINE, PROP_OUTLINE);
}

#[test]
fn kerbs_take_the_barriers_colours() {
    let (kerbs, barriers) = (find("kerbs").unwrap(), find("barriers").unwrap());
    for map in Map::ALL {
        assert_eq!(kerbs.hex(map), barriers.hex(map));
    }
    assert_eq!(kerbs.roughness, barriers.roughness);
}

#[test]
fn signals_emit_their_colour_at_the_maps_strength() {
    let signals = find("signals").unwrap();
    let sunbaked = signals.material(Map::Sunbaked);
    assert_eq!(
        variable(&sunbaked, "emission_strength"),
        &json!({"Float": 40.0})
    );
    assert_eq!(
        variable(&sunbaked, "emission_color"),
        variable(&sunbaked, "base_color_factor")
    );
    assert_eq!(variable(&sunbaked, "roughness"), &json!({"Float": 0.4}));
    assert_eq!(
        variable(
            &signals.material(Map::SunnyBattlefield),
            "emission_strength"
        ),
        &json!({"Float": 30.0})
    );
}

#[test]
fn hexes_must_be_six_digits() {
    assert_eq!(linear_rgb("#FFFFFF"), Some([1.0, 1.0, 1.0]));
    assert_eq!(linear_rgb("FFFFFF"), None);
    assert_eq!(linear_rgb("#FFF"), None);
    assert_eq!(linear_rgb("#GG0000"), None);
}

#[test]
fn tall_grass_uses_the_engines_grass_shader() {
    let meta = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/shaders/mesh/grass.meta"
    ))
    .unwrap();
    let meta: Value = serde_json::from_str(&meta).unwrap();
    assert_eq!(meta["main"]["id"], GRASS_SHADER);
    for name in ["tall_grass", "tall_grass_light"] {
        for map in Map::ALL {
            assert_eq!(find(name).unwrap().material(map)["shader"], GRASS_SHADER);
        }
    }
    assert_eq!(
        find("grass").unwrap().material(Map::Moonlit)["shader"],
        PBR_SHADER
    );
}
