//! GPU timings per render pass through wgpu timestamp queries.
//!
//! [`GpuScope`] writes a timestamp into a command encoder when it opens and
//! another when it closes. Each frame's timestamps go into one of a few
//! query sets; at the frame's end they are resolved and copied into a
//! buffer that is mapped once the GPU has finished, a frame or more later,
//! and read back into [`ScopeSample`]s.
//!
//! Timing needs `TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_ENCODERS`;
//! the UI pass, which egui records, also needs
//! `TIMESTAMP_QUERY_INSIDE_PASSES`. Without them the scopes record nothing
//! and [`status`] says which feature is missing.

use std::borrow::Cow;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use egui_wgpu::wgpu;

use super::report::{GpuTimingStatus, ScopeSample};

/// Timestamps one frame can hold: two per scope.
const QUERY_CAPACITY: u32 = 1024;
/// Frames whose timestamps can be in flight at once. A frame that finds
/// every slot still in flight records no GPU timings.
const SLOT_COUNT: usize = 4;

const MAP_PENDING: u8 = 0;
const MAP_DONE: u8 = 1;
const MAP_FAILED: u8 = 2;

static TIMER: Mutex<Option<GpuTimer>> = Mutex::new(None);
static STATUS: Mutex<Option<GpuTimingStatus>> = Mutex::new(None);

/// Which GPU timings the device can record. Before [`install`] there is
/// no device to time.
pub fn status() -> GpuTimingStatus {
    STATUS
        .lock()
        .ok()
        .and_then(|status| status.clone())
        .unwrap_or_else(|| GpuTimingStatus {
            available: false,
            reason: Some("no GPU device was set up for profiling".to_owned()),
        })
}

/// Sets up GPU timing on `device` when it has the timestamp features. Only
/// scopes opened on the calling thread record, so renderers of other
/// threads (thumbnails, tests) never write into this device's queries.
pub fn install(device: &wgpu::Device, queue: &wgpu::Queue) {
    let (status, pass_timestamps) = timing_status(device.features());
    match &status.reason {
        Some(reason) => log::info!("GPU profiling: {reason}"),
        None => log::info!("GPU profiling available"),
    }
    if let Some(pass_timestamps) = pass_timestamps {
        if let Ok(mut timer) = TIMER.lock() {
            *timer = Some(GpuTimer::new(device, queue, pass_timestamps));
        }
    }
    if let Ok(mut current) = STATUS.lock() {
        *current = Some(status);
    }
}

/// Which timings a device with `features` can record, and, when it can
/// time passes at all, whether it can time the UI pass too.
fn timing_status(features: wgpu::Features) -> (GpuTimingStatus, Option<bool>) {
    let missing = [
        wgpu::Features::TIMESTAMP_QUERY,
        wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    ]
    .into_iter()
    .find(|feature| !features.contains(*feature));
    if let Some(feature) = missing {
        return (
            GpuTimingStatus {
                available: false,
                reason: Some(format!("the GPU device lacks {feature:?}")),
            },
            None,
        );
    }
    let pass_timestamps = features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
    (
        GpuTimingStatus {
            available: true,
            reason: (!pass_timestamps).then(|| {
                "the GPU device lacks TIMESTAMP_QUERY_INSIDE_PASSES, so the UI pass is not timed"
                    .to_owned()
            }),
        },
        Some(pass_timestamps),
    )
}

/// Resolves the timestamps of the frame `frame` and starts reading them back.
pub(super) fn end_frame(frame: u64) {
    if let Ok(mut timer) = TIMER.lock() {
        if let Some(timer) = timer.as_mut() {
            timer.end_frame(frame);
        }
    }
}

/// Returns the frames whose timestamps have been read back, as
/// `(frame, passes)`, oldest first.
pub(super) fn collect() -> Vec<(u64, Vec<ScopeSample>)> {
    match TIMER.lock() {
        Ok(mut timer) => timer.as_mut().map(GpuTimer::collect).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Whether the UI pass can be timed this frame.
pub(super) fn ui_timing_available() -> bool {
    super::is_capturing()
        && TIMER
            .lock()
            .ok()
            .is_some_and(|timer| timer.as_ref().is_some_and(|timer| timer.pass_timestamps))
}

/// Something timestamps can be written into.
trait TimestampWriter {
    fn write_timestamp(&mut self, query_set: &wgpu::QuerySet, index: u32);
    fn is_pass(&self) -> bool;
}

impl TimestampWriter for wgpu::CommandEncoder {
    fn write_timestamp(&mut self, query_set: &wgpu::QuerySet, index: u32) {
        wgpu::CommandEncoder::write_timestamp(self, query_set, index);
    }

    fn is_pass(&self) -> bool {
        false
    }
}

impl TimestampWriter for wgpu::RenderPass<'_> {
    fn write_timestamp(&mut self, query_set: &wgpu::QuerySet, index: u32) {
        wgpu::RenderPass::write_timestamp(self, query_set, index);
    }

    fn is_pass(&self) -> bool {
        true
    }
}

/// An open GPU scope of the frame being recorded: which recording it
/// belongs to and its index in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QueryId {
    generation: u64,
    index: usize,
}

fn begin(
    writer: &mut impl TimestampWriter,
    label: Cow<'static, str>,
    parent: Option<QueryId>,
) -> Option<QueryId> {
    let mut timer = TIMER.lock().ok()?;
    timer.as_mut()?.begin(writer, label, parent)
}

fn end(writer: &mut impl TimestampWriter, id: QueryId) {
    if let Ok(mut timer) = TIMER.lock() {
        if let Some(timer) = timer.as_mut() {
            timer.end(writer, id);
        }
    }
}

/// A timed stretch of a command encoder: it writes a timestamp when it
/// opens and another when it is dropped, so the GPU time of the commands
/// recorded through it is reported under its label. It dereferences to the
/// encoder, so pass it wherever a `&mut wgpu::CommandEncoder` is expected.
///
/// While capture is off it records nothing; opening one then costs an
/// atomic load.
pub struct GpuScope<'a> {
    encoder: &'a mut wgpu::CommandEncoder,
    id: Option<QueryId>,
}

impl<'a> GpuScope<'a> {
    /// Opens a scope named `label`, nested in the innermost scope still
    /// open on this thread, if any, as CPU scopes nest.
    pub fn new(encoder: &'a mut wgpu::CommandEncoder, label: &'static str) -> Self {
        Self::open(encoder, None, || Cow::Borrowed(label))
    }

    /// Opens a scope like [`Self::new`] whose name is only built while
    /// capturing.
    pub fn new_with(encoder: &'a mut wgpu::CommandEncoder, label: impl FnOnce() -> String) -> Self {
        Self::open(encoder, None, || Cow::Owned(label()))
    }

    /// Opens a scope nested in this one.
    pub fn scope(&mut self, label: &'static str) -> GpuScope<'_> {
        GpuScope::open(self.encoder, self.id, || Cow::Borrowed(label))
    }

    /// Opens a scope nested in this one, whose name is only built while
    /// capturing.
    pub fn scope_with(&mut self, label: impl FnOnce() -> String) -> GpuScope<'_> {
        GpuScope::open(self.encoder, self.id, || Cow::Owned(label()))
    }

    fn open(
        encoder: &'a mut wgpu::CommandEncoder,
        parent: Option<QueryId>,
        label: impl FnOnce() -> Cow<'static, str>,
    ) -> Self {
        let id = if super::is_capturing() {
            begin(encoder, label(), parent)
        } else {
            None
        };
        Self { encoder, id }
    }
}

impl Deref for GpuScope<'_> {
    type Target = wgpu::CommandEncoder;

    fn deref(&self) -> &Self::Target {
        self.encoder
    }
}

impl DerefMut for GpuScope<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.encoder
    }
}

impl Drop for GpuScope<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            end(self.encoder, id);
        }
    }
}

/// A timed stretch inside a render pass, for the draws of one kind a pass
/// records, such as one shader's, so a pass's time can be split between
/// them. It nests in the innermost scope open on this thread, as
/// [`GpuScope`] does, and is closed with [`PassScope::end`] on the same
/// pass.
///
/// It records nothing while capture is off or when the device cannot write
/// timestamps inside a pass (`TIMESTAMP_QUERY_INSIDE_PASSES`); the pass's
/// own [`GpuScope`] still times it whole. GPUs overlap the draws of a pass,
/// so the stretches are approximate: their sum can differ from the pass's
/// time, and a stretch includes the tail of the work before it.
#[must_use = "a pass scope records nothing until it is ended"]
pub struct PassScope {
    id: Option<QueryId>,
}

impl PassScope {
    /// Opens a stretch named `label` in `pass`; the name is only built while
    /// capturing.
    pub fn begin(pass: &mut wgpu::RenderPass<'_>, label: impl FnOnce() -> String) -> Self {
        let id = if super::is_capturing() {
            begin(pass, Cow::Owned(label()), None)
        } else {
            None
        };
        Self { id }
    }

    /// Closes the stretch in `pass`, the pass it was opened in.
    pub fn end(self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(id) = self.id {
            end(pass, id);
        }
    }
}

/// The UI scope opened by the first UI paint callback of a frame, closed by
/// the last.
static UI_QUERY: Mutex<Option<QueryId>> = Mutex::new(None);

/// Times the egui pass of the current frame as `UI`, through two paint
/// callbacks: one painted before everything else and one after. Call it
/// once per frame, before any panel is shown. It adds nothing while
/// capture is off or the device cannot write timestamps inside a pass.
pub fn time_ui_pass(ctx: &egui::Context) {
    if !ui_timing_available() {
        return;
    }
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0));
    ctx.layer_painter(egui::LayerId::background())
        .add(egui_wgpu::Callback::new_paint_callback(
            rect,
            UiTimestamp::Begin,
        ));
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Debug,
        egui::Id::new("calyx_profiler_ui_end"),
    ))
    .add(egui_wgpu::Callback::new_paint_callback(
        rect,
        UiTimestamp::End,
    ));
}

enum UiTimestamp {
    Begin,
    End,
}

impl egui_wgpu::CallbackTrait for UiTimestamp {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        let Ok(mut ui_query) = UI_QUERY.lock() else {
            return;
        };
        match self {
            UiTimestamp::Begin => {
                *ui_query = begin(render_pass, Cow::Borrowed("UI"), None);
            }
            UiTimestamp::End => {
                if let Some(id) = ui_query.take() {
                    end(render_pass, id);
                }
            }
        }
    }
}

struct GpuQuery {
    label: Cow<'static, str>,
    parent: Option<usize>,
    start: u32,
    ended: bool,
}

struct Recording {
    slot: usize,
    generation: u64,
    queries: Vec<GpuQuery>,
    /// The scopes open now, innermost last: a scope opened without a parent
    /// nests in the innermost, as CPU scopes do.
    open: Vec<usize>,
    used: u32,
}

enum SlotState {
    Free,
    Recording,
    Mapping {
        frame: u64,
        queries: Vec<GpuQuery>,
        used: u32,
        mapped: Arc<AtomicU8>,
    },
}

struct Slot {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    state: SlotState,
}

struct GpuTimer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    owner: ThreadId,
    pass_timestamps: bool,
    slots: Vec<Slot>,
    recording: Option<Recording>,
    /// The frame that already found no free slot, so it is not looked for
    /// again on every scope.
    starved: bool,
    generation: u64,
}

impl GpuTimer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, pass_timestamps: bool) -> Self {
        let size = u64::from(QUERY_CAPACITY) * u64::from(wgpu::QUERY_SIZE);
        let slots = (0..SLOT_COUNT)
            .map(|_| Slot {
                query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("profiler_timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: QUERY_CAPACITY,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("profiler_resolve"),
                    size,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                read: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("profiler_read"),
                    size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: SlotState::Free,
            })
            .collect();
        Self {
            device: device.clone(),
            queue: queue.clone(),
            owner: std::thread::current().id(),
            pass_timestamps,
            slots,
            recording: None,
            starved: false,
            generation: 0,
        }
    }

    fn begin(
        &mut self,
        writer: &mut impl TimestampWriter,
        label: Cow<'static, str>,
        parent: Option<QueryId>,
    ) -> Option<QueryId> {
        if std::thread::current().id() != self.owner || (writer.is_pass() && !self.pass_timestamps)
        {
            return None;
        }
        if self.recording.is_none() {
            if self.starved {
                return None;
            }
            let Some(slot) = self
                .slots
                .iter()
                .position(|slot| matches!(slot.state, SlotState::Free))
            else {
                self.starved = true;
                return None;
            };
            self.slots[slot].state = SlotState::Recording;
            self.generation += 1;
            self.recording = Some(Recording {
                slot,
                generation: self.generation,
                queries: Vec::new(),
                open: Vec::new(),
                used: 0,
            });
        }
        let recording = self.recording.as_mut()?;
        if recording.used + 2 > QUERY_CAPACITY {
            return None;
        }
        let parent = parent
            .filter(|parent| parent.generation == recording.generation)
            .map(|parent| parent.index)
            .or_else(|| recording.open.last().copied());
        writer.write_timestamp(&self.slots[recording.slot].query_set, recording.used);
        recording.queries.push(GpuQuery {
            label,
            parent,
            start: recording.used,
            ended: false,
        });
        recording.used += 2;
        recording.open.push(recording.queries.len() - 1);
        Some(QueryId {
            generation: recording.generation,
            index: recording.queries.len() - 1,
        })
    }

    fn end(&mut self, writer: &mut impl TimestampWriter, id: QueryId) {
        let Some(recording) = self.recording.as_mut() else {
            return;
        };
        if recording.generation != id.generation {
            return;
        }
        let Some(query) = recording.queries.get_mut(id.index) else {
            return;
        };
        if !query.ended {
            writer.write_timestamp(&self.slots[recording.slot].query_set, query.start + 1);
            query.ended = true;
        }
        if let Some(open) = recording.open.iter().rposition(|&index| index == id.index) {
            recording.open.remove(open);
        }
    }

    fn end_frame(&mut self, frame: u64) {
        self.starved = false;
        let Some(recording) = self.recording.take() else {
            return;
        };
        let slot = &mut self.slots[recording.slot];
        if recording.used == 0 {
            slot.state = SlotState::Free;
            return;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("profiler_resolve"),
            });
        // A scope left open (a UI pass that never reached its last
        // callback) gets an end now so no unwritten query is resolved; it
        // is reported as unfinished and dropped.
        for query in recording.queries.iter().filter(|query| !query.ended) {
            encoder.write_timestamp(&slot.query_set, query.start + 1);
        }
        let bytes = u64::from(recording.used) * u64::from(wgpu::QUERY_SIZE);
        encoder.resolve_query_set(&slot.query_set, 0..recording.used, &slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.read, 0, bytes);
        self.queue.submit(Some(encoder.finish()));
        let mapped = Arc::new(AtomicU8::new(MAP_PENDING));
        let on_mapped = mapped.clone();
        slot.read
            .slice(0..bytes)
            .map_async(wgpu::MapMode::Read, move |result| {
                on_mapped.store(
                    if result.is_ok() { MAP_DONE } else { MAP_FAILED },
                    Ordering::Release,
                );
            });
        slot.state = SlotState::Mapping {
            frame,
            queries: recording.queries,
            used: recording.used,
            mapped,
        };
    }

    fn collect(&mut self) -> Vec<(u64, Vec<ScopeSample>)> {
        // Nothing in flight, the case whenever capture is off: no poll.
        if !self
            .slots
            .iter()
            .any(|slot| matches!(slot.state, SlotState::Mapping { .. }))
        {
            return Vec::new();
        }
        let _ = self.device.poll(wgpu::Maintain::Poll);
        let period = f64::from(self.queue.get_timestamp_period());
        let mut frames = Vec::new();
        for slot in &mut self.slots {
            let SlotState::Mapping { mapped, .. } = &slot.state else {
                continue;
            };
            let state = mapped.load(Ordering::Acquire);
            if state == MAP_PENDING {
                continue;
            }
            let SlotState::Mapping {
                frame,
                queries,
                used,
                ..
            } = std::mem::replace(&mut slot.state, SlotState::Free)
            else {
                unreachable!("the slot was just matched as mapping");
            };
            if state == MAP_FAILED {
                continue;
            }
            let bytes = u64::from(used) * u64::from(wgpu::QUERY_SIZE);
            let timestamps: Vec<u64> = {
                let view = slot.read.slice(0..bytes).get_mapped_range();
                view.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|chunk| u64::from_le_bytes(*chunk))
                    .collect()
            };
            slot.read.unmap();
            frames.push((frame, samples_of(&queries, &timestamps, period)));
        }
        frames.sort_by_key(|(frame, _)| *frame);
        frames
    }
}

/// Builds the scope tree of a frame's queries from their timestamps, in
/// ticks of `period` nanoseconds. Scopes that never ended are dropped with
/// their children.
fn samples_of(queries: &[GpuQuery], timestamps: &[u64], period: f64) -> Vec<ScopeSample> {
    fn children_of(
        parent: Option<usize>,
        queries: &[GpuQuery],
        timestamps: &[u64],
        period: f64,
    ) -> Vec<ScopeSample> {
        let mut samples = Vec::new();
        for (index, query) in queries.iter().enumerate() {
            if query.parent != parent || !query.ended {
                continue;
            }
            let start = timestamps.get(query.start as usize).copied().unwrap_or(0);
            let end = timestamps
                .get(query.start as usize + 1)
                .copied()
                .unwrap_or(start);
            let ns = (end.saturating_sub(start) as f64 * period) as u64;
            let children = children_of(Some(index), queries, timestamps, period);
            ScopeSample::merge_into(
                &mut samples,
                ScopeSample::new(query.label.clone(), ns, children),
            );
        }
        samples
    }
    children_of(None, queries, timestamps, period)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_without_timestamps_reports_which_feature_it_lacks() {
        let (status, timer) = timing_status(wgpu::Features::empty());
        assert!(!status.available);
        assert_eq!(timer, None);
        assert!(status.reason.unwrap().contains("TIMESTAMP_QUERY"));

        let (status, timer) = timing_status(wgpu::Features::TIMESTAMP_QUERY);
        assert!(!status.available);
        assert_eq!(timer, None);
        assert!(status
            .reason
            .unwrap()
            .contains("TIMESTAMP_QUERY_INSIDE_ENCODERS"));

        let (status, timer) = timing_status(
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
        );
        assert!(status.available);
        assert_eq!(
            timer,
            Some(false),
            "the UI pass needs timestamps inside passes"
        );
        assert!(status.reason.unwrap().contains("UI pass"));
    }

    /// Times a clear pass on the test device, when it has the timestamp
    /// features, through a timer of its own rather than the global one.
    /// The inner scope names no parent and nests in the one open around it.
    #[test]
    fn a_timed_pass_reads_back_as_a_nested_scope() {
        let context = crate::test_support::test_render_context();
        let device = context.device();
        let (_, Some(pass_timestamps)) = timing_status(device.features()) else {
            eprintln!("skipped: the test device has no timestamp queries");
            return;
        };
        let mut timer = GpuTimer::new(device, context.queue(), pass_timestamps);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        let outer = timer
            .begin(&mut encoder, Cow::Borrowed("Frame"), None)
            .expect("a free slot");
        let inner = timer
            .begin(&mut encoder, Cow::Borrowed("Clear"), None)
            .expect("room for a second scope");
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        timer.end(&mut encoder, inner);
        timer.end(&mut encoder, outer);
        context.queue().submit(Some(encoder.finish()));
        timer.end_frame(5);
        let _ = device.poll(wgpu::Maintain::Wait);

        let frames = timer.collect();
        assert_eq!(frames.len(), 1);
        let (frame, passes) = &frames[0];
        assert_eq!(*frame, 5);
        assert_eq!(passes.len(), 1);
        assert_eq!(passes[0].name, "Frame");
        assert_eq!(passes[0].children[0].name, "Clear");
        assert!(passes[0].children[0].ns <= passes[0].ns);
    }

    /// A stretch timed inside a render pass, as [`PassScope`] times one
    /// shader's draws, nests in the encoder scope open around the pass.
    #[test]
    fn a_stretch_inside_a_pass_reads_back_nested_in_the_scope_around_it() {
        let context = crate::test_support::test_render_context();
        let device = context.device();
        let (_, Some(true)) = timing_status(device.features()) else {
            eprintln!("skipped: the test device has no timestamps inside passes");
            return;
        };
        let mut timer = GpuTimer::new(device, context.queue(), true);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        let pass_scope = timer
            .begin(&mut encoder, Cow::Borrowed("Forward"), None)
            .expect("a free slot");
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let stretch = timer
                .begin(&mut pass, Cow::Borrowed("water"), None)
                .expect("room for a second scope");
            timer.end(&mut pass, stretch);
        }
        timer.end(&mut encoder, pass_scope);
        context.queue().submit(Some(encoder.finish()));
        timer.end_frame(7);
        let _ = device.poll(wgpu::Maintain::Wait);

        let frames = timer.collect();
        let (_, passes) = &frames[0];
        assert_eq!(passes[0].name, "Forward");
        assert_eq!(passes[0].children[0].name, "water");
    }

    fn query(label: &'static str, parent: Option<usize>, start: u32, ended: bool) -> GpuQuery {
        GpuQuery {
            label: Cow::Borrowed(label),
            parent,
            start,
            ended,
        }
    }

    #[test]
    fn timestamps_become_a_tree_of_passes_in_nanoseconds() {
        let queries = [
            query("Scene view", None, 0, true),
            query("Shadows", Some(0), 2, true),
            query("Lighting", Some(0), 4, true),
            query("Shadows", Some(0), 6, true),
            query("UI", None, 8, false),
        ];
        let timestamps = [100, 400, 110, 150, 150, 300, 300, 320, 400, 500];
        let samples = samples_of(&queries, &timestamps, 2.0);
        assert_eq!(samples.len(), 1, "the unfinished UI scope is dropped");
        let scene = &samples[0];
        assert_eq!((scene.name.as_str(), scene.ns), ("Scene view", 600));
        let children: Vec<(&str, u64, u32)> = scene
            .children
            .iter()
            .map(|child| (child.name.as_str(), child.ns, child.calls))
            .collect();
        assert_eq!(children, [("Shadows", 120, 2), ("Lighting", 300, 1)]);
    }
}
