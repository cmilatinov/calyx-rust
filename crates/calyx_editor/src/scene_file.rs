use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use calyx_engine::scene::{Prefab, Scene, SceneSnapshot};
use serde::Serialize;
use sha1::{Digest, Sha1};
use uuid::Uuid;

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct SceneRecovery {
    pub source_file: PathBuf,
    pub autosave_file: PathBuf,
}

#[derive(Debug, Default)]
pub struct SceneSaveState {
    dirty: bool,
    saved_scene_fingerprint: Option<String>,
    generation: u64,
    revision: u64,
    last_autosaved_revision: u64,
    last_edit_at: Option<Instant>,
    last_autosave_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneSaveRevision {
    generation: u64,
    revision: u64,
}

impl SceneSaveState {
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_dirty(&mut self, now: Instant) {
        self.dirty = true;
        self.revision = self.revision.saturating_add(1);
        self.last_edit_at = Some(now);
    }

    pub fn mark_clean(&mut self, scene_fingerprint: Option<String>) {
        self.saved_scene_fingerprint = scene_fingerprint;
        self.clear_dirty();
    }

    pub fn sync_dirty_to_fingerprint(
        &mut self,
        current_scene_fingerprint: Option<String>,
        now: Instant,
    ) {
        let dirty = match (
            self.saved_scene_fingerprint.as_ref(),
            current_scene_fingerprint.as_ref(),
        ) {
            (Some(saved), Some(current)) => saved != current,
            _ => true,
        };

        if dirty {
            self.mark_dirty(now);
        } else if self.dirty {
            self.clear_dirty();
        }
    }

    pub fn current_revision(&self) -> SceneSaveRevision {
        SceneSaveRevision {
            generation: self.generation,
            revision: self.revision,
        }
    }

    pub fn should_commit_autosave(&self, revision: SceneSaveRevision) -> bool {
        self.dirty
            && revision.generation == self.generation
            && revision.revision > self.last_autosaved_revision
    }

    pub fn should_autosave(&self, now: Instant) -> bool {
        if !self.dirty || self.revision == self.last_autosaved_revision {
            return false;
        }
        if self
            .last_edit_at
            .is_some_and(|last_edit| now.duration_since(last_edit) < AUTOSAVE_DELAY)
        {
            return false;
        }
        self.last_autosave_at
            .map(|last_autosave| now.duration_since(last_autosave) >= AUTOSAVE_INTERVAL)
            .unwrap_or(true)
    }

    pub fn mark_autosave_started(&mut self, now: Instant) {
        self.last_autosave_at = Some(now);
    }

    pub fn mark_autosave_finished(&mut self, revision: SceneSaveRevision, success: bool) {
        if success && revision.generation == self.generation {
            self.last_autosaved_revision = self.last_autosaved_revision.max(revision.revision);
        }
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
        self.generation = self.generation.saturating_add(1);
        self.last_autosaved_revision = self.revision;
        self.last_edit_at = None;
        self.last_autosave_at = None;
    }
}

#[derive(Clone)]
pub struct SceneAutosave {
    root: PathBuf,
}

pub struct PreparedSceneAutosave {
    file: PreparedFile,
}

impl PreparedSceneAutosave {
    pub fn autosave_file(&self) -> &Path {
        &self.file.target_file
    }

    pub fn commit(self) -> Result<PathBuf, String> {
        self.file.commit()
    }
}

impl SceneAutosave {
    pub fn new(project_path: &Path) -> Self {
        Self {
            root: autosave_root(project_path),
        }
    }

    pub fn prepare(
        &self,
        source_file: &Path,
        snapshot: &SceneSnapshot,
    ) -> Result<PreparedSceneAutosave, String> {
        let path = self.path_for_scene(source_file);
        prepare_json_file(&path, snapshot, "scene autosave")
            .map(|file| PreparedSceneAutosave { file })
    }

    pub fn write(&self, source_file: &Path, snapshot: &SceneSnapshot) -> Result<PathBuf, String> {
        self.prepare(source_file, snapshot)?.commit()
    }

    pub fn discard(&self, source_file: &Path) -> Result<(), String> {
        let path = self.path_for_scene(source_file);
        if !path.exists() {
            return Ok(());
        }
        fs::remove_file(&path)
            .map_err(|err| format!("failed to remove scene autosave {}: {err}", path.display()))
    }

    pub fn recovery_for_scene(&self, source_file: &Path) -> Option<SceneRecovery> {
        let autosave_file = self.path_for_scene(source_file);
        let autosave_modified = fs::metadata(&autosave_file).ok()?.modified().ok()?;
        let source_modified = fs::metadata(source_file)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        is_newer_recovery(autosave_modified, source_modified).then_some(SceneRecovery {
            source_file: source_file.to_path_buf(),
            autosave_file,
        })
    }

    pub fn path_for_scene(&self, source_file: &Path) -> PathBuf {
        let label = source_file
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("scene");
        self.root.join(format!(
            "{}-{}.cxscene.autosave",
            label,
            path_hash(source_file)
        ))
    }
}

pub fn write_scene_file(path: &Path, scene: &Scene) -> Result<(), String> {
    prepare_json_file(path, scene, "scene file")?
        .commit()
        .map(|_| ())
}

/// Writes `prefab` to `path` the way scenes are written: the whole file
/// replaced at once, in the scene data's deterministic order.
pub fn write_prefab_file(path: &Path, prefab: &Prefab) -> Result<(), String> {
    prepare_json_file(path, prefab, "prefab file")?
        .commit()
        .map(|_| ())
}

struct PreparedFile {
    target_file: PathBuf,
    temp_file: PathBuf,
    committed: bool,
}

impl PreparedFile {
    fn commit(mut self) -> Result<PathBuf, String> {
        replace_file(&self.temp_file, &self.target_file)?;
        self.committed = true;
        Ok(self.target_file.clone())
    }
}

impl Drop for PreparedFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temp_file);
        }
    }
}

fn prepare_json_file<T: Serialize + ?Sized>(
    target_file: &Path,
    value: &T,
    description: &str,
) -> Result<PreparedFile, String> {
    if let Some(parent) = target_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create {description} directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let temp_file = temporary_path(target_file);
    let write_result = (|| {
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_file)
            .map_err(|err| {
                format!(
                    "failed to open {description} temp file {}: {err}",
                    temp_file.display()
                )
            })?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, value).map_err(|err| {
            format!(
                "failed to serialize {description} temp file {}: {err}",
                temp_file.display()
            )
        })?;
        writer.flush().map_err(|err| {
            format!(
                "failed to flush {description} temp file {}: {err}",
                temp_file.display()
            )
        })
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_file);
        return Err(error);
    }

    Ok(PreparedFile {
        target_file: target_file.to_path_buf(),
        temp_file,
        committed: false,
    })
}

fn temporary_path(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty());
    let suffix = Uuid::new_v4();
    let extension = extension
        .map(|extension| format!("{extension}.{suffix}.tmp"))
        .unwrap_or_else(|| format!("{suffix}.tmp"));
    path.with_extension(extension)
}

fn autosave_root(project_path: &Path) -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Calyx")
        .join("Editor")
        .join("autosaves")
        .join(path_hash(project_path))
}

fn replace_file(tmp_path: &Path, path: &Path) -> Result<(), String> {
    match fs::rename(tmp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_error) if path.exists() => {
            let backup_path = temporary_path(path);
            fs::rename(path, &backup_path).map_err(|err| {
                format!(
                    "failed to stage existing file {} for replacement after rename error {rename_error}: {err}",
                    path.display(),
                )
            })?;
            match fs::rename(tmp_path, path) {
                Ok(()) => {
                    let _ = fs::remove_file(backup_path);
                    Ok(())
                }
                Err(err) => {
                    let restore_result = fs::rename(&backup_path, path);
                    Err(match restore_result {
                        Ok(()) => format!(
                            "failed to move replacement file {} to {}: {err}",
                            tmp_path.display(),
                            path.display()
                        ),
                        Err(restore_error) => format!(
                            "failed to move replacement file {} to {}: {err}; also failed to restore backup {}: {restore_error}",
                            tmp_path.display(),
                            path.display(),
                            backup_path.display()
                        ),
                    })
                }
            }
        }
        Err(err) => Err(format!(
            "failed to move replacement file {} to {}: {err}",
            tmp_path.display(),
            path.display()
        )),
    }
}

fn path_hash(path: &Path) -> String {
    let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let normalized = canonical.to_string_lossy().replace('\\', "/");
    let mut hasher = Sha1::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn scene_fingerprint(scene: &Scene) -> Option<String> {
    let value = serde_json::to_value(scene).ok()?;
    let bytes = serde_json::to_vec(&value).ok()?;
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    Some(format!("{:x}", hasher.finalize()))
}

fn is_newer_recovery(autosave_modified: SystemTime, source_modified: Option<SystemTime>) -> bool {
    source_modified
        .map(|source_modified| autosave_modified > source_modified)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests;
