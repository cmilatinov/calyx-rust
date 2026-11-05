//! Model parameter files: one TOML file per model, named for the model.
//!
//! ```toml
//! # crate.toml
//! kind = "box"            # box, rock, cylinder, blob, prism or import
//! role = "crates"         # a palette role; gives the model's material
//! shading = "smooth"      # "smooth" (the default) or "flat"
//! smooth_angle = 60       # degrees; smooth shading only
//! outline = 0.07          # optional ink hull thickness in metres
//!
//! size = [1.1, 1.1, 1.1]  # width (x), height (y), depth (z) in metres
//! bevel = 0.12
//! segments = 4
//! band = { height = 0.18, overhang = 0.04 }
//! ```
//!
//! The common keys come first; the rest belong to the kind (see [`Shape`]).
//! Every model stands on the ground: its base is at y = 0, centred on x
//! and z, in Calyx's left-handed, Y-up space.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::palette;

/// The licences a sourced model may carry (SPDX ids): the direction's
/// sourcing rule allows CC0 and CC-BY only.
pub const ALLOWED_LICENCES: &[&str] = &["CC0-1.0", "CC-BY-3.0", "CC-BY-4.0"];

/// The source formats an `import` model can read.
pub const IMPORT_FORMATS: &[&str] = &["glb", "gltf", "obj", "fbx"];

/// One model to generate.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// The parameter file's stem; names the model's files.
    pub name: String,
    pub role: String,
    pub shading: Shading,
    /// The ink hull's thickness, when the model has one.
    pub outline: Option<f64>,
    pub shape: Shape,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Shading {
    /// Each corner's normal averages the faces around it that lie within
    /// `angle` degrees of its own face: soft clay with crisp creases.
    Smooth { angle: f64 },
    /// Each triangle carries its face normal: the faceted low-poly look.
    Flat,
}

/// What the model is made of. Sizes are metres.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    /// Named pieces, assembled by a generated prefab per map. Transforms
    /// are local to the model root; meshes keep their ordinary ground origin.
    Parts { parts: Vec<Part> },
    /// A closed footprint extruded upwards. Used for clipped paint stripes,
    /// Jersey barrier sections and matching modular wall junctions.
    Prism {
        points: Vec<[f64; 2]>,
        height: f64,
        #[serde(default)]
        bevel: f64,
        #[serde(default = "default_segments")]
        segments: u32,
    },
    /// A rounded block: a crate, a barrier, a wall.
    Box {
        size: [f64; 3],
        #[serde(default)]
        bevel: f64,
        #[serde(default = "default_segments")]
        segments: u32,
        /// A raised band round the middle, as on a crate.
        band: Option<Band>,
        /// A wider slab at the bottom, as under a barrier.
        foot: Option<Band>,
    },
    /// A lumpy squashed sphere.
    Rock {
        size: [f64; 3],
        /// How far each vertex moves in or out, as a fraction of the radius.
        #[serde(default = "default_jitter")]
        jitter: f64,
        #[serde(default)]
        seed: u32,
        #[serde(default = "default_subdivisions")]
        subdivisions: u32,
    },
    /// An upright rounded cylinder: a trunk, a post.
    Cylinder {
        radius: f64,
        #[serde(default)]
        top_radius: Option<f64>,
        height: f64,
        #[serde(default = "default_vertices")]
        vertices: u32,
        #[serde(default)]
        bevel: f64,
        #[serde(default = "default_segments")]
        segments: u32,
        /// A wider disc at the bottom, as a trunk's roots.
        flare: Option<Flare>,
    },
    /// Overlapping spheres: a tree canopy, a bush.
    Blob {
        lobes: Vec<Lobe>,
        /// Voxel size for joining overlapping lobes into one organic surface.
        /// Zero retains the original independent-lobe output.
        #[serde(default)]
        fuse: f64,
        #[serde(default = "default_subdivisions")]
        subdivisions: u32,
    },
    /// A sourced model, restyled: its geometry only, scaled, turned and
    /// optionally bevelled, in the role's material.
    Import {
        /// The source file, relative to the parameter file.
        source: PathBuf,
        #[serde(default = "default_scale")]
        scale: f64,
        /// Degrees about the vertical axis.
        #[serde(default)]
        rotate_y: f64,
        #[serde(default)]
        bevel: f64,
        #[serde(default = "default_import_segments")]
        segments: u32,
        credit: Credit,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Part {
    pub name: String,
    pub role: String,
    pub shape: Shape,
    pub shading: Shading,
    pub offset: [f64; 3],
    /// XYZ Euler angles, in degrees, matching the editor's transform command.
    pub rotation: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    pub height: f64,
    /// How far it stands out from the block on each side.
    pub overhang: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flare {
    pub radius: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Lobe {
    /// The sphere's centre, x, y (up) and z.
    pub center: [f64; 3],
    pub radius: f64,
}

/// Where a sourced model comes from, as `THIRD_PARTY_ASSETS.md` records it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Credit {
    pub title: String,
    pub author: String,
    /// An SPDX id from [`ALLOWED_LICENCES`].
    pub licence: String,
    pub url: String,
}

fn default_segments() -> u32 {
    4
}
fn default_import_segments() -> u32 {
    2
}
fn default_jitter() -> f64 {
    0.1
}
fn default_subdivisions() -> u32 {
    2
}
fn default_vertices() -> u32 {
    16
}
fn default_scale() -> f64 {
    1.0
}

/// The keys every kind shares.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Common {
    role: String,
    #[serde(default)]
    shading: Option<String>,
    smooth_angle: Option<f64>,
    outline: Option<f64>,
}

const COMMON_KEYS: &[&str] = &["role", "shading", "smooth_angle", "outline"];

/// The default smoothing angle: bevels read soft, creases between faces
/// more than this apart stay sharp.
pub const DEFAULT_SMOOTH_ANGLE: f64 = 60.0;

impl Model {
    /// Parses the parameter file `text` of the model `name`. Paths in it are
    /// relative to `dir`, the file's folder.
    pub fn parse(name: &str, text: &str, dir: &Path) -> Result<Model, String> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(format!(
                "model names use lowercase letters, digits and underscores, not {name:?}"
            ));
        }
        let mut table: toml::Table = toml::from_str(text).map_err(|err| err.to_string())?;
        if let Some(parts) = table.remove("parts") {
            for key in table.keys() {
                if !["shading", "smooth_angle", "outline"].contains(&key.as_str()) {
                    return Err(format!(
                        "unknown assembly key {key:?}; shapes and roles belong to each part"
                    ));
                }
            }
            let parts = parts.as_array().ok_or("parts must be an array of tables")?;
            if parts.is_empty() {
                return Err("a model needs at least one part".into());
            }
            let mut names = std::collections::BTreeSet::new();
            let mut parsed = Vec::new();
            for part in parts {
                let mut part = part.as_table().ok_or("each part must be a table")?.clone();
                let name = part
                    .remove("name")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .ok_or("each part needs a stable name")?;
                if !names.insert(name.clone()) {
                    return Err(format!("duplicate part name {name:?}"));
                }
                let vector = |table: &mut toml::Table, key: &str| -> Result<[f64; 3], String> {
                    match table.remove(key) {
                        None => Ok([0.0; 3]),
                        Some(value) => {
                            let values: [f64; 3] = value
                                .try_into()
                                .map_err(|e: toml::de::Error| e.to_string())?;
                            if values.iter().all(|v| v.is_finite()) {
                                Ok(values)
                            } else {
                                Err(format!("{key} must be finite"))
                            }
                        }
                    }
                };
                let offset = vector(&mut part, "offset")?;
                let rotation = vector(&mut part, "rotation")?;
                for key in ["shading", "smooth_angle"] {
                    if key == "smooth_angle"
                        && part.get("shading").and_then(toml::Value::as_str) == Some("flat")
                    {
                        continue;
                    }
                    if !part.contains_key(key) {
                        if let Some(value) = table.get(key) {
                            part.insert(key.into(), value.clone());
                        }
                    }
                }
                if part.contains_key("parts") || part.contains_key("outline") {
                    return Err(
                        "parts cannot nest or set an outline; set outline on the model".into(),
                    );
                }
                let model = Model::parse(
                    &name,
                    &toml::to_string(&part).map_err(|e| e.to_string())?,
                    dir,
                )?;
                if matches!(model.shape, Shape::Import { .. } | Shape::Parts { .. }) {
                    return Err("parts use box, rock, cylinder, blob or prism shapes".into());
                }
                parsed.push(Part {
                    name,
                    role: model.role,
                    shape: model.shape,
                    shading: model.shading,
                    offset,
                    rotation,
                });
            }
            // Reuse common-key validation, rejecting kind/role and unknown keys
            // on an assembly rather than silently ignoring them.
            table.insert("kind".into(), "box".into());
            table.insert("role".into(), parsed[0].role.clone().into());
            table.insert("size".into(), toml::Value::Array(vec![1.into(); 3]));
            let mut model = Model::parse(
                name,
                &toml::to_string(&table).map_err(|e| e.to_string())?,
                dir,
            )?;
            model.shape = Shape::Parts { parts: parsed };
            return Ok(model);
        }
        let mut common = toml::Table::new();
        for key in COMMON_KEYS {
            if let Some(value) = table.remove(*key) {
                common.insert(key.to_string(), value);
            }
        }
        let common: Common = common
            .try_into()
            .map_err(|err: toml::de::Error| err.to_string())?;
        let mut shape: Shape = table
            .try_into()
            .map_err(|err: toml::de::Error| err.to_string())?;
        if let Shape::Import { source, .. } = &mut shape {
            *source = dir.join(&*source);
        }
        let shading = match (common.shading.as_deref(), common.smooth_angle) {
            (None | Some("smooth"), angle) => Shading::Smooth {
                angle: angle.unwrap_or(DEFAULT_SMOOTH_ANGLE),
            },
            (Some("flat"), None) => Shading::Flat,
            (Some("flat"), Some(_)) => {
                return Err("smooth_angle applies to smooth shading only".into())
            }
            (Some(other), _) => {
                return Err(format!("shading is \"smooth\" or \"flat\", not {other:?}"))
            }
        };
        let model = Model {
            name: name.to_string(),
            role: common.role,
            shading,
            outline: common.outline,
            shape,
        };
        model.validate()?;
        Ok(model)
    }

    /// Reads the parameter file at `path`.
    pub fn load(path: &Path) -> Result<Model, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let dir = path.parent().unwrap_or(Path::new(""));
        Model::parse(name, &text, dir).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The role's palette entry.
    pub fn palette_role(&self) -> &'static palette::Role {
        palette::find(&self.role).expect("validated")
    }

    fn validate(&self) -> Result<(), String> {
        if palette::find(&self.role).is_none() {
            let known: Vec<_> = palette::ROLES.iter().map(|r| r.name).collect();
            return Err(format!(
                "unknown role {:?}; the palette's roles are {}",
                self.role,
                known.join(", ")
            ));
        }
        if let Shading::Smooth { angle } = self.shading {
            in_range("smooth_angle", angle, 0.0, 180.0)?;
        }
        if let Some(outline) = self.outline {
            positive("outline", outline)?;
            in_range("outline", outline, 0.0, 0.5)?;
        }
        let segments = |segments: u32| {
            if (1..=16).contains(&segments) {
                Ok(())
            } else {
                Err(format!("segments is 1 to 16, not {segments}"))
            }
        };
        let subdivisions = |subdivisions: u32| {
            if (1..=5).contains(&subdivisions) {
                Ok(())
            } else {
                Err(format!("subdivisions is 1 to 5, not {subdivisions}"))
            }
        };
        match &self.shape {
            Shape::Prism {
                points,
                height,
                bevel,
                segments: s,
            } => {
                positive("height", *height)?;
                segments(*s)?;
                if points.len() < 3 || points.iter().flatten().any(|v| !v.is_finite()) {
                    return Err("a prism needs at least three finite footprint points".into());
                }
                let area: f64 = points
                    .iter()
                    .zip(points.iter().cycle().skip(1))
                    .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
                    .sum();
                if area.abs() < 1e-8 {
                    return Err("prism footprint has zero area".into());
                }
                if !bevel.is_finite() || *bevel < 0.0 || *bevel * 2.0 >= *height {
                    return Err("prism bevel must fit its height".into());
                }
            }
            Shape::Parts { .. } => return Err("list parts without a top-level kind".into()),
            Shape::Box {
                size,
                bevel,
                segments: s,
                band,
                foot,
            } => {
                sizes("size", size)?;
                segments(*s)?;
                let smallest = size.iter().copied().fold(f64::INFINITY, f64::min);
                if *bevel < 0.0 || *bevel * 2.0 >= smallest {
                    return Err(format!(
                        "bevel {bevel} must be at least 0 and under half the smallest size ({})",
                        smallest / 2.0
                    ));
                }
                for (key, band) in [("band", band), ("foot", foot)] {
                    if let Some(band) = band {
                        positive(&format!("{key}.height"), band.height)?;
                        in_range(&format!("{key}.overhang"), band.overhang, 0.0, 1.0)?;
                        if band.height > size[1] {
                            return Err(format!("{key}.height is taller than the box"));
                        }
                    }
                }
            }
            Shape::Rock {
                size,
                jitter,
                subdivisions: d,
                ..
            } => {
                sizes("size", size)?;
                in_range("jitter", *jitter, 0.0, 0.45)?;
                subdivisions(*d)?;
            }
            Shape::Cylinder {
                radius,
                top_radius,
                height,
                vertices,
                bevel,
                segments: s,
                flare,
            } => {
                positive("radius", *radius)?;
                if let Some(radius) = top_radius {
                    positive("top_radius", *radius)?;
                }
                positive("height", *height)?;
                segments(*s)?;
                if !(3..=64).contains(vertices) {
                    return Err(format!("vertices is 3 to 64, not {vertices}"));
                }
                if *bevel < 0.0 || *bevel >= radius.min(height / 2.0) {
                    return Err(format!(
                        "bevel {bevel} must be at least 0 and under the radius and half the height"
                    ));
                }
                if let Some(flare) = flare {
                    positive("flare.radius", flare.radius)?;
                    positive("flare.height", flare.height)?;
                }
            }
            Shape::Blob {
                lobes,
                fuse,
                subdivisions: d,
            } => {
                if lobes.is_empty() {
                    return Err("a blob needs at least one lobe".into());
                }
                in_range("fuse", *fuse, 0.0, 0.5)?;
                for lobe in lobes {
                    positive("lobe radius", lobe.radius)?;
                }
                subdivisions(*d)?;
            }
            Shape::Import {
                source,
                scale,
                bevel,
                segments: s,
                credit,
                ..
            } => {
                positive("scale", *scale)?;
                in_range("bevel", *bevel, 0.0, 1.0)?;
                segments(*s)?;
                let extension = source
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .unwrap_or_default();
                if !IMPORT_FORMATS.contains(&extension.as_str()) {
                    return Err(format!(
                        "source {} is not one of {}",
                        source.display(),
                        IMPORT_FORMATS.join(", ")
                    ));
                }
                if !ALLOWED_LICENCES.contains(&credit.licence.as_str()) {
                    return Err(format!(
                        "licence {:?} is not allowed; sourced models must be {}",
                        credit.licence,
                        ALLOWED_LICENCES.join(", ")
                    ));
                }
                for (key, value) in [("title", &credit.title), ("author", &credit.author)] {
                    if value.trim().is_empty() {
                        return Err(format!("credit.{key} is empty"));
                    }
                }
                if !credit.url.starts_with("https://") {
                    return Err(format!("credit.url {:?} is not an https URL", credit.url));
                }
            }
        }
        Ok(())
    }
}

fn positive(key: &str, value: f64) -> Result<(), String> {
    if value > 0.0 && value.is_finite() {
        Ok(())
    } else {
        Err(format!("{key} must be above 0, not {value}"))
    }
}

fn in_range(key: &str, value: f64, low: f64, high: f64) -> Result<(), String> {
    if (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(format!("{key} is {low} to {high}, not {value}"))
    }
}

fn sizes(key: &str, size: &[f64; 3]) -> Result<(), String> {
    for value in size {
        positive(key, *value)?;
    }
    Ok(())
}

/// Every model in `dir`, one per `.toml` file, sorted by name. Reports every
/// file that fails, not just the first.
pub fn load_dir(dir: &Path) -> Result<Vec<Model>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|err| format!("cannot read {}: {err}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    let mut models = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match Model::load(&path) {
            Ok(model) => models.push(model),
            Err(err) => errors.push(err),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    if models.is_empty() {
        return Err(format!("{} has no model parameter files", dir.display()));
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Model, String> {
        Model::parse("sample", text, Path::new("models"))
    }

    #[test]
    fn a_box_reads_its_size_bevel_role_and_band() {
        let model = parse(
            r#"
kind = "box"
role = "crates"
outline = 0.07
size = [1.1, 1.1, 1.1]
bevel = 0.12
band = { height = 0.18, overhang = 0.04 }
"#,
        )
        .unwrap();
        assert_eq!(
            model,
            Model {
                name: "sample".into(),
                role: "crates".into(),
                shading: Shading::Smooth { angle: 60.0 },
                outline: Some(0.07),
                shape: Shape::Box {
                    size: [1.1, 1.1, 1.1],
                    bevel: 0.12,
                    segments: 4,
                    band: Some(Band {
                        height: 0.18,
                        overhang: 0.04
                    }),
                    foot: None,
                },
            }
        );
    }

    #[test]
    fn shading_is_smooth_with_an_angle_or_flat() {
        let rock = |shading: &str| {
            parse(&format!(
                "kind = \"rock\"\nrole = \"rocks\"\nsize = [1, 1, 1]\n{shading}"
            ))
        };
        assert_eq!(
            rock("smooth_angle = 40").unwrap().shading,
            Shading::Smooth { angle: 40.0 }
        );
        assert_eq!(rock("shading = \"flat\"").unwrap().shading, Shading::Flat);
        assert!(rock("shading = \"flat\"\nsmooth_angle = 40").is_err());
        assert!(rock("shading = \"glossy\"").is_err());
    }

    #[test]
    fn an_unknown_role_lists_the_palettes_roles() {
        let err = parse("kind = \"rock\"\nrole = \"lava\"\nsize = [1, 1, 1]").unwrap_err();
        assert!(err.contains("unknown role \"lava\""), "{err}");
        assert!(err.contains("crates"), "{err}");
    }

    #[test]
    fn unknown_keys_and_kinds_are_errors() {
        // `jitter` belongs to rocks, not boxes.
        assert!(
            parse("kind = \"box\"\nrole = \"crates\"\nsize = [1, 1, 1]\njitter = 0.1").is_err()
        );
        assert!(parse("kind = \"torus\"\nrole = \"crates\"").is_err());
        assert!(
            parse("kind = \"box\"\nrole = \"crates\"\nsize = [1, 1, 1]\ncolour = \"#fff\"")
                .is_err()
        );
    }

    #[test]
    fn a_bevel_must_fit_the_box() {
        let crate_with = |bevel: f64| {
            parse(&format!(
                "kind = \"box\"\nrole = \"crates\"\nsize = [1.0, 0.6, 1.0]\nbevel = {bevel}"
            ))
        };
        assert!(crate_with(0.29).is_ok());
        assert!(crate_with(0.3).is_err());
        assert!(crate_with(-0.1).is_err());
    }

    const IMPORT: &str = r#"
kind = "import"
role = "barriers"
source = "../sources/wall-low.glb"
scale = 1.15
credit = { title = "Wall (low)", author = "Kenney", licence = "LICENCE", url = "https://example.com/wall" }
"#;

    #[test]
    fn an_import_resolves_its_source_beside_the_file() {
        let model = parse(&IMPORT.replace("LICENCE", "CC0-1.0")).unwrap();
        let Shape::Import { source, scale, .. } = &model.shape else {
            panic!("{model:?}")
        };
        assert_eq!(source, &Path::new("models").join("../sources/wall-low.glb"));
        assert_eq!(*scale, 1.15);
    }

    #[test]
    fn an_import_must_be_cc0_or_cc_by() {
        assert!(parse(&IMPORT.replace("LICENCE", "CC-BY-4.0")).is_ok());
        let err = parse(&IMPORT.replace("LICENCE", "CC-BY-NC-4.0")).unwrap_err();
        assert!(err.contains("not allowed"), "{err}");
        // A credit without a licence fails to parse.
        assert!(parse(&IMPORT.replace("licence = \"LICENCE\", ", "")).is_err());
    }

    #[test]
    fn model_names_are_lowercase_identifiers() {
        let text = "kind = \"rock\"\nrole = \"rocks\"\nsize = [1, 1, 1]";
        assert!(Model::parse("rock_a2", text, Path::new("")).is_ok());
        assert!(Model::parse("Rock A", text, Path::new("")).is_err());
    }
}
