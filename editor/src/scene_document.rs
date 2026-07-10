use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use engine::scene::Scene;
use sha1::{Digest, Sha1};

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct SceneRecovery {
    pub source_file: PathBuf,
    pub autosave_file: PathBuf,
}

#[derive(Debug)]
pub struct SceneDocumentState {
    dirty: bool,
    saved_scene_fingerprint: Option<String>,
    revision: u64,
    last_autosaved_revision: u64,
    last_edit_at: Option<Instant>,
    last_autosave_at: Option<Instant>,
}

impl Default for SceneDocumentState {
    fn default() -> Self {
        Self {
            dirty: false,
            saved_scene_fingerprint: None,
            revision: 0,
            last_autosaved_revision: 0,
            last_edit_at: None,
            last_autosave_at: None,
        }
    }
}

impl SceneDocumentState {
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_dirty(&mut self, now: Instant) {
        self.dirty = true;
        self.revision = self.revision.saturating_add(1);
        self.last_edit_at = Some(now);
    }

    pub fn mark_clean(&mut self, scene_fingerprint: Option<String>) {
        self.dirty = false;
        self.saved_scene_fingerprint = scene_fingerprint;
        self.last_autosaved_revision = self.revision;
        self.last_edit_at = None;
        self.last_autosave_at = None;
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
        } else {
            self.dirty = false;
            self.last_autosaved_revision = self.revision;
            self.last_edit_at = None;
            self.last_autosave_at = None;
        }
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

    pub fn mark_autosave_attempt(&mut self, now: Instant, success: bool) {
        self.last_autosave_at = Some(now);
        if success {
            self.last_autosaved_revision = self.revision;
        }
    }
}

pub struct SceneAutosave {
    root: PathBuf,
}

impl SceneAutosave {
    pub fn new(project_path: &Path) -> Self {
        Self {
            root: autosave_root(project_path),
        }
    }

    pub fn write(&self, source_file: &Path, scene: &Scene) -> Result<PathBuf, String> {
        let path = self.path_for_scene(source_file);
        let parent = path
            .parent()
            .ok_or_else(|| format!("autosave path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create autosave directory {}: {err}",
                parent.display()
            )
        })?;

        let tmp_path = path.with_extension("cxscene.autosave.tmp");
        {
            let file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)
                .map_err(|err| {
                    format!(
                        "failed to open scene autosave temp file {}: {err}",
                        tmp_path.display()
                    )
                })?;
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, scene).map_err(|err| {
                format!(
                    "failed to write scene autosave temp file {}: {err}",
                    tmp_path.display()
                )
            })?;
        }

        replace_file(&tmp_path, &path)?;
        Ok(path)
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
            fs::remove_file(path).map_err(|err| {
                format!(
                    "failed to replace scene autosave {} after rename error {rename_error}: {err}",
                    path.display()
                )
            })?;
            fs::rename(tmp_path, path).map_err(|err| {
                format!(
                    "failed to move scene autosave {} to {}: {err}",
                    tmp_path.display(),
                    path.display()
                )
            })
        }
        Err(err) => Err(format!(
            "failed to move scene autosave {} to {}: {err}",
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
mod tests {
    use super::*;

    #[test]
    fn clean_document_does_not_autosave() {
        let state = SceneDocumentState::default();

        assert!(!state.should_autosave(Instant::now() + AUTOSAVE_DELAY));
    }

    #[test]
    fn dirty_document_autosaves_after_debounce() {
        let mut state = SceneDocumentState::default();
        let now = Instant::now();

        state.mark_dirty(now);

        assert!(!state.should_autosave(now + AUTOSAVE_DELAY - Duration::from_millis(1)));
        assert!(state.should_autosave(now + AUTOSAVE_DELAY));
    }

    #[test]
    fn successful_autosave_suppresses_same_revision() {
        let mut state = SceneDocumentState::default();
        let now = Instant::now();

        state.mark_dirty(now);
        state.mark_autosave_attempt(now + AUTOSAVE_DELAY, true);

        assert!(!state
            .should_autosave(now + AUTOSAVE_DELAY + AUTOSAVE_INTERVAL + Duration::from_secs(1)));
    }

    #[test]
    fn dirty_sync_marks_matching_saved_fingerprint_clean() {
        let mut state = SceneDocumentState::default();
        let now = Instant::now();

        state.mark_clean(Some("saved".into()));
        state.mark_dirty(now);
        state.sync_dirty_to_fingerprint(Some("saved".into()), now + Duration::from_secs(1));

        assert!(!state.is_dirty());
        assert!(!state.should_autosave(now + AUTOSAVE_DELAY + AUTOSAVE_INTERVAL));
    }

    #[test]
    fn dirty_sync_keeps_different_fingerprint_dirty() {
        let mut state = SceneDocumentState::default();
        let now = Instant::now();

        state.mark_clean(Some("saved".into()));
        state.sync_dirty_to_fingerprint(Some("changed".into()), now);

        assert!(state.is_dirty());
        assert!(state.should_autosave(now + AUTOSAVE_DELAY));
    }

    #[test]
    fn recovery_requires_autosave_newer_than_source() {
        let source = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        let older_autosave = SystemTime::UNIX_EPOCH + Duration::from_secs(9);
        let newer_autosave = SystemTime::UNIX_EPOCH + Duration::from_secs(11);

        assert!(!is_newer_recovery(older_autosave, Some(source)));
        assert!(is_newer_recovery(newer_autosave, Some(source)));
        assert!(is_newer_recovery(newer_autosave, None));
    }

    #[test]
    fn autosave_path_is_stable_for_source_scene() {
        let autosave = SceneAutosave::new(Path::new("sandbox"));
        let source = Path::new("assets/scene.cxscene");

        assert_eq!(
            autosave.path_for_scene(source),
            autosave.path_for_scene(source)
        );
        assert_eq!(
            autosave
                .path_for_scene(source)
                .extension()
                .and_then(|ext| ext.to_str()),
            Some("autosave")
        );
    }
}
