use super::cache::ThumbnailCache;
use super::generator::ThumbnailGenerator;
use super::pipeline::*;
use super::*;

pub struct ThumbnailService {
    pub(super) shared: Arc<ThumbnailShared>,
    worker: Option<JoinHandle<()>>,
}

pub(super) struct ThumbnailShared {
    pub(super) state: Mutex<ThumbnailState>,
    wake: Condvar,
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
            }),
            worker: None,
        }
    }

    pub fn render_settings(&self) -> ThumbnailRenderSettings {
        self.shared.state.lock().unwrap().render_settings
    }

    pub fn set_render_settings(&mut self, render_settings: ThumbnailRenderSettings) {
        let render_settings = render_settings.sanitized();
        let mut state = self.shared.state.lock().unwrap();
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
        let mut state = self.shared.state.lock().unwrap();
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
        let mut state = self.shared.state.lock().unwrap();
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
        self.shared.state.lock().unwrap().pipeline.status(key)
    }

    pub fn texture_id(&self, key: ThumbnailKey) -> Option<egui::TextureId> {
        self.shared
            .state
            .lock()
            .unwrap()
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
            let mut state = self.shared.state.lock().unwrap();
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

fn thumbnail_worker_loop(
    shared: Arc<ThumbnailShared>,
    context: ReadOnlyAssetContext,
    render_settings: ThumbnailRenderSettings,
) {
    let mut generator = ThumbnailGenerator::with_render_settings(render_settings);
    loop {
        let command = {
            let mut state = shared.state.lock().unwrap();
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
                state = shared.wake.wait(state).unwrap();
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

        generator.render_settings = render_settings;
        let cache = ThumbnailCache::new(&context, render_settings);
        let key = job.key();
        let started_at = Instant::now();
        let render_state = context.render_context.render_state();
        match cache.load(&context, &job.request) {
            Ok(Some(texture)) => {
                let mut state = shared.state.lock().unwrap();
                let status_updated =
                    state.cache_epoch == cache_epoch && state.pipeline.complete(key);
                if status_updated {
                    state.textures.insert(key, texture);
                }
                continue;
            }
            Ok(None) | Err(_) => {}
        }

        match generator.generate(&context, render_state, &job.request) {
            Ok(texture) => {
                let should_store = {
                    let state = shared.state.lock().unwrap();
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
                let mut state = shared.state.lock().unwrap();
                let status_updated =
                    state.cache_epoch == cache_epoch && state.pipeline.complete(key);
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
                let mut state = shared.state.lock().unwrap();
                if state.cache_epoch != cache_epoch {
                    continue;
                }
                let status_updated = state.pipeline.fail(key, message.as_str());
                let failure_count = state.pipeline.failure_count(key);
                if status_updated && failure_count <= THUMBNAIL_MAX_FAILURES {
                    log::warn!(
                        "Thumbnail request failed asset={} type={} version={} attempt={}/{} path={} elapsed_ms={} error={}",
                        job.request.asset_id,
                        thumbnail_asset_type_name(job.request.asset_type),
                        job.request.source_version,
                        failure_count,
                        THUMBNAIL_MAX_FAILURES,
                        thumbnail_source_label(&job.request),
                        started_at.elapsed().as_millis(),
                        message
                    );
                }
            }
        }
    }
}
