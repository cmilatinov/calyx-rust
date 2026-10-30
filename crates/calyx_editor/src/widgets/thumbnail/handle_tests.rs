use super::handle::{thumbnail_view, ThumbnailRequestMemo};
use super::pipeline::*;
use super::service::ThumbnailService;
use super::*;
use std::cell::Cell;
use std::time::Duration;

fn request(asset_id: u128, source_version: u64) -> ThumbnailRequest {
    ThumbnailRequest {
        asset_id: Uuid::from_u128(asset_id),
        asset_type: Texture::type_uuid(),
        source_path: Some(PathBuf::from("assets/texture.png")),
        source_version,
    }
}

fn texture_id() -> egui::TextureId {
    egui::TextureId::User(7)
}

#[test]
fn thumbnail_view_shows_texture_only_when_ready_with_a_texture() {
    assert_eq!(
        thumbnail_view(Some(ThumbnailStatus::Ready), Some(texture_id())),
        ThumbnailView::Ready(texture_id())
    );
    assert_eq!(
        thumbnail_view(Some(ThumbnailStatus::Ready), None),
        ThumbnailView::Loading
    );
}

#[test]
fn thumbnail_view_reports_loading_while_queued_or_in_progress() {
    for status in [
        ThumbnailStatus::Missing,
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::High,
        },
        ThumbnailStatus::InProgress,
    ] {
        assert_eq!(thumbnail_view(Some(status), None), ThumbnailView::Loading);
    }
}

#[test]
fn thumbnail_view_reports_failure_message_and_unsupported_assets() {
    assert_eq!(
        thumbnail_view(
            Some(ThumbnailStatus::Failed {
                message: "no mesh".into()
            }),
            Some(texture_id())
        ),
        ThumbnailView::Failed("no mesh".into())
    );
    assert_eq!(thumbnail_view(None, None), ThumbnailView::Unsupported);
}

#[test]
fn thumbnail_view_badges_match_display_state() {
    assert_eq!(
        ThumbnailView::Ready(texture_id()).badge(),
        ThumbnailBadge::None
    );
    assert_eq!(ThumbnailView::Loading.badge(), ThumbnailBadge::Loading);
    assert_eq!(
        ThumbnailView::Failed("x".into()).badge(),
        ThumbnailBadge::Failed
    );
    assert_eq!(ThumbnailView::Unsupported.badge(), ThumbnailBadge::None);
    assert_eq!(
        ThumbnailView::Ready(texture_id()).texture_id(),
        Some(texture_id())
    );
    assert_eq!(ThumbnailView::Loading.texture_id(), None);
}

#[test]
fn request_memo_reuses_request_until_the_refresh_interval_passes() {
    let mut memo = ThumbnailRequestMemo::new(Duration::from_secs(1));
    let asset_id = Uuid::from_u128(1);
    let computed = Cell::new(0);
    let start = Instant::now();
    let compute = |version| {
        computed.set(computed.get() + 1);
        Some(request(1, version))
    };

    let first = memo.get_or_compute(asset_id, start, || compute(1));
    let cached = memo.get_or_compute(asset_id, start + Duration::from_millis(999), || compute(2));
    let refreshed = memo.get_or_compute(asset_id, start + Duration::from_secs(1), || compute(3));

    assert_eq!(first.map(|r| r.source_version), Some(1));
    assert_eq!(cached.map(|r| r.source_version), Some(1));
    assert_eq!(refreshed.map(|r| r.source_version), Some(3));
    assert_eq!(computed.get(), 2);
}

#[test]
fn request_memo_caches_unsupported_assets_and_keys_by_asset() {
    let mut memo = ThumbnailRequestMemo::new(Duration::from_secs(1));
    let now = Instant::now();
    let computed = Cell::new(0);

    for _ in 0..3 {
        let result = memo.get_or_compute(Uuid::from_u128(1), now, || {
            computed.set(computed.get() + 1);
            None
        });
        assert_eq!(result, None);
    }
    let other = memo.get_or_compute(Uuid::from_u128(2), now, || Some(request(2, 5)));

    assert_eq!(computed.get(), 1);
    assert_eq!(other.map(|r| r.asset_id), Some(Uuid::from_u128(2)));
}

#[test]
fn request_memo_drops_expired_entries_once_it_grows() {
    let mut memo = ThumbnailRequestMemo::new(Duration::from_secs(1));
    let start = Instant::now();
    for id in 0..ThumbnailRequestMemo::PRUNE_THRESHOLD as u128 {
        memo.get_or_compute(Uuid::from_u128(id), start, || None);
    }
    assert_eq!(memo.len(), ThumbnailRequestMemo::PRUNE_THRESHOLD);

    memo.get_or_compute(
        Uuid::from_u128(u128::MAX),
        start + Duration::from_secs(2),
        || None,
    );

    assert_eq!(memo.len(), 1);
}

#[test]
fn handle_round_trips_through_the_egui_context() {
    let ctx = egui::Context::default();
    assert!(ThumbnailHandle::from_ctx(&ctx).is_none());

    let service = ThumbnailService::default();
    let handle = service.handle();
    handle.install(&ctx);

    let found = ThumbnailHandle::from_ctx(&ctx).expect("installed handle should be found");
    assert!(found.ptr_eq(&handle));
}

#[test]
fn handle_requests_supported_assets_and_skips_unsupported_ones() {
    let root = std::env::temp_dir().join(format!("calyx-thumbnail-handle-{}", Uuid::new_v4()));
    let texture_dir = root.join("textures");
    fs::create_dir_all(&texture_dir).expect("failed to create texture dir");
    let texture_path = texture_dir.join("source.png");
    fs::write(&texture_path, b"a").expect("failed to write texture source");

    let context = calyx_engine::test_support::test_asset_context_with_assets(vec![root.clone()]);
    let registry = context.registries.assets.read();
    let texture_id = registry
        .asset_id("textures/source")
        .expect("texture asset should be registered");

    let service = ThumbnailService::default();
    let handle = service.handle();

    assert_eq!(
        handle.view_for_path(&registry, &texture_path, ThumbnailPriority::High),
        ThumbnailView::Loading
    );
    let request = ThumbnailRequest::from_asset_path(&registry, &texture_path).unwrap();
    assert_eq!(request.asset_id, texture_id);
    assert_eq!(
        service.status(request.key()),
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::High
        }
    );
    assert_eq!(
        handle.view_for_asset(&registry, texture_id, ThumbnailPriority::Low),
        ThumbnailView::Loading
    );
    assert_eq!(
        handle.view_for_asset(&registry, Uuid::new_v4(), ThumbnailPriority::High),
        ThumbnailView::Unsupported
    );
    assert_eq!(
        handle.view_for_asset(&registry, Uuid::nil(), ThumbnailPriority::High),
        ThumbnailView::Unsupported
    );

    drop(registry);
    fs::remove_dir_all(root).expect("failed to remove temp asset root");
}
