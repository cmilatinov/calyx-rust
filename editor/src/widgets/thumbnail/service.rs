use super::cache::ThumbnailCache;
use super::generator::ThumbnailGenerator;
use super::pipeline::*;
use super::*;
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::MutexGuard;

pub struct ThumbnailService {
    pub(super) shared: Arc<ThumbnailShared>,
    worker: Option<JoinHandle<()>>,
}

pub(super) struct ThumbnailShared {
    pub(super) state: Mutex<ThumbnailState>,
    wake: Condvar,
    state_poisoned: AtomicBool,
}

#[derive(Default)]
pub(super) struct ThumbnailState {
    pipeline: ThumbnailPipeline,
    textures: HashMap<ThumbnailKey, Texture>,
    pub(super) render_settings: ThumbnailRenderSettings,
    pub(super) cache_epoch: u64,
    invalidate_cache: bool,
    stop: bool,
}

enum ThumbnailWorkerCommand {
    Generate {
        job: ThumbnailJob,
        render_settings: ThumbnailRenderSettings,
        cache_epoch: u64,
    },
    InvalidateCache,
}

impl Default for ThumbnailService {
    fn default() -> Self {
        Self::with_render_settings(ThumbnailRenderSettings::default())
    }
}

impl ThumbnailService {
    pub fn with_render_settings(render_settings: ThumbnailRenderSettings) -> Self {
        Self {
            shared: Arc::new(ThumbnailShared {
                state: Mutex::new(ThumbnailState {
                    render_settings: render_settings.sanitized(),
                    ..Default::default()
                }),
                wake: Condvar::new(),
                state_poisoned: AtomicBool::new(false),
            }),
            worker: None,
        }
    }

    pub fn render_settings(&self) -> ThumbnailRenderSettings {
        lock_thumbnail_state(&self.shared).render_settings
    }

    pub fn set_render_settings(&mut self, render_settings: ThumbnailRenderSettings) {
        let render_settings = render_settings.sanitized();
        let mut state = lock_thumbnail_state(&self.shared);
        if state.render_settings == render_settings {
            return;
        }

        state.render_settings = render_settings;
        state.cache_epoch = state.cache_epoch.wrapping_add(1);
        state.pipeline = ThumbnailPipeline::default();
        state.textures.clear();
        drop(state);
        self.shared.wake.notify_one();
    }

    pub fn invalidate_cache(&mut self) {
        let mut state = lock_thumbnail_state(&self.shared);
        state.cache_epoch = state.cache_epoch.wrapping_add(1);
        state.invalidate_cache = true;
        state.pipeline = ThumbnailPipeline::default();
        state.textures.clear();
        drop(state);
        self.shared.wake.notify_one();
    }

    pub fn request(
        &mut self,
        request: ThumbnailRequest,
        priority: ThumbnailPriority,
    ) -> ThumbnailStatus {
        let mut state = lock_thumbnail_state(&self.shared);
        state
            .pipeline
            .clear_asset_versions_except(request.asset_id, request.source_version);
        state.textures.retain(|key, _| {
            key.asset_id != request.asset_id || key.source_version == request.source_version
        });
        let status = state.pipeline.request(request, priority);
        drop(state);
        self.shared.wake.notify_one();
        status
    }

    pub fn status(&self, key: ThumbnailKey) -> ThumbnailStatus {
        lock_thumbnail_state(&self.shared).pipeline.status(key)
    }

    pub fn texture_id(&self, key: ThumbnailKey) -> Option<egui::TextureId> {
        lock_thumbnail_state(&self.shared)
            .textures
            .get(&key)
            .and_then(|texture| texture.handle.as_ref())
            .map(|handle| handle.id())
    }

    pub fn process(&mut self, context: &ReadOnlyAssetContext, _render_state: &RenderState) {
        self.ensure_worker(context.clone());
    }

    fn ensure_worker(&mut self, context: ReadOnlyAssetContext) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::warn!("Thumbnail generation worker exited with a panic");
            }
        }

        let shared = self.shared.clone();
        let render_settings = self.render_settings();
        self.worker = Some(
            thread::Builder::new()
                .name("thumbnail-generator".into())
                .spawn(move || thumbnail_worker_loop(shared, context, render_settings))
                .expect("failed to spawn thumbnail generation worker"),
        );
        self.shared.wake.notify_one();
    }
}

impl Drop for ThumbnailService {
    fn drop(&mut self) {
        {
            let mut state = lock_thumbnail_state(&self.shared);
            state.stop = true;
        }
        self.shared.wake.notify_one();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::warn!("Thumbnail generation worker exited with a panic");
            }
        }
    }
}

fn lock_thumbnail_state(shared: &ThumbnailShared) -> MutexGuard<'_, ThumbnailState> {
    match shared.state.lock() {
        Ok(state) => state,
        Err(poisoned) => {
            log_thumbnail_state_poison(shared);
            poisoned.into_inner()
        }
    }
}

fn wait_thumbnail_state<'a>(
    shared: &ThumbnailShared,
    state: MutexGuard<'a, ThumbnailState>,
) -> MutexGuard<'a, ThumbnailState> {
    match shared.wake.wait(state) {
        Ok(state) => state,
        Err(poisoned) => {
            log_thumbnail_state_poison(shared);
            poisoned.into_inner()
        }
    }
}

fn log_thumbnail_state_poison(shared: &ThumbnailShared) {
    if !shared.state_poisoned.swap(true, Ordering::Relaxed) {
        log::warn!("Thumbnail service state mutex was poisoned; recovering state");
    }
}

fn thumbnail_worker_loop(
    shared: Arc<ThumbnailShared>,
    context: ReadOnlyAssetContext,
    render_settings: ThumbnailRenderSettings,
) {
    let mut generator = ThumbnailGenerator::with_render_settings(render_settings);
    loop {
        let command = {
            let mut state = lock_thumbnail_state(&shared);
            loop {
                if state.stop {
                    return;
                }
                if state.invalidate_cache {
                    state.invalidate_cache = false;
                    break ThumbnailWorkerCommand::InvalidateCache;
                }
                if let Some(job) = state.pipeline.start_next() {
                    break ThumbnailWorkerCommand::Generate {
                        job,
                        render_settings: state.render_settings,
                        cache_epoch: state.cache_epoch,
                    };
                }
                state = wait_thumbnail_state(&shared, state);
            }
        };

        let ThumbnailWorkerCommand::Generate {
            job,
            render_settings,
            cache_epoch,
        } = command
        else {
            match ThumbnailCache::invalidate_project(&context) {
                Ok(()) => log::info!("Invalidated thumbnail cache"),
                Err(err) => log::warn!("{err}"),
            }
            continue;
        };

        let key = job.key();
        let request = job.request.clone();
        let started_at = Instant::now();
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            process_thumbnail_job(
                &shared,
                &context,
                &mut generator,
                render_settings,
                cache_epoch,
                job,
                started_at,
            );
        }));
        if let Err(payload) = result {
            let message = format!(
                "thumbnail generation panicked: {}",
                panic_payload_message(payload.as_ref())
            );
            generator = ThumbnailGenerator::with_render_settings(render_settings);
            fail_thumbnail_job(&shared, cache_epoch, key, &request, started_at, message);
        }
    }
}

fn process_thumbnail_job(
    shared: &ThumbnailShared,
    context: &ReadOnlyAssetContext,
    generator: &mut ThumbnailGenerator,
    render_settings: ThumbnailRenderSettings,
    cache_epoch: u64,
    job: ThumbnailJob,
    started_at: Instant,
) {
    generator.render_settings = render_settings;
    let cache = ThumbnailCache::new(context, render_settings);
    let key = job.key();
    let render_state = context.render_context.render_state();
    match cache.load(context, &job.request) {
        Ok(Some(texture)) => {
            let mut state = lock_thumbnail_state(shared);
            let status_updated = state.cache_epoch == cache_epoch && state.pipeline.complete(key);
            if status_updated {
                state.textures.insert(key, texture);
            }
            return;
        }
        Ok(None) | Err(_) => {}
    }

    match generator.generate(context, render_state, &job.request) {
        Ok(texture) => {
            let should_store = {
                let state = lock_thumbnail_state(shared);
                state.cache_epoch == cache_epoch
                    && matches!(state.pipeline.status(key), ThumbnailStatus::InProgress)
            };
            if should_store {
                if let Err(err) = cache.store(render_state, &job.request, &texture) {
                    log::warn!(
                            "Failed to store thumbnail cache asset={} type={} version={} path={} error={}",
                            job.request.asset_id,
                            thumbnail_asset_type_name(job.request.asset_type),
                            job.request.source_version,
                            thumbnail_source_label(&job.request),
                            err
                        );
                }
            }
            let mut state = lock_thumbnail_state(shared);
            let status_updated = state.cache_epoch == cache_epoch && state.pipeline.complete(key);
            if status_updated {
                log::trace!(
                        "Generated thumbnail asset={} type={} version={} path={} texture_size={}x{} format={:?} elapsed_ms={}",
                        job.request.asset_id,
                        thumbnail_asset_type_name(job.request.asset_type),
                        job.request.source_version,
                        thumbnail_source_label(&job.request),
                        texture.descriptor.size.width,
                        texture.descriptor.size.height,
                        texture.descriptor.format,
                        started_at.elapsed().as_millis()
                    );
                state.textures.insert(key, texture);
            }
        }
        Err(message) => {
            fail_thumbnail_job(shared, cache_epoch, key, &job.request, started_at, message);
        }
    }
}

fn fail_thumbnail_job(
    shared: &ThumbnailShared,
    cache_epoch: u64,
    key: ThumbnailKey,
    request: &ThumbnailRequest,
    started_at: Instant,
    message: impl Into<String>,
) {
    let message = message.into();
    let mut state = lock_thumbnail_state(shared);
    if state.cache_epoch != cache_epoch {
        return;
    }
    let status_updated = state.pipeline.fail(key, message.as_str());
    let failure_count = state.pipeline.failure_count(key);
    if status_updated && failure_count <= THUMBNAIL_MAX_FAILURES {
        log::warn!(
            "Thumbnail request failed asset={} type={} version={} attempt={}/{} path={} elapsed_ms={} error={}",
            request.asset_id,
            thumbnail_asset_type_name(request.asset_type),
            request.source_version,
            failure_count,
            THUMBNAIL_MAX_FAILURES,
            thumbnail_source_label(request),
            started_at.elapsed().as_millis(),
            message
        );
    }
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".into()
    }
}
