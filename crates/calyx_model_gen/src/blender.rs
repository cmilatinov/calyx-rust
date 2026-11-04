//! Running Blender headless to build the models.
//!
//! The modelling itself is `blender_models.py`, which has to be Python to
//! drive Blender's API; it is embedded here and written next to the job, as
//! `calyx_unity_import` does with its conversion script.

use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::MeshJob;

/// The Blender-side modelling script.
pub const SCRIPT: &str = include_str!("blender_models.py");

fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

/// The job file the script reads.
pub fn job(jobs: &[MeshJob]) -> serde_json::Value {
    let models: Vec<_> = jobs
        .iter()
        .map(|job| {
            let mut shape = serde_json::to_value(&job.shape).expect("shapes serialize");
            if let Some(source) = shape.get_mut("source") {
                let path = source.as_str().unwrap_or_default().to_string();
                *source = json!(absolute(Path::new(&path)));
            }
            json!({
                "name": job.name,
                "obj": absolute(&job.obj),
                "outline_obj": job.outline_obj.as_deref().map(absolute),
                "outline": job.outline,
                "shading": job.shading,
                "shape": shape,
            })
        })
        .collect();
    json!({ "models": models })
}

/// Runs `blender` on the jobs, using `work` for the script and job file.
pub fn build(blender: &Path, work: &Path, jobs: &[MeshJob]) -> Result<(), String> {
    std::fs::create_dir_all(work)
        .map_err(|err| format!("cannot create {}: {err}", work.display()))?;
    let script = work.join("blender_models.py");
    std::fs::write(&script, SCRIPT)
        .map_err(|err| format!("cannot write {}: {err}", script.display()))?;
    let job_path = work.join("job.json");
    std::fs::write(&job_path, job(jobs).to_string())
        .map_err(|err| format!("cannot write {}: {err}", job_path.display()))?;
    let output = Command::new(blender)
        .args(["--background", "--factory-startup", "--python"])
        .arg(&script)
        .arg("--")
        .arg(absolute(&job_path))
        .output()
        .map_err(|err| format!("cannot run Blender at {}: {err}", blender.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let built = stdout
        .lines()
        .filter(|l| l.starts_with("generated "))
        .count();
    if !output.status.success() || built != jobs.len() {
        let tail = |text: &str| {
            let start = text.len().saturating_sub(4000);
            let start = (start..text.len())
                .find(|i| text.is_char_boundary(*i))
                .unwrap_or(text.len());
            text[start..].to_string()
        };
        return Err(format!(
            "Blender built {built} of {} models\n{}{}",
            jobs.len(),
            tail(&stdout),
            tail(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    Ok(())
}
