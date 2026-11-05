//! Generates the tank game's low-poly clay models: one TOML parameter file
//! per model ([`params`]) becomes an OBJ built by headless Blender
//! ([`blender`]), an optional ink outline hull, and the palette materials
//! of all three maps ([`palette`]), all with stable ids ([`layout`]).
//!
//! Sourced models (CC0 or CC-BY) come through the same path as an `import`
//! kind: Blender reads the source, the generator restyles it in a palette
//! role, and [`credits`] refuses one `THIRD_PARTY_ASSETS.md` does not credit.

pub mod blender;
pub mod credits;
pub mod ids;
pub mod layout;
pub mod obj;
pub mod palette;
pub mod params;
pub mod prefab;

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::layout::{Layout, Plan};
use crate::obj::ObjMesh;
use crate::params::{Shading, Shape};

/// One model for Blender to build.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshJob {
    pub name: String,
    /// Where the model's OBJ goes.
    pub obj: PathBuf,
    /// Where its outline hull's OBJ goes, when it has one, and how thick.
    pub outline_obj: Option<PathBuf>,
    pub outline: Option<f64>,
    pub shading: Shading,
    pub shape: Shape,
}

/// The Blender jobs of `plan`.
pub fn jobs(plan: &Plan) -> Vec<MeshJob> {
    plan.meshes
        .iter()
        .map(|mesh| MeshJob {
            name: mesh.model.name.clone(),
            obj: mesh.mesh.path.clone(),
            outline_obj: mesh.outline.as_ref().map(|o| o.path.clone()),
            outline: mesh.model.outline,
            shading: mesh.model.shading,
            shape: mesh.model.shape.clone(),
        })
        .collect()
}

/// What a run wrote and removed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub models: usize,
    pub outlines: usize,
    pub materials: usize,
    pub removed: Vec<PathBuf>,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "generated {} models, {} outline hulls and {} materials",
            self.models, self.outlines, self.materials
        )?;
        for path in &self.removed {
            write!(f, "\nremoved stale {}", path.display())?;
        }
        Ok(())
    }
}

/// The files directly in `dirs`.
fn files_in(dirs: &[PathBuf]) -> BTreeSet<PathBuf> {
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect()
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).expect("JSON values serialize")
}

/// How far a generated mesh may stray from its expected bounds: Blender
/// writes positions to five decimals.
const TOLERANCE: f64 = 2e-3;

/// The width, height and depth a shape's mesh must span, as a range per
/// axis: a cylinder's polygon spans less than its diameter on one axis.
fn expected_extent(shape: &Shape) -> Option<[(f64, f64); 3]> {
    match shape {
        Shape::Box {
            size, band, foot, ..
        } => {
            let overhang = [band, foot]
                .iter()
                .filter_map(|b| b.map(|b| b.overhang))
                .fold(0.0, f64::max);
            let w = size[0] + 2.0 * overhang;
            let d = size[2] + 2.0 * overhang;
            Some([(w, w), (size[1], size[1]), (d, d)])
        }
        Shape::Cylinder {
            radius,
            top_radius,
            height,
            vertices,
            flare,
            ..
        } => {
            if top_radius.is_some() {
                return None;
            }
            let r = flare.map_or(*radius, |f| f.radius.max(*radius));
            let low = 2.0 * r * (std::f64::consts::PI / *vertices as f64).cos();
            let height = flare.map_or(*height, |f| f.height.max(*height));
            Some([(low, 2.0 * r), (height, height), (low, 2.0 * r)])
        }
        Shape::Ring {
            radius,
            height,
            vertices,
            ..
        } => {
            let low = 2.0 * radius * (std::f64::consts::PI / *vertices as f64).cos();
            Some([(low, 2.0 * radius), (*height, *height), (low, 2.0 * radius)])
        }
        _ => None,
    }
}

/// Checks the OBJ Blender wrote for `job`: its triangles face out, its
/// normals match its shading, it stands on the ground and spans its size.
pub fn check_mesh(job: &MeshJob, text: &str) -> Result<(), String> {
    let mesh = ObjMesh::parse(text)?;
    mesh.check_normals(job.shading == Shading::Flat)?;
    if mesh.signed_volume() <= 0.0 {
        return Err("its triangles face inwards".into());
    }
    let (low, high) = mesh.bounds();
    if !matches!(job.shape, Shape::Blob { .. }) && low[1].abs() > TOLERANCE {
        return Err(format!("its base is at y = {}, not 0", low[1]));
    }
    if let Some(extent) = expected_extent(&job.shape) {
        for (axis, (min, max)) in extent.iter().enumerate() {
            let span = high[axis] - low[axis];
            if span < min - TOLERANCE || span > max + TOLERANCE {
                return Err(format!(
                    "it spans {span} on {}, expected {min} to {max}",
                    ["x", "y", "z"][axis]
                ));
            }
        }
    }
    Ok(())
}

/// Checks an outline hull: inverted (it faces inwards, so back-face culling
/// leaves only its rim) and faceted.
pub fn check_hull(text: &str) -> Result<(), String> {
    let mesh = ObjMesh::parse(text)?;
    mesh.check_normals(true)?;
    if mesh.signed_volume() >= 0.0 {
        return Err("the hull is not inverted".into());
    }
    Ok(())
}

/// Generates `models` into `layout`: `build` writes each job's OBJ files
/// (headless Blender, through [`blender::build`]); the generator checks
/// them, writes the materials and every `.meta`, and removes the files an
/// earlier run wrote that this one no longer does.
///
/// Each meta is written once its asset exists, so an open editor never
/// sees a meta without its file.
pub fn generate(
    layout: &Layout,
    models: &[params::Model],
    build: impl FnOnce(&[MeshJob]) -> Result<(), String>,
) -> Result<Report, String> {
    let plan = layout.plan(models);
    let mut mesh_names = BTreeSet::new();
    for mesh in &plan.meshes {
        if !mesh_names.insert(&mesh.mesh.name) {
            return Err(format!(
                "models produce the same mesh name: {}",
                mesh.mesh.name
            ));
        }
    }
    let before = files_in(&layout.owned_dirs());
    for dir in layout.owned_dirs() {
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    }
    let jobs = jobs(&plan);
    build(&jobs)?;
    for job in &jobs {
        let read = |path: &Path| {
            std::fs::read_to_string(path)
                .map_err(|err| format!("{}: Blender wrote no {}: {err}", job.name, path.display()))
        };
        check_mesh(job, &read(&job.obj)?).map_err(|err| format!("{}: {err}", job.obj.display()))?;
        if let Some(hull) = &job.outline_obj {
            check_hull(&read(hull)?).map_err(|err| format!("{}: {err}", hull.display()))?;
        }
    }
    for material in &plan.materials {
        write(
            &material.asset.path,
            &pretty(&material.role.material(material.map)),
        )?;
    }
    for prefab in &plan.prefabs {
        write(&prefab.asset.path, &pretty(&prefab.data(layout)))?;
    }
    let assets = plan
        .meshes
        .iter()
        .flat_map(|m| std::iter::once(&m.mesh).chain(m.outline.as_ref()))
        .chain(plan.materials.iter().map(|m| &m.asset))
        .chain(plan.prefabs.iter().map(|p| &p.asset));
    for asset in assets {
        write(&asset.meta_path(), &asset.meta())?;
    }
    write(
        &layout.root.join("README.md"),
        "Generated by `cargo run --release -p calyx_model_gen` from the parameter files in\n\
         `sandbox/asset_sources/models/`. Edit those and regenerate; edits here are overwritten.\n",
    )?;
    let written = plan.files();
    let removed: Vec<PathBuf> = before.difference(&written).cloned().collect();
    for path in &removed {
        std::fs::remove_file(path)
            .map_err(|err| format!("cannot remove {}: {err}", path.display()))?;
    }
    Ok(Report {
        models: models.len(),
        outlines: plan.meshes.iter().filter(|m| m.outline.is_some()).count(),
        materials: plan.materials.len(),
        removed,
    })
}
