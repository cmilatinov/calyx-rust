//! Keeps the registry in step with `.meta` files that tools other than the
//! editor write while it runs, such as an importer, a version control
//! checkout or a sync.
//!
//! Two rules hold together:
//!
//! - A file the watcher reports without a `.meta` waits [`CREATED_FILE_SETTLE`]
//!   for one before the registry generates its own. The file is not
//!   registered while it waits, so its generated id is never handed out
//!   before the window ends.
//! - A `.meta` on disk is the record of an asset's id. The registry never
//!   rewrites the id of one that exists, and re-reads one that changes: when
//!   it names another id, the asset is re-keyed under that id, and the id it
//!   had resolves to the new one for the rest of the session.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use relative_path::RelativePathBuf;
use uuid::Uuid;

use super::{AssetMeta, AssetMetaData, AssetRegistry};
use crate::error::BoxedError;

/// How long a file created without a `.meta` waits for one to appear before
/// the registry generates its own. A tool writing a file and then its `.meta`
/// does so well inside it; each write to the waiting file restarts it.
pub(crate) const CREATED_FILE_SETTLE: Duration = Duration::from_millis(500);

/// How often the watcher thread registers the files whose window passed.
pub(super) const SETTLE_POLL: Duration = Duration::from_millis(100);

impl AssetRegistry {
    /// Holds a file the watcher reported without a `.meta` until its `.meta`
    /// appears or [`CREATED_FILE_SETTLE`] passes. Reporting it again restarts
    /// its window.
    pub(super) fn defer_created_file(&self, file: &Path) {
        log::trace!("Waiting for the .meta of {}", file.display());
        self.asset_data_mut()
            .pending
            .insert(file.to_path_buf(), Instant::now());
    }

    /// Restarts the window of `file` when it waits for its `.meta`, as a
    /// tool still writing it reports it again.
    pub(super) fn touch_pending_file(&self, file: &Path) {
        if let Some(seen) = self.asset_data_mut().pending.get_mut(file) {
            *seen = Instant::now();
        }
    }

    /// Stops waiting for the files at or below a deleted `path`.
    pub(super) fn forget_pending_under(&self, path: &Path) {
        self.asset_data_mut()
            .pending
            .retain(|file, _| !file.starts_with(path));
    }

    /// Whether `file` was created without a `.meta` and still waits for one.
    #[cfg(test)]
    pub(crate) fn is_pending_file(&self, file: &Path) -> bool {
        self.asset_data().pending.contains_key(file)
    }

    /// Registers each file created without a `.meta` that has waited out
    /// [`CREATED_FILE_SETTLE`] by `now`, generating its `.meta` unless one
    /// appeared meanwhile.
    pub(crate) fn register_settled_files(&self, now: Instant) {
        let settled: Vec<PathBuf> = {
            let mut data = self.asset_data_mut();
            if data.pending.is_empty() {
                return;
            }
            let settled: Vec<PathBuf> = data
                .pending
                .iter()
                .filter(|(_, seen)| now.saturating_duration_since(**seen) >= CREATED_FILE_SETTLE)
                .map(|(file, _)| file.clone())
                .collect();
            for file in &settled {
                data.pending.remove(file);
            }
            settled
        };
        for file in settled {
            if !file.is_file() || self.is_registered_at(&file) {
                continue;
            }
            let Some(root) = self.owning_asset_root(&file) else {
                continue;
            };
            match self.build_asset_meta(root, &file, &file.with_extension("meta")) {
                // Its create and write events came while it had no id, so
                // they could not retry a failed load of the id it has now.
                Ok(()) => self.clear_failed_background_load(&file),
                Err(err) => log::warn!(
                    "Failed to build asset metadata for {}: {}",
                    file.display(),
                    err
                ),
            }
        }
    }

    /// Brings the registry in line with the `.meta` at `meta_path`, which
    /// was just written or changed: registers its waiting asset file under
    /// the id it names, or re-keys the registered asset when the id differs.
    /// A `.meta` naming the id the asset has, as the registry's own writes
    /// do, changes nothing.
    pub(super) fn sync_meta_file(&self, meta_path: &Path) {
        let Some(root) = self.owning_asset_root(meta_path) else {
            return;
        };
        let extensions: Vec<String> = self.asset_data().extensions.keys().cloned().collect();
        for ext in extensions {
            let file = meta_path.with_extension(ext);
            if file.is_file() {
                self.sync_asset_with_meta(root, &file, meta_path);
            }
        }
    }

    fn sync_asset_with_meta(&self, root: &Path, file: &Path, meta_path: &Path) {
        let registered = self
            .asset_meta_from_path(file)
            .filter(|meta| meta.path.as_deref() == Some(file));
        let Some(registered) = registered else {
            // Waiting for this `.meta`, or registering failed on an earlier,
            // partly written one.
            match self.build_asset_meta(root, file, meta_path) {
                Ok(()) => {
                    self.asset_data_mut().pending.remove(file);
                    self.clear_failed_background_load(file);
                    log::debug!(
                        "Registered {} from its .meta {}",
                        file.display(),
                        meta_path.display()
                    );
                }
                // A `.meta` still being written; its next write retries.
                Err(err) => log::debug!("Cannot read {} yet: {}", meta_path.display(), err),
            }
            return;
        };
        let on_disk = match Self::read_meta_file(meta_path) {
            Ok(meta) => meta,
            Err(err) => {
                log::debug!("Cannot read {} yet: {}", meta_path.display(), err);
                return;
            }
        };
        if on_disk.main.id != registered.id {
            self.rekey_asset(root, file, meta_path, registered);
        }
    }

    /// Moves the asset at `file` from the id it is registered under to the
    /// one its `.meta` names. Its cached value, read under the old id, is
    /// evicted, so the next load reads it under the new one; a holder of the
    /// old reference keeps its value. The old id, and each sub-asset's the
    /// new `.meta` lists under another id, resolve to the new ones, so a
    /// reference saved with them still loads; a sub-asset it does not list
    /// keeps its id under the new parent.
    fn rekey_asset(&self, root: &Path, file: &Path, meta_path: &Path, old: AssetMeta) {
        let old_children: Vec<AssetMeta> = {
            let data = self.asset_data();
            old.children
                .iter()
                .filter_map(|child| data.meta.get(child).cloned())
                .collect()
        };
        let mut ids: HashSet<Uuid> = old.children.iter().copied().collect();
        ids.insert(old.id);
        self.drop_asset_ids(&ids);
        if let Err(err) = self.build_asset_meta(root, file, meta_path) {
            log::warn!(
                "Failed to re-read asset metadata {}: {}",
                meta_path.display(),
                err
            );
            // Register it once its `.meta` reads again, or generate one.
            self.defer_created_file(file);
            return;
        }
        let Some(new) = self.asset_meta_from_path(file) else {
            return;
        };
        let mut moves = vec![(old.id, new.id)];
        let mut data = self.asset_meta_mut();
        for mut old_child in old_children {
            let new_child = new.children.iter().copied().find(|child| {
                data.meta
                    .get(child)
                    .is_some_and(|meta| meta.name == old_child.name)
            });
            match new_child {
                Some(new_child) if new_child != old_child.id => {
                    moves.push((old_child.id, new_child));
                }
                Some(_) => {}
                // The new `.meta` does not list it, as a tool's that leaves
                // sub-assets to the import: keep it under the new parent, so
                // loading it by id still loads the model that recreates it.
                None => {
                    let name = Self::sub_asset_name(&new.name, &old_child.name);
                    old_child.parent = Some(new.id);
                    data.names.insert(
                        RelativePathBuf::from(name.as_str()).normalize(),
                        old_child.id,
                    );
                    if let Some(parent) = data.meta.get_mut(&new.id) {
                        parent.children.push(old_child.id);
                    }
                    data.meta.insert(old_child.id, old_child);
                }
            }
        }
        for (from, to) in moves {
            Self::add_alias_locked(&mut data.aliases, from, to);
        }
        drop(data);
        log::info!(
            "Re-keyed asset {} from {} to {}: its .meta {} names another id",
            new.name,
            old.id,
            new.id,
            meta_path.display()
        );
    }

    /// Points `from` at `to`, and every id that pointed at `from` at `to`
    /// too, so each alias names the id an asset has now, however many times
    /// it was re-keyed.
    fn add_alias_locked(aliases: &mut HashMap<Uuid, Uuid>, from: Uuid, to: Uuid) {
        for target in aliases.values_mut() {
            if *target == from {
                *target = to;
            }
        }
        aliases.insert(from, to);
        // `to` is registered again, as when a `.meta` changes back.
        aliases.remove(&to);
    }

    /// The id an asset re-keyed from `id` has now, or `id` itself. A
    /// registered id is never redirected.
    pub(super) fn resolve_asset_id(&self, id: Uuid) -> Uuid {
        let data = self.asset_data();
        if data.meta.contains_key(&id) {
            return id;
        }
        data.aliases.get(&id).copied().unwrap_or(id)
    }

    /// Whether the registry holds an asset whose source is `file` itself,
    /// not a same-name file in another root.
    pub(super) fn is_registered_at(&self, file: &Path) -> bool {
        self.asset_meta_from_path(file)
            .is_some_and(|meta| meta.path.as_deref() == Some(file))
    }

    /// Parses the `.meta` at `meta_path` without registering anything.
    pub(super) fn read_meta_file(meta_path: &Path) -> Result<AssetMetaData, BoxedError> {
        let file = std::fs::File::open(meta_path).map_err(Box::new)?;
        Ok(serde_json::from_reader(BufReader::new(file)).map_err(Box::new)?)
    }

    /// Writes a generated `.meta` only where none exists, so one another tool
    /// wrote since the registry looked is never replaced. `false` when one
    /// exists.
    pub(super) fn write_new_meta_file(
        meta_path: &Path,
        meta: &AssetMetaData,
    ) -> Result<bool, BoxedError> {
        let content = serde_json::to_string_pretty(meta).map_err(Box::new)?;
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(meta_path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
            Err(err) => return Err(Box::new(err)),
        };
        file.write_all(content.as_bytes()).map_err(Box::new)?;
        Ok(true)
    }

    /// The id the `.meta` at `meta_path` names when it is not `id`, so
    /// writing `id` over it would replace another tool's id.
    pub(super) fn meta_names_another_id(meta_path: &Path, id: Uuid) -> Option<Uuid> {
        Self::read_meta_file(meta_path)
            .ok()
            .map(|meta| meta.main.id)
            .filter(|on_disk| *on_disk != id)
    }
}

#[cfg(test)]
mod tests;
