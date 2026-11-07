//! What thumbnail jobs leave loaded in the asset registry: each test runs
//! the worker's job loop by hand on the shared test GPU.

use super::cache::ThumbnailCache;
use super::generator::ThumbnailGenerator;
use super::pipeline::*;
use super::service::{finish_worker_job, process_thumbnail_job, start_worker_job};
use super::*;
use calyx_engine::context::AssetContext;
use calyx_engine::test_support::TestAssets;

const SIZE_PX: u32 = 64;

/// A copy of the engine assets, and a thumbnail cache of its own that is
/// removed when the fixture drops.
struct Fixture {
    _assets: TestAssets,
    context: AssetContext,
}

impl Fixture {
    fn new() -> Self {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        Self {
            _assets: assets,
            context,
        }
    }

    fn context(&self) -> ReadOnlyAssetContext {
        self.context.lock_read()
    }

    fn asset_id(&self, name: &str) -> Uuid {
        self.context()
            .registries
            .assets
            .read()
            .asset_id(name)
            .unwrap_or_else(|| panic!("engine asset {name} should be registered"))
    }

    fn is_loaded(&self, asset_id: Uuid) -> bool {
        let context = self.context();
        let registry = context.registries.assets.read();
        registry.loaded_by_id::<Texture>(asset_id).is_some()
            || registry.loaded_by_id::<Mesh>(asset_id).is_some()
    }

    fn loaded_count(&self) -> usize {
        self.context().registries.assets.read().loaded_count()
    }

    fn change_epoch(&self) -> u64 {
        self.context().registries.assets.read().change_epoch()
    }

    /// Asks a fresh service for the thumbnail of each asset, as the content
    /// browser's tiles do, runs every job it queues, and returns each
    /// thumbnail's status. The views stay `Loading` here, since tests
    /// register no egui texture for a finished thumbnail.
    fn draw_tiles(&self, asset_ids: &[Uuid]) -> Vec<ThumbnailStatus> {
        let render_settings = ThumbnailRenderSettings::with_size_px(SIZE_PX);
        let service = ThumbnailService::with_render_settings(render_settings);
        let handle = service.handle();
        let context = self.context();
        let view = |asset_id: Uuid| {
            let registry = context.registries.assets.read();
            handle.view_for_asset(&registry, asset_id, ThumbnailPriority::Normal)
        };
        for asset_id in asset_ids {
            assert_eq!(view(*asset_id), ThumbnailView::Loading);
        }

        let mut generator = ThumbnailGenerator::with_render_settings(render_settings);
        loop {
            let (job, cache_epoch) = {
                let mut state = service.shared.state.lock().unwrap();
                let Some(job) = start_worker_job(&mut state) else {
                    break;
                };
                (job, state.cache_epoch)
            };
            process_thumbnail_job(
                &service.shared,
                &context,
                &mut generator,
                render_settings,
                cache_epoch,
                job,
                Instant::now(),
            );
            finish_worker_job(&service.shared);
        }
        let registry = context.registries.assets.read();
        asset_ids
            .iter()
            .map(|asset_id| {
                let request = ThumbnailRequest::from_asset_id(&registry, *asset_id)
                    .expect("the asset has a thumbnail");
                service.status(request.key())
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = ThumbnailCache::invalidate_project(&self.context());
    }
}

fn assert_ready(statuses: &[ThumbnailStatus]) {
    for status in statuses {
        assert_eq!(*status, ThumbnailStatus::Ready, "every thumbnail is ready");
    }
}

#[test]
fn generating_texture_and_mesh_thumbnails_leaves_neither_loaded() {
    let fixture = Fixture::new();
    // The first jobs load what every thumbnail of their kind shares, such
    // as the thumbnail shaders, which stay; the count is taken after them.
    assert_ready(&fixture.draw_tiles(&[
        fixture.asset_id("textures/folder_dark"),
        fixture.asset_id("meshes/plane"),
    ]));
    let baseline = fixture.loaded_count();

    let texture = fixture.asset_id("textures/red");
    let mesh = fixture.asset_id("meshes/teapot");
    assert!(!fixture.is_loaded(texture) && !fixture.is_loaded(mesh));
    assert_ready(&fixture.draw_tiles(&[texture, mesh]));

    assert!(!fixture.is_loaded(texture), "the texture was evicted");
    assert!(!fixture.is_loaded(mesh), "the mesh was evicted");
    assert_eq!(
        fixture.loaded_count(),
        baseline,
        "nothing the jobs loaded stays loaded"
    );
}

#[test]
fn a_thumbnail_job_keeps_assets_something_else_loaded() {
    let fixture = Fixture::new();
    let mesh = fixture.asset_id("meshes/stanford-bunny");
    let held = fixture
        .context()
        .registries
        .assets
        .read()
        .load_by_id::<Mesh>(mesh)
        .expect("the mesh loads");

    assert_ready(&fixture.draw_tiles(&[mesh]));

    let cached = fixture
        .context()
        .registries
        .assets
        .read()
        .loaded_by_id::<Mesh>(mesh)
        .expect("a mesh loaded before the job stays loaded");
    assert_eq!(cached.ptr_id(), held.ptr_id(), "it is the same asset");
}

#[test]
fn drawing_a_folder_whose_thumbnails_are_cached_loads_no_asset() {
    let fixture = Fixture::new();
    let folder = fixture
        .context()
        .registries
        .assets
        .read()
        .root_path()
        .join("textures");
    let asset_ids = {
        let context = fixture.context();
        let registry = context.registries.assets.read();
        fs::read_dir(&folder)
            .expect("the textures folder lists")
            .flatten()
            .filter_map(|entry| registry.asset_id_from_path(&entry.path()))
            .collect::<Vec<_>>()
    };
    assert!(!asset_ids.is_empty());
    assert_ready(&fixture.draw_tiles(&asset_ids));
    let (count, epoch) = (fixture.loaded_count(), fixture.change_epoch());

    let record = AssetLoadRecord::start();
    let statuses = fixture.draw_tiles(&asset_ids);
    let loaded = record.finish();

    assert_ready(&statuses);
    assert_eq!(loaded, Vec::<Uuid>::new(), "no asset was loaded");
    assert_eq!(fixture.loaded_count(), count);
    assert_eq!(
        fixture.change_epoch(),
        epoch,
        "the registry did not change at all"
    );
}
