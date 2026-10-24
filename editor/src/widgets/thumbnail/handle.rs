//! UI-facing access to the thumbnail service.
//!
//! [`ThumbnailHandle`] is a cheap clone of the service's shared state. The
//! editor installs it into the egui context every frame, so any widget with a
//! `Ui` (asset selectors in inspectors, the content browser) can request a
//! thumbnail and read its texture without reaching `EditorAppState`.

use super::pipeline::{ThumbnailPriority, ThumbnailStatus};
use super::service::{request_thumbnail, thumbnail_texture_id, ThumbnailShared};
use super::*;
use egui::Ui;
use std::time::Duration;

/// How long a memoized [`ThumbnailRequest`] is reused before its source
/// version is computed again. Source edits show up after at most this long.
const THUMBNAIL_REQUEST_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// What an asset UI should draw for one asset's thumbnail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThumbnailView {
    /// The thumbnail texture is ready to draw.
    Ready(egui::TextureId),
    /// The thumbnail is queued or rendering; draw the fallback icon with a
    /// loading indicator.
    Loading,
    /// Generation failed for good; draw the fallback icon and offer the
    /// message on hover.
    Failed(String),
    /// The asset is unknown or its type has no thumbnail; draw the fallback
    /// icon only.
    Unsupported,
}

/// The indicator drawn over a fallback icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbnailBadge {
    None,
    Loading,
    Failed,
}

impl ThumbnailView {
    pub fn texture_id(&self) -> Option<egui::TextureId> {
        match self {
            Self::Ready(id) => Some(*id),
            _ => None,
        }
    }

    pub fn badge(&self) -> ThumbnailBadge {
        match self {
            Self::Loading => ThumbnailBadge::Loading,
            Self::Failed(_) => ThumbnailBadge::Failed,
            Self::Ready(_) | Self::Unsupported => ThumbnailBadge::None,
        }
    }

    /// Hover text explaining a fallback icon, if any.
    pub fn hover_text(&self) -> Option<String> {
        match self {
            Self::Loading => Some("Generating thumbnail…".into()),
            Self::Failed(message) => Some(format!("Thumbnail failed: {message}")),
            Self::Ready(_) | Self::Unsupported => None,
        }
    }

    /// Draws this thumbnail into `rect`: the texture when ready, otherwise
    /// `fallback` with the badge for this state.
    pub fn paint(&self, ui: &Ui, rect: egui::Rect, fallback: egui::ImageSource<'_>) {
        match self.texture_id() {
            Some(id) => {
                ui.painter().image(
                    id,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                egui::Image::new(fallback).paint_at(ui, rect);
                paint_thumbnail_badge(ui, rect, self.badge());
            }
        }
    }
}

/// Draws `badge` over a fallback icon occupying `rect`.
pub(crate) fn paint_thumbnail_badge(ui: &Ui, rect: egui::Rect, badge: ThumbnailBadge) {
    match badge {
        ThumbnailBadge::None => {}
        ThumbnailBadge::Loading => {
            let size = (rect.width().min(rect.height()) * 0.4).clamp(8.0, 32.0);
            let spinner_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
            egui::Spinner::new().size(size).paint_at(ui, spinner_rect);
        }
        ThumbnailBadge::Failed => {
            let radius = (rect.width().min(rect.height()) * 0.16).clamp(3.0, 12.0);
            let center = rect.right_bottom() - egui::vec2(radius, radius);
            let painter = ui.painter();
            painter.circle_filled(center, radius, ui.visuals().error_fg_color);
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "!",
                egui::FontId::proportional(radius * 1.6),
                Color32::WHITE,
            );
        }
    }
}

/// Maps a request's pipeline status, or `None` for an asset with no
/// thumbnail, to what the UI draws.
pub(super) fn thumbnail_view(
    status: Option<ThumbnailStatus>,
    texture_id: Option<egui::TextureId>,
) -> ThumbnailView {
    match status {
        None => ThumbnailView::Unsupported,
        Some(ThumbnailStatus::Ready) => texture_id
            .map(ThumbnailView::Ready)
            .unwrap_or(ThumbnailView::Loading),
        Some(ThumbnailStatus::Failed { message }) => ThumbnailView::Failed(message),
        Some(
            ThumbnailStatus::Missing | ThumbnailStatus::Queued { .. } | ThumbnailStatus::InProgress,
        ) => ThumbnailView::Loading,
    }
}

/// Reuses each asset's [`ThumbnailRequest`] for a refresh interval, so UI code
/// that asks every frame does not reread and rehash the asset's files every
/// frame.
pub(super) struct ThumbnailRequestMemo {
    entries: HashMap<Uuid, ThumbnailRequestMemoEntry>,
    refresh_interval: Duration,
}

struct ThumbnailRequestMemoEntry {
    request: Option<ThumbnailRequest>,
    computed_at: Instant,
}

impl Default for ThumbnailRequestMemo {
    fn default() -> Self {
        Self::new(THUMBNAIL_REQUEST_REFRESH_INTERVAL)
    }
}

impl ThumbnailRequestMemo {
    /// Entry count past which inserting a new entry first drops expired ones.
    pub(super) const PRUNE_THRESHOLD: usize = 4096;

    pub(super) fn new(refresh_interval: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            refresh_interval,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns the memoized request for `asset_id`, calling `compute` when there
    /// is none or it is at least one refresh interval old at `now`.
    pub(super) fn get_or_compute(
        &mut self,
        asset_id: Uuid,
        now: Instant,
        compute: impl FnOnce() -> Option<ThumbnailRequest>,
    ) -> Option<ThumbnailRequest> {
        if let Some(entry) = self.entries.get(&asset_id) {
            if now.saturating_duration_since(entry.computed_at) < self.refresh_interval {
                return entry.request.clone();
            }
        }

        if self.entries.len() >= Self::PRUNE_THRESHOLD {
            let refresh_interval = self.refresh_interval;
            self.entries.retain(|_, entry| {
                now.saturating_duration_since(entry.computed_at) < refresh_interval
            });
        }

        let request = compute();
        self.entries.insert(
            asset_id,
            ThumbnailRequestMemoEntry {
                request: request.clone(),
                computed_at: now,
            },
        );
        request
    }
}

/// A cloneable handle to the thumbnail service, reachable from any `Ui`.
#[derive(Clone)]
pub struct ThumbnailHandle {
    shared: Arc<ThumbnailShared>,
}

impl ThumbnailHandle {
    pub(super) fn new(shared: Arc<ThumbnailShared>) -> Self {
        Self { shared }
    }

    fn context_id() -> egui::Id {
        egui::Id::new("calyx_editor_thumbnail_handle")
    }

    /// Makes this handle reachable through [`Self::from_ctx`] on `ctx`.
    pub fn install(&self, ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(Self::context_id(), self.clone()));
    }

    /// Returns the handle the editor installed on `ctx`, if any.
    pub fn from_ctx(ctx: &egui::Context) -> Option<Self> {
        ctx.data(|data| data.get_temp::<Self>(Self::context_id()))
    }

    /// Whether both handles point at the same service.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// Requests the thumbnail of the asset at `path` and returns what to draw.
    pub fn view_for_path(
        &self,
        registry: &AssetRegistry,
        path: &Path,
        priority: ThumbnailPriority,
    ) -> ThumbnailView {
        match registry.asset_id_from_path(path) {
            Some(asset_id) => self.view_for_asset(registry, asset_id, priority),
            None => ThumbnailView::Unsupported,
        }
    }

    /// Requests the thumbnail of the asset `asset_id` and returns what to draw.
    /// Repeated calls are cheap: the request is memoized and the pipeline
    /// deduplicates it, raising its priority when `priority` is higher.
    pub fn view_for_asset(
        &self,
        registry: &AssetRegistry,
        asset_id: Uuid,
        priority: ThumbnailPriority,
    ) -> ThumbnailView {
        if asset_id.is_nil() {
            return ThumbnailView::Unsupported;
        }
        let request = {
            let mut memo = self
                .shared
                .requests
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            memo.get_or_compute(asset_id, Instant::now(), || {
                ThumbnailRequest::from_asset_id(registry, asset_id)
            })
        };
        let Some(request) = request else {
            return ThumbnailView::Unsupported;
        };
        let key = request.key();
        let status = request_thumbnail(&self.shared, request, priority);
        thumbnail_view(Some(status), thumbnail_texture_id(&self.shared, key))
    }
}
