//! Running Blender headless to convert FBX models to OBJ and TGA textures to
//! PNG, and checking its output.
//!
//! The conversion itself is `blender_convert.py`, which has to be Python to
//! drive Blender's API; it is embedded here and written next to the job.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::json::Json;
use crate::yaml;

/// The Blender-side conversion script.
const SCRIPT: &str = include_str!("blender_convert.py");

/// Unity's `ModelImporterNormals` values, as `normalImportMode` stores them.
pub const NORMALS_IMPORT: i64 = 0;
pub const NORMALS_CALCULATE: i64 = 1;
pub const NORMALS_NONE: i64 = 2;

/// One model to convert, with its normal import settings.
pub struct MeshJob {
    pub fbx: PathBuf,
    pub obj: PathBuf,
    pub normal_mode: i64,
    pub smooth_angle: f64,
}

/// One texture to convert.
pub struct TextureJob {
    pub tga: PathBuf,
    pub png: PathBuf,
}

/// `(mode, smoothing angle)` of a model's normals from its `.meta`. None
/// becomes Calculate, as Unity does for a lit shader; a meta without the
/// settings gets Unity's defaults, Import and 60 degrees.
pub fn normal_settings(meta: &str) -> (i64, f64) {
    let meta = yaml::parse_meta(meta);
    let tangent_space = meta
        .get("ModelImporter")
        .and_then(|m| m.get("tangentSpace"));
    let mode = tangent_space
        .and_then(|t| t.i64_at("normalImportMode"))
        .unwrap_or(NORMALS_IMPORT);
    let angle = tangent_space
        .and_then(|t| t.f64_at("normalSmoothAngle"))
        .unwrap_or(60.0);
    (
        if mode == NORMALS_NONE {
            NORMALS_CALCULATE
        } else {
            mode
        },
        angle,
    )
}

fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

/// Runs `blender` on the jobs, using `work` for the script and job file.
pub fn convert(
    blender: &Path,
    work: &Path,
    meshes: &[MeshJob],
    textures: &[TextureJob],
) -> Result<(), String> {
    let script = work.join("blender_convert.py");
    std::fs::write(&script, SCRIPT)
        .map_err(|err| format!("cannot write {}: {err}", script.display()))?;
    let job = Json::object([
        (
            "meshes",
            Json::Array(
                meshes
                    .iter()
                    .map(|m| {
                        Json::Array(vec![
                            Json::Str(absolute(&m.fbx)),
                            Json::Str(absolute(&m.obj)),
                            Json::Int(m.normal_mode),
                            Json::Float(m.smooth_angle),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "textures",
            Json::Array(
                textures
                    .iter()
                    .map(|t| {
                        Json::Array(vec![
                            Json::Str(absolute(&t.tga)),
                            Json::Str(absolute(&t.png)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]);
    let job_path = work.join("job.json");
    std::fs::write(&job_path, job.compact())
        .map_err(|err| format!("cannot write {}: {err}", job_path.display()))?;
    let output = Command::new(blender)
        .args(["--background", "--factory-startup", "--python"])
        .arg(&script)
        .arg("--")
        .arg(std::path::absolute(&job_path).unwrap_or(job_path.clone()))
        .output()
        .map_err(|err| format!("cannot run Blender at {}: {err}", blender.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let converted = stdout
        .lines()
        .filter(|l| l.starts_with("converted "))
        .count();
    if !output.status.success() || converted != meshes.len() + textures.len() {
        let tail = |text: &str| text[text.len().saturating_sub(4000)..].to_string();
        return Err(format!(
            "Blender conversion failed\n{}{}",
            tail(&stdout),
            tail(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    Ok(())
}

/// Checks that the OBJ text is faceted the way a model whose normals Unity
/// calculates with a smoothing angle of 0 is: every triangle's corners carry
/// the triangle's own face normal, facing the way its (counter-clockwise)
/// winding does. A smooth-shaded mesh, or one whose normals point against
/// its winding, fails.
pub fn check_faceted(obj: &str) -> Result<(), String> {
    let mut positions: Vec<[f64; 3]> = Vec::new();
    let mut normals: Vec<[f64; 3]> = Vec::new();
    let parse3 = |parts: &[&str]| -> Result<[f64; 3], String> {
        let mut out = [0.0; 3];
        for (slot, part) in out.iter_mut().zip(parts) {
            *slot = part.parse().map_err(|_| format!("bad number {part}"))?;
        }
        Ok(out)
    };
    for line in obj.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.first() {
            Some(&"v") => positions.push(parse3(&parts[1..])?),
            Some(&"vn") => normals.push(parse3(&parts[1..])?),
            Some(&"f") => {
                let corners: Vec<Vec<&str>> =
                    parts[1..].iter().map(|c| c.split('/').collect()).collect();
                let index = |text: &str| -> Result<usize, String> {
                    text.parse::<usize>()
                        .map(|i| i - 1)
                        .map_err(|_| format!("bad index {text}"))
                };
                let corner = |k: usize| -> Result<[f64; 3], String> {
                    positions
                        .get(index(corners[k][0])?)
                        .copied()
                        .ok_or_else(|| "a face uses a missing position".to_string())
                };
                let (a, b, c) = (corner(0)?, corner(1)?, corner(2)?);
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let face = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let length = (face[0] * face[0] + face[1] * face[1] + face[2] * face[2]).sqrt();
                if length < 1e-9 {
                    continue; // a degenerate triangle has no face normal to compare
                }
                for corner in &corners {
                    let n = corner
                        .get(2)
                        .ok_or("a face corner has no normal")
                        .and_then(|i| index(i).map_err(|_| "a face corner has no normal"))
                        .ok()
                        .and_then(|i| normals.get(i))
                        .ok_or("a face corner has no normal")?;
                    let dot = (face[0] * n[0] + face[1] * n[1] + face[2] * n[2]) / length;
                    if dot < 0.999 {
                        return Err("a corner normal is not its triangle's face normal".into());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
