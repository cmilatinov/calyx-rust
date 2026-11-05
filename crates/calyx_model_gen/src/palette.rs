//! The tank game's palette roles for its three maps, as
//! `docs/design/art/tank-game-direction.md` lists them, and the clay
//! materials made from them.

use serde_json::{json, Value};

/// Calyx's PBR shader (`shaders/mesh/pbr`).
pub const PBR_SHADER: &str = "869cbd32-a789-f19d-3e24-5207f558d1c2";

/// One of the direction's map palettes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Map {
    SunnyBattlefield,
    Sunbaked,
    Moonlit,
}

impl Map {
    pub const ALL: [Map; 3] = [Map::SunnyBattlefield, Map::Sunbaked, Map::Moonlit];

    /// The folder the map's materials go in.
    pub fn folder(self) -> &'static str {
        match self {
            Map::SunnyBattlefield => "sunny",
            Map::Sunbaked => "sunbaked",
            Map::Moonlit => "moonlit",
        }
    }
}

/// A palette role: what a surface is, which gives its colour on each map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Role {
    /// The name a parameter file gives it, and its material's file name.
    pub name: &'static str,
    /// Its hex on Sunny Battlefield, Sunbaked and Moonlit, as it should appear.
    pub sunny: &'static str,
    pub sunbaked: &'static str,
    pub moonlit: &'static str,
    pub roughness: f64,
    /// Emission strength on Sunny Battlefield, Sunbaked and Moonlit; the role's
    /// colour is emitted. Zero for every role but the gameplay signals.
    pub emission: (f64, f64, f64),
}

const fn role(
    name: &'static str,
    sunny: &'static str,
    sunbaked: &'static str,
    moonlit: &'static str,
    roughness: f64,
) -> Role {
    Role {
        name,
        sunny,
        sunbaked,
        moonlit,
        roughness,
        emission: (0.0, 0.0, 0.0),
    }
}

/// The direction's map palette roles.
pub const ROLES: &[Role] = &[
    role("ground", "#87BC6C", "#B9AD97", "#44525B", 1.0),
    role("road", "#D0C292", "#B1A289", "#657179", 1.0),
    role("ground_dirt", "#A2855E", "#968267", "#4C5D62", 1.0),
    role("ground_paving", "#BFB9AE", "#C0BCB6", "#9FA7AF", 1.0),
    role("ground_asphalt", "#53565A", "#5C5B55", "#35434D", 1.0),
    role("ground_marking", "#F3C84B", "#E6B84C", "#CDA559", 1.0),
    role("walls", "#D6B275", "#AAA59D", "#5E6F81", 0.95),
    role("barriers", "#BFB9AE", "#C0BCB6", "#9FA7AF", 0.95),
    role("crates", "#A2855E", "#66635F", "#6B8086", 0.95),
    role("crate_frame", "#986128", "#79572E", "#465D65", 0.95),
    role("crate_panel", "#C28B3C", "#B99863", "#6B8086", 0.95),
    role("crate_plank", "#B57B32", "#A78955", "#5D747B", 0.95),
    role("crate_brace", "#D9AB5B", "#D0B17A", "#8FA4A7", 0.95),
    role("metal", "#454948", "#454745", "#3D4348", 0.9),
    role("metal_caps", "#A4A5A0", "#AAA89D", "#A8B5BD", 0.7),
    role("flag_red", "#D95C38", "#C65A39", "#CE604E", 0.95),
    role("berries", "#D9402F", "#B94B35", "#A64F45", 0.9),
    role("sandbags", "#CBA965", "#B7A681", "#718080", 0.95),
    role("sandbag_seams", "#AA874C", "#98896B", "#54646A", 0.95),
    role("hazard_yellow", "#F3C84B", "#E6B84C", "#CDA559", 0.95),
    role("hazard_black", "#303038", "#303030", "#30373F", 0.95),
    role("grass", "#83AC38", "#9AAB61", "#507165", 0.95),
    role("rocks", "#B4B5AA", "#A29F96", "#848C96", 0.95),
    role("tree_trunks", "#A2855E", "#8F7960", "#4C4541", 0.95),
    role("tree_canopies", "#83B942", "#A4B479", "#3E6564", 0.95),
    role("tree_canopies_dark", "#5F8E2C", "#7F8C57", "#2E4D4C", 0.95),
    role("tree_canopies_light", "#AACF58", "#C1C989", "#62817B", 0.95),
    role("tracks", "#6C6C74", "#66635F", "#3D4348", 0.9),
    role("road_wheels", "#D4D2CD", "#C0BCB6", "#9FA7AF", 0.9),
    Role {
        name: "signals",
        sunny: "#FFB000",
        sunbaked: "#FFB547",
        moonlit: "#FF9330",
        roughness: 0.4,
        emission: (30.0, 40.0, 4.0),
    },
    Role {
        name: "pickup_yellow",
        sunny: "#20D45B",
        sunbaked: "#2FC65D",
        moonlit: "#24DE8C",
        roughness: 0.35,
        emission: (12.0, 12.0, 6.0),
    },
    Role {
        name: "pickup_blue",
        sunny: "#168FFF",
        sunbaked: "#2696F0",
        moonlit: "#249FFF",
        roughness: 0.35,
        emission: (24.0, 24.0, 16.0),
    },
    role("ink", "#121212", "#121212", "#121212", 1.0),
];

/// The role outline hulls are drawn in.
pub const INK: &str = "ink";

/// The role called `name`.
pub fn find(name: &str) -> Option<&'static Role> {
    ROLES.iter().find(|role| role.name == name)
}

impl Role {
    pub fn hex(&self, map: Map) -> &'static str {
        match map {
            Map::SunnyBattlefield => self.sunny,
            Map::Sunbaked => self.sunbaked,
            Map::Moonlit => self.moonlit,
        }
    }

    pub fn emission_strength(&self, map: Map) -> f64 {
        match map {
            Map::SunnyBattlefield => self.emission.0,
            Map::Sunbaked => self.emission.1,
            Map::Moonlit => self.emission.2,
        }
    }

    /// The role's `.cxmat` on `map`: a flat clay PBR material whose colour
    /// is the hex sRGB-decoded, so it renders as the hex (the direction's
    /// colour convention).
    pub fn material(&self, map: Map) -> Value {
        let [r, g, b] = linear_rgb(self.hex(map)).expect("the palette's hexes are valid");
        let color = json!([round5(r), round5(g), round5(b), 1.0]);
        let strength = self.emission_strength(map);
        let emission_color = if strength > 0.0 {
            color.clone()
        } else {
            json!([1.0, 1.0, 1.0, 1.0])
        };
        let uniform = |name: &str, offset: u32, span: u32, value: Value| json!({"group": 3, "binding": 2, "offset": offset, "name": name, "span": span, "value": value});
        json!({
            "shader": PBR_SHADER,
            "variables": [
                {"group": 3, "binding": 0, "offset": null, "name": "base_color_texture", "span": null,
                 "value": {"Texture2D": {"Color": [1.0, 1.0, 1.0, 1.0]}}},
                uniform("metallic", 0, 4, json!({"Float": 0.0})),
                uniform("roughness", 4, 4, json!({"Float": self.roughness})),
                uniform("ambient_occlusion", 8, 4, json!({"Float": 1.0})),
                uniform("emission_strength", 12, 4, json!({"Float": strength})),
                uniform("emission_color", 16, 16, json!({"Color": emission_color})),
                uniform("base_color_factor", 32, 16, json!({"Color": color})),
                uniform("outline_style", 60, 4, json!({"Uint": if self.name.starts_with("ground_") || matches!(self.name, "ground" | "road" | "signals" | "pickup_yellow" | "pickup_blue" | "ink") { 0 } else { 1 }})),
            ]
        })
    }
}

fn round5(value: f64) -> f64 {
    (value * 1e5).round() / 1e5
}

/// An sRGB channel in linear space.
pub fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// `#RRGGBB` decoded from sRGB to linear RGB.
pub fn linear_rgb(hex: &str) -> Option<[f64; 3]> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([
        srgb_to_linear(channel(0)? as f64 / 255.0),
        srgb_to_linear(channel(2)? as f64 / 255.0),
        srgb_to_linear(channel(4)? as f64 / 255.0),
    ])
}

#[cfg(test)]
mod tests {
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
}
