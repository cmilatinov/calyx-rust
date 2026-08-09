use std::collections::VecDeque;
use std::path::PathBuf;

use engine::render::{ReadbackPoll, TextureReadback};
use remote_protocol::{ErrorKind, ResponseEnvelope};

use super::server::{ClientId, RemoteServer};

/// How many frames a GPU texture readback may stay pending before the
/// screenshot request fails with a timeout.
const TEXTURE_READBACK_DEADLINE_FRAMES: u64 = 120;

/// Address of a response that will be sent once a deferred operation
/// completes.
#[derive(Clone, Copy)]
pub struct ResponseSlot {
    pub client: ClientId,
    pub request_id: u64,
}

/// A deferred operation completed by the frame loop rather than by the
/// command handler that created it.
pub enum PendingOp {
    /// Counting down simulated frames for `step_frames`; pauses on zero.
    StepFrames { slot: ResponseSlot, remaining: u32 },
    /// Waiting until every scheduled synthetic input event has been delivered.
    /// Tracked in raw-input hooks rather than frames because scheduling happens
    /// after the current frame's hook has already run.
    InputDrain {
        slot: ResponseSlot,
        done_at_hook: u64,
    },
    /// Waiting for a number of frames to elapse.
    WaitFrames {
        slot: ResponseSlot,
        done_at_frame: u64,
    },
    /// Waiting for a scene/game texture readback to complete, then writing the
    /// PNG.
    TextureShot {
        slot: ResponseSlot,
        path: PathBuf,
        readback: TextureReadback,
        deadline_frame: u64,
    },
}

impl PendingOp {
    /// Returns the response slot this operation will answer.
    pub fn slot(&self) -> &ResponseSlot {
        match self {
            PendingOp::StepFrames { slot, .. }
            | PendingOp::InputDrain { slot, .. }
            | PendingOp::WaitFrames { slot, .. }
            | PendingOp::TextureShot { slot, .. } => slot,
        }
    }
}

/// A full-window screenshot in flight, matched against
/// [`egui::Event::Screenshot`] by the request id stored in the viewport
/// command's user data.
pub struct WindowShot {
    pub slot: ResponseSlot,
    pub path: PathBuf,
}

/// Main-thread state of the remote control layer, owned by the editor app.
pub struct RemoteRuntime {
    pub server: RemoteServer,
    /// Project directory the editor was launched with.
    pub project_path: PathBuf,
    /// Monotonic frame counter, incremented at the end of every editor frame.
    pub frame_index: u64,
    /// Number of raw-input hooks that have run. Input delivery is tracked
    /// against this rather than `frame_index`, whose increment happens at the
    /// end of the frame in which a script is scheduled.
    pub hooks_run: u64,
    /// Synthetic egui events scheduled per upcoming frame; the front entry is
    /// delivered on the next `raw_input_hook` call.
    pub input_schedule: VecDeque<Vec<egui::Event>>,
    /// Deferred operations polled at the end of every frame.
    pub pending: Vec<PendingOp>,
    /// Full-window screenshots awaiting their `Event::Screenshot`.
    pub pending_window_shots: Vec<WindowShot>,
    /// Game panel image rect (egui points) captured at the end of the last
    /// frame the Game tab was visible.
    pub last_game_rect: Option<egui::Rect>,
    /// Viewport panel image rect captured the same way.
    pub last_viewport_rect: Option<egui::Rect>,
}

impl RemoteRuntime {
    pub fn new(server: RemoteServer, project_path: PathBuf) -> Self {
        Self {
            server,
            project_path,
            frame_index: 0,
            hooks_run: 0,
            input_schedule: VecDeque::new(),
            pending: Vec::new(),
            pending_window_shots: Vec::new(),
            last_game_rect: None,
            last_viewport_rect: None,
        }
    }

    /// Sends a success response.
    pub fn respond_ok(&self, slot: &ResponseSlot, data: serde_json::Value) {
        self.server
            .respond(slot.client, &ResponseEnvelope::ok(slot.request_id, data));
    }

    /// Sends an error response.
    pub fn respond_error(&self, slot: &ResponseSlot, kind: ErrorKind, message: impl Into<String>) {
        self.server.respond(
            slot.client,
            &ResponseEnvelope::error(slot.request_id, kind, message),
        );
    }

    /// Merges per-frame event buckets into the schedule. Bucket `i` is
    /// delivered by the `i + 1`th raw-input hook from now. Returns the hook
    /// count at which the last bucket will have been delivered.
    pub fn schedule_events(&mut self, buckets: Vec<Vec<egui::Event>>) -> u64 {
        let bucket_count = buckets.len() as u64;
        for (offset, events) in buckets.into_iter().enumerate() {
            if self.input_schedule.len() <= offset {
                self.input_schedule.resize_with(offset + 1, Vec::new);
            }
            self.input_schedule[offset].extend(events);
        }
        self.hooks_run + bucket_count
    }

    /// Returns the events to inject into the current frame's raw input.
    pub fn take_scheduled_events(&mut self) -> Vec<egui::Event> {
        self.hooks_run += 1;
        self.input_schedule.pop_front().unwrap_or_default()
    }

    /// Whether a `step_frames` operation is already counting down. A second
    /// one would strand the first: whichever finishes earlier pauses the
    /// simulation, and the other then stops advancing.
    pub fn has_pending_step(&self) -> bool {
        self.pending
            .iter()
            .any(|op| matches!(op, PendingOp::StepFrames { .. }))
    }

    /// Registers a texture screenshot readback with the standard deadline.
    pub fn push_texture_shot(
        &mut self,
        slot: ResponseSlot,
        path: PathBuf,
        readback: TextureReadback,
    ) {
        let deadline_frame = self.frame_index + TEXTURE_READBACK_DEADLINE_FRAMES;
        self.pending.push(PendingOp::TextureShot {
            slot,
            path,
            readback,
            deadline_frame,
        });
    }

    /// Polls all deferred operations at the end of a frame. `simulated` tells
    /// whether the scene simulation advanced this frame. Returns `true` when a
    /// `step_frames` operation just finished and simulation should pause.
    pub fn poll_pending(
        &mut self,
        render_context: &engine::render::RenderContext,
        simulated: bool,
    ) -> bool {
        let frame_index = self.frame_index;
        let hooks_run = self.hooks_run;
        let mut pause_simulation = false;
        let mut finished = Vec::new();

        for (index, op) in self.pending.iter_mut().enumerate() {
            match op {
                PendingOp::StepFrames { remaining, .. } => {
                    if simulated && *remaining > 0 {
                        *remaining -= 1;
                    }
                    if *remaining == 0 {
                        pause_simulation = true;
                        finished.push((index, Ok(serde_json::Value::Null)));
                    }
                }
                PendingOp::InputDrain { done_at_hook, .. } => {
                    if hooks_run >= *done_at_hook {
                        finished.push((index, Ok(serde_json::Value::Null)));
                    }
                }
                PendingOp::WaitFrames { done_at_frame, .. } => {
                    if frame_index >= *done_at_frame {
                        finished.push((index, Ok(serde_json::Value::Null)));
                    }
                }
                PendingOp::TextureShot {
                    path,
                    readback,
                    deadline_frame,
                    ..
                } => match readback.try_finish(render_context) {
                    ReadbackPoll::Pending => {
                        if frame_index >= *deadline_frame {
                            finished.push((
                                index,
                                Err((
                                    ErrorKind::Timeout,
                                    "texture readback did not complete in time".to_string(),
                                )),
                            ));
                        }
                    }
                    ReadbackPoll::Failed(message) => {
                        finished.push((index, Err((ErrorKind::Internal, message))));
                    }
                    ReadbackPoll::Ready(image) => {
                        finished.push((index, save_png(path, image)));
                    }
                },
            }
        }

        for (index, result) in finished.into_iter().rev() {
            let op = self.pending.swap_remove(index);
            let slot = match &op {
                PendingOp::StepFrames { slot, .. }
                | PendingOp::InputDrain { slot, .. }
                | PendingOp::WaitFrames { slot, .. }
                | PendingOp::TextureShot { slot, .. } => slot,
            };
            match result {
                Ok(data) => self.respond_ok(slot, data),
                Err((kind, message)) => self.respond_error(slot, kind, message),
            }
        }

        pause_simulation
    }
}

/// Writes `image` to `path` as PNG, creating parent directories, and returns
/// the standard screenshot response payload.
pub fn save_png(
    path: &std::path::Path,
    image: image::RgbaImage,
) -> Result<serde_json::Value, (ErrorKind, String)> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let (width, height) = image.dimensions();
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|error| {
            (
                ErrorKind::Internal,
                format!("failed to write {}: {error}", path.display()),
            )
        })?;
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Ok(serde_json::json!({
        "path": absolute.display().to_string(),
        "width": width,
        "height": height,
    }))
}

/// Converts an egui screenshot into an owned RGBA image.
pub fn color_image_to_rgba(image: &egui::ColorImage) -> Option<image::RgbaImage> {
    let [width, height] = image.size;
    let mut pixels = Vec::with_capacity(width * height * 4);
    for color in &image.pixels {
        pixels.extend_from_slice(&color.to_array());
    }
    image::RgbaImage::from_raw(width as u32, height as u32, pixels)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::remote::server::RemoteConfig;

    /// A runtime backed by a real server on an ephemeral port. Deferred
    /// operations and input scheduling need no editor state.
    pub(crate) fn test_runtime() -> RemoteRuntime {
        let project_path =
            std::env::temp_dir().join(format!("calyx-runtime-test-{}", uuid::Uuid::new_v4()));
        let server = RemoteServer::start(RemoteConfig {
            port: 0,
            discovery_path: project_path.join(".calyx").join("remote.json"),
        })
        .expect("server should bind an ephemeral port");
        RemoteRuntime::new(server, project_path)
    }

    fn key_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::W,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }
    }

    #[test]
    fn input_drain_waits_for_every_scheduled_bucket() {
        let mut runtime = test_runtime();
        // Scheduling happens during `update`, after this frame's raw-input
        // hook has already run and popped its bucket.
        runtime.take_scheduled_events();

        let done_at_hook = runtime.schedule_events(vec![vec![key_event()], vec![key_event()]]);

        // Each later hook delivers one bucket, and the deadline must not be
        // reached until the last one has actually been delivered.
        for _ in 0..2 {
            assert!(
                runtime.hooks_run < done_at_hook,
                "drain completed while {} bucket(s) were still undelivered",
                runtime.input_schedule.len()
            );
            assert!(!runtime.take_scheduled_events().is_empty());
        }
        assert_eq!(runtime.hooks_run, done_at_hook);
        assert!(runtime.input_schedule.is_empty());
    }

    #[test]
    fn input_drain_deadline_is_independent_of_the_frame_counter() {
        let mut runtime = test_runtime();
        runtime.take_scheduled_events();
        let done_at_hook = runtime.schedule_events(vec![vec![key_event()]]);

        // Frames advancing without raw-input hooks must not complete a drain.
        for _ in 0..5 {
            runtime.frame_index += 1;
        }
        assert!(runtime.hooks_run < done_at_hook);

        runtime.take_scheduled_events();
        assert_eq!(runtime.hooks_run, done_at_hook);
    }

    #[test]
    fn concurrent_step_operations_are_detected() {
        let mut runtime = test_runtime();
        let slot = ResponseSlot {
            client: 1,
            request_id: 1,
        };
        assert!(!runtime.has_pending_step());

        runtime
            .pending
            .push(PendingOp::StepFrames { slot, remaining: 4 });
        assert!(runtime.has_pending_step());

        runtime.pending.clear();
        runtime.pending.push(PendingOp::WaitFrames {
            slot,
            done_at_frame: 10,
        });
        assert!(!runtime.has_pending_step());
    }
}
