//! Frame profiler: where each frame's time goes, as named CPU scopes per
//! thread and GPU timings per render pass.
//!
//! - CPU scopes are [puffin] scopes, opened with [`profile_scope!`] and
//!   [`profile_function!`]. Each records into its thread's own stream and
//!   nests in the scope open around it.
//! - GPU timings come from wgpu timestamp queries around each pass, opened
//!   with [`GpuScope`] (see [`gpu`]).
//! - The frame loop calls [`new_frame`] once per frame, first thing. It
//!   closes the previous frame into the history, which keeps the last
//!   [`HISTORY_FRAMES`] frames, and attaches GPU timings to their frame
//!   once the GPU reports them, usually a frame or two later.
//! - [`report`] and [`report_range`] aggregate a window of frames into a
//!   [`ProfileReport`]: averages and maximums of the frame time, the CPU
//!   and GPU time and every scope.
//!
//! Capture is off until [`set_capture`] turns it on, and a change applies
//! at the next frame boundary. While it is off a scope costs one atomic
//! load. Building with `RUSTFLAGS="--cfg calyx_no_profiling"` compiles the
//! CPU scope macros out, for measuring that cost.
//!
//! A project assembly (DLL) links its own copy of this module and of
//! puffin, whose capture stays off: scopes in project code record nothing.
//! The engine's scopes around component hooks and the HUD time project
//! code from the editor's side.

mod cpu;
pub mod gpu;
mod report;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

pub use gpu::{time_ui_pass, GpuScope};
pub use puffin;
pub use report::{
    aggregate, FrameSample, GpuTimingStatus, ProfileReport, ScopeSample, ScopeStats, ThreadSample,
    ThreadStats, TimeStats,
};

/// Whether the profiler is compiled in, that is, the build is not made
/// with `--cfg calyx_no_profiling`.
pub const COMPILED_IN: bool = !cfg!(calyx_no_profiling);

/// Frames the history keeps.
pub const HISTORY_FRAMES: usize = 600;

/// Opens a CPU scope named `$name`, with optional `$data` such as an
/// object or type name that is only evaluated while capturing, until the
/// end of the enclosing block.
#[cfg(not(calyx_no_profiling))]
#[macro_export]
macro_rules! profile_scope {
    ($($arg:tt)*) => {
        $crate::profiler::puffin::profile_scope!($($arg)*);
    };
}

/// Compiled out by `--cfg calyx_no_profiling`.
#[cfg(calyx_no_profiling)]
#[macro_export]
macro_rules! profile_scope {
    ($($arg:tt)*) => {};
}

/// Opens a CPU scope named after the enclosing function until its end.
#[cfg(not(calyx_no_profiling))]
#[macro_export]
macro_rules! profile_function {
    ($($arg:tt)*) => {
        $crate::profiler::puffin::profile_function!($($arg)*);
    };
}

/// Compiled out by `--cfg calyx_no_profiling`.
#[cfg(calyx_no_profiling)]
#[macro_export]
macro_rules! profile_function {
    ($($arg:tt)*) => {};
}

/// Capture as asked for; it takes effect at the next [`new_frame`].
static REQUESTED: AtomicBool = AtomicBool::new(false);
/// Capture of the frame running now.
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Count of [`set_capture`] calls, so a caller can tell whether anyone
/// else has asked for a change since its own.
static CAPTURE_CHANGES: AtomicU64 = AtomicU64::new(0);

/// Turns capture on or off from the next frame on, and returns the
/// [`capture_changes`] count this call made.
pub fn set_capture(on: bool) -> u64 {
    REQUESTED.store(on, Ordering::Relaxed);
    CAPTURE_CHANGES.fetch_add(1, Ordering::Relaxed) + 1
}

/// How many times capture has been asked to change.
pub fn capture_changes() -> u64 {
    CAPTURE_CHANGES.load(Ordering::Relaxed)
}

/// Whether capture is on or asked for from the next frame.
pub fn capture_requested() -> bool {
    REQUESTED.load(Ordering::Relaxed)
}

/// Whether the frame running now is captured.
#[inline]
pub fn is_capturing() -> bool {
    !cfg!(calyx_no_profiling) && CAPTURING.load(Ordering::Relaxed)
}

struct Profiler {
    /// Index of the frame running now.
    frame: u64,
    frame_start: Option<Instant>,
    main_thread: String,
    scopes: puffin::ScopeCollection,
    sink_installed: bool,
    history: VecDeque<FrameSample>,
}

static PROFILER: LazyLock<Mutex<Profiler>> = LazyLock::new(|| {
    Mutex::new(Profiler {
        frame: 0,
        frame_start: None,
        main_thread: String::new(),
        scopes: Default::default(),
        sink_installed: false,
        history: VecDeque::new(),
    })
});

/// Ends the frame running now and starts the next. Call it from the frame
/// loop's thread, once per frame, before any scope of the frame opens.
pub fn new_frame() {
    let Ok(mut profiler) = PROFILER.lock() else {
        return;
    };
    let now = Instant::now();
    let frame_ns = profiler
        .frame_start
        .map_or(0, |start| (now - start).as_nanos() as u64);
    profiler.frame_start = Some(now);
    let finished = profiler.frame;
    if CAPTURING.load(Ordering::Relaxed) {
        puffin::GlobalProfiler::lock().new_frame();
        let profiler = &mut *profiler;
        let mut threads: Vec<ThreadSample> = Vec::new();
        for frame in cpu::take_frames() {
            for thread in cpu::threads_of(&frame, &mut profiler.scopes) {
                match threads.iter_mut().find(|other| other.name == thread.name) {
                    Some(other) => {
                        for scope in thread.scopes {
                            ScopeSample::merge_into(&mut other.scopes, scope);
                        }
                    }
                    None => threads.push(thread),
                }
            }
        }
        if profiler.main_thread.is_empty() {
            profiler.main_thread = std::thread::current().name().unwrap_or("main").to_owned();
        }
        let main_thread = profiler.main_thread.clone();
        threads.sort_by_key(|thread| thread.name != main_thread);
        if profiler.history.len() == HISTORY_FRAMES {
            profiler.history.pop_front();
        }
        profiler.history.push_back(FrameSample {
            index: finished,
            frame_ns,
            main_thread,
            threads,
            gpu: None,
        });
    }
    gpu::end_frame(finished);
    for (frame, passes) in gpu::collect() {
        if let Some(sample) = profiler
            .history
            .iter_mut()
            .rev()
            .find(|sample| sample.index == frame)
        {
            sample.gpu = Some(passes);
        }
    }

    let requested = REQUESTED.load(Ordering::Relaxed) && !cfg!(calyx_no_profiling);
    if requested != CAPTURING.load(Ordering::Relaxed) {
        if requested && !profiler.sink_installed {
            cpu::install_sink();
            profiler.sink_installed = true;
        }
        puffin::set_scopes_on(requested);
        CAPTURING.store(requested, Ordering::Relaxed);
    }
    profiler.frame = finished + 1;
}

/// Index of the frame running now.
pub fn current_frame() -> u64 {
    PROFILER.lock().map_or(0, |profiler| profiler.frame)
}

/// Index of the first frame that capture fully covers once it is on: the
/// frame running now when it is already captured, else the next.
pub fn first_captured_frame() -> u64 {
    current_frame() + u64::from(!is_capturing())
}

/// Aggregates the last `frames` captured frames.
pub fn report(frames: usize) -> ProfileReport {
    let Ok(profiler) = PROFILER.lock() else {
        return ProfileReport::default();
    };
    let skip = profiler.history.len().saturating_sub(frames);
    let window: Vec<&FrameSample> = profiler.history.iter().skip(skip).collect();
    ProfileReport::from_frames(&window, gpu::status())
}

/// What [`report_range`] found for a window of frames.
pub enum RangeReport {
    /// Some of the frames have not been captured yet.
    Pending,
    /// The frames are captured, and `complete` tells whether every one of
    /// them has its GPU timings (always, without GPU timing).
    Ready {
        report: ProfileReport,
        complete: bool,
    },
    /// The history no longer holds every frame of the window.
    Missing,
}

/// Aggregates the `frames` frames starting at `first`.
pub fn report_range(first: u64, frames: usize) -> RangeReport {
    let Ok(profiler) = PROFILER.lock() else {
        return RangeReport::Missing;
    };
    let end = first + frames as u64;
    let window: Vec<&FrameSample> = profiler
        .history
        .iter()
        .filter(|sample| (first..end).contains(&sample.index))
        .collect();
    if window.len() == frames {
        let status = gpu::status();
        let complete = !status.available || window.iter().all(|sample| sample.gpu.is_some());
        return RangeReport::Ready {
            report: ProfileReport::from_frames(&window, status),
            complete,
        };
    }
    if profiler.frame < end {
        RangeReport::Pending
    } else {
        RangeReport::Missing
    }
}

/// Visits the frames in the history, oldest first.
pub fn with_history<R>(visit: impl FnOnce(&VecDeque<FrameSample>) -> R) -> Option<R> {
    PROFILER
        .lock()
        .ok()
        .map(|profiler| visit(&profiler.history))
}

#[cfg(test)]
mod tests;
