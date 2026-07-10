use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use eframe::egui;
use engine::assets::LoadedAsset;
use engine::context::GameContext;
use engine::core::Ref;
use engine::scene::{Scene, SceneSnapshot};

use crate::scene_document::{
    PreparedSceneAutosave, SceneAutosave, SceneDocumentRevision, SceneDocumentState, SceneRecovery,
};

pub struct EditorSceneAutosave {
    document: SceneDocumentState,
    storage: SceneAutosave,
    pending: Option<PendingSceneAutosave>,
    recovery: Option<SceneRecovery>,
}

struct PendingSceneAutosave {
    source_file: PathBuf,
    revision: SceneDocumentRevision,
    receiver: mpsc::Receiver<Result<PreparedSceneAutosave, String>>,
}

impl EditorSceneAutosave {
    pub fn new(project_path: &Path) -> Self {
        Self {
            document: SceneDocumentState::default(),
            storage: SceneAutosave::new(project_path),
            pending: None,
            recovery: None,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.document.mark_dirty(Instant::now());
    }

    pub fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    pub fn prepare_scene_change(&mut self, game: &GameContext) -> bool {
        if self.write_now(game) {
            true
        } else {
            log::warn!("Kept current scene open because its recovery autosave failed");
            false
        }
    }

    pub fn scene_loaded(&mut self, source_file: Option<&Path>) {
        self.mark_clean();
        self.recovery = source_file.and_then(|file| self.storage.recovery_for_scene(file));
        if let Some(recovery) = &self.recovery {
            log::info!(
                "Detected newer scene recovery file: source={} autosave={}",
                recovery.source_file.display(),
                recovery.autosave_file.display()
            );
        }
    }

    pub fn new_scene_loaded(&mut self) {
        self.recovery = None;
        self.mark_clean();
    }

    pub fn scene_saved(&mut self, previous_file: Option<&Path>, file: &Path) {
        self.mark_clean();
        if let Some(previous_file) = previous_file {
            self.discard_file(previous_file);
        }
        self.discard_file(file);
    }

    pub fn update(&mut self, game: &GameContext) {
        self.poll_task(game);
        if self.pending.is_some() {
            return;
        }

        let now = Instant::now();
        if !self.document.should_autosave(now) {
            return;
        }
        let Some(file) = game.scenes.current_scene_meta().file.clone() else {
            return;
        };
        let revision = self.document.current_revision();
        let snapshot = game.scenes.current_scene().snapshot();
        let storage = self.storage.clone();
        let worker_file = file.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        game.resources
            .background()
            .read()
            .thread_pool()
            .execute(move || {
                let result = storage.prepare(&worker_file, &snapshot);
                let _ = sender.send(result);
            });

        self.document.mark_autosave_started(now);
        self.pending = Some(PendingSceneAutosave {
            source_file: file,
            revision,
            receiver,
        });
    }

    pub fn shutdown(&mut self, game: &GameContext) {
        if !self.write_now(game) {
            log::warn!("Failed to write final scene recovery autosave during editor shutdown");
        }
    }

    pub fn show_recovery_prompt(&mut self, ctx: &egui::Context, game: &mut GameContext) {
        let Some(recovery) = self.recovery.clone() else {
            return;
        };

        enum RecoveryAction {
            Recover,
            Discard,
        }

        let mut action = None;
        egui::Modal::new(egui::Id::new("scene_recovery")).show(ctx, |ui| {
            ui.heading("Scene Recovery");
            ui.separator();
            ui.label(format!(
                "A newer recovery save exists for {}.",
                recovery.source_file.display()
            ));
            ui.label(format!("{}", recovery.autosave_file.display()));
            ui.horizontal(|ui| {
                if ui.button("Recover").clicked() {
                    action = Some(RecoveryAction::Recover);
                }
                if ui.button("Discard").clicked() {
                    action = Some(RecoveryAction::Discard);
                }
            });
        });

        match action {
            Some(RecoveryAction::Recover) => self.recover_scene(game, recovery),
            Some(RecoveryAction::Discard) => self.discard_recovery(recovery),
            None => {}
        }
    }

    fn mark_clean(&mut self) {
        self.pending = None;
        self.document.mark_clean();
    }

    fn write_now(&mut self, game: &GameContext) -> bool {
        self.poll_task(game);
        let Some(file) = game.scenes.current_scene_meta().file.clone() else {
            return true;
        };
        if !self.document.is_dirty() {
            return true;
        }

        let revision = self.document.current_revision();
        if !self.document.should_commit_autosave(revision) {
            self.pending = None;
            return true;
        }
        let snapshot = game.scenes.current_scene().snapshot();
        self.document.mark_autosave_started(Instant::now());
        let success = self.write_file(&file, &snapshot);
        self.document.mark_autosave_finished(revision, success);
        if success {
            self.pending = None;
        }
        success
    }

    fn poll_task(&mut self, game: &GameContext) {
        let result = match self.pending.as_ref() {
            Some(task) => match task.receiver.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("background autosave worker disconnected".to_string())
                }
            },
            None => return,
        };
        let task = self
            .pending
            .take()
            .expect("pending autosave should still exist");

        let prepared = match result {
            Ok(prepared) => prepared,
            Err(error) => {
                self.document.mark_autosave_finished(task.revision, false);
                log::warn!(
                    "Failed to autosave scene {}: {}",
                    task.source_file.display(),
                    error
                );
                return;
            }
        };

        let source_is_current =
            game.scenes.current_scene_meta().file.as_deref() == Some(task.source_file.as_path());
        if !source_is_current || !self.document.should_commit_autosave(task.revision) {
            log::trace!(
                "Discarded stale scene autosave result: source={} autosave={}",
                task.source_file.display(),
                prepared.autosave_file().display()
            );
            return;
        }

        match prepared.commit() {
            Ok(autosave_file) => {
                log::info!(
                    "Autosaved scene recovery file: source={} autosave={}",
                    task.source_file.display(),
                    autosave_file.display()
                );
                self.document.mark_autosave_finished(task.revision, true);
            }
            Err(error) => {
                log::warn!(
                    "Failed to autosave scene {}: {}",
                    task.source_file.display(),
                    error
                );
                self.document.mark_autosave_finished(task.revision, false);
            }
        }
    }

    fn write_file(&self, file: &Path, snapshot: &SceneSnapshot) -> bool {
        match self.storage.write(file, snapshot) {
            Ok(autosave_file) => {
                log::info!(
                    "Autosaved scene recovery file: source={} autosave={}",
                    file.display(),
                    autosave_file.display()
                );
                true
            }
            Err(error) => {
                log::warn!("Failed to autosave scene {}: {}", file.display(), error);
                false
            }
        }
    }

    fn recover_scene(&mut self, game: &mut GameContext, recovery: SceneRecovery) {
        let loaded = {
            let assets = game.assets.lock_read();
            LoadedAsset::<Scene>::from_json_file_ctx(&assets, &recovery.autosave_file)
        };
        let scene = match loaded {
            Ok(loaded) => loaded.asset,
            Err(error) => {
                log::error!(
                    "Failed to recover scene from {}: {}",
                    recovery.autosave_file.display(),
                    error
                );
                return;
            }
        };
        game.scenes.load_scene(Ref::new(scene).readonly());
        game.scenes
            .set_current_scene_file(Some(recovery.source_file.clone()));
        self.mark_clean();
        self.mark_dirty();
        self.recovery = None;
        log::info!(
            "Recovered scene from autosave: source={} autosave={}",
            recovery.source_file.display(),
            recovery.autosave_file.display()
        );
    }

    fn discard_recovery(&mut self, recovery: SceneRecovery) {
        self.discard_file(&recovery.source_file);
        self.recovery = None;
        log::info!(
            "Discarded scene recovery file for {}",
            recovery.source_file.display()
        );
    }

    fn discard_file(&self, source_file: &Path) {
        if let Err(error) = self.storage.discard(source_file) {
            log::warn!("{error}");
        }
    }
}
