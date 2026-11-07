//! Per-frame profile samples and their aggregation over a window of frames.

use serde::Serialize;

/// One named scope of one frame: the time spent in it, how many times it
/// ran, and the scopes nested in it. Sibling scopes of the same name are
/// merged, so `calls` counts how often the scope ran under its parent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeSample {
    pub name: String,
    pub ns: u64,
    pub calls: u32,
    pub children: Vec<ScopeSample>,
}

impl ScopeSample {
    pub fn new(name: impl Into<String>, ns: u64, children: Vec<ScopeSample>) -> Self {
        Self {
            name: name.into(),
            ns,
            calls: 1,
            children,
        }
    }

    /// Adds `sample` to `siblings`, merging it into the sibling of the same
    /// name: their times and calls add up and their children merge in turn.
    pub fn merge_into(siblings: &mut Vec<ScopeSample>, sample: ScopeSample) {
        match siblings.iter_mut().find(|other| other.name == sample.name) {
            Some(existing) => {
                existing.ns += sample.ns;
                existing.calls += sample.calls;
                for child in sample.children {
                    Self::merge_into(&mut existing.children, child);
                }
            }
            None => siblings.push(sample),
        }
    }
}

/// The CPU scopes one thread recorded in a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadSample {
    pub name: String,
    pub scopes: Vec<ScopeSample>,
}

impl ThreadSample {
    /// Time spent in the thread's top-level scopes.
    pub fn busy_ns(&self) -> u64 {
        self.scopes.iter().map(|scope| scope.ns).sum()
    }
}

/// Everything the profiler recorded for one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameSample {
    /// The profiler's frame index, counted from the first frame.
    pub index: u64,
    /// Wall-clock time from this frame's start to the next frame's.
    pub frame_ns: u64,
    /// Name of the thread that runs the frame loop.
    pub main_thread: String,
    /// CPU scopes per thread, the main thread first.
    pub threads: Vec<ThreadSample>,
    /// GPU timings of the frame's passes, once the GPU has reported them;
    /// `None` until then, and for good without GPU timing.
    pub gpu: Option<Vec<ScopeSample>>,
}

impl FrameSample {
    /// Time the main thread spent in its top-level scopes.
    pub fn cpu_ns(&self) -> u64 {
        self.threads
            .iter()
            .find(|thread| thread.name == self.main_thread)
            .map_or(0, ThreadSample::busy_ns)
    }

    /// Time the GPU spent in the frame's top-level passes.
    pub fn gpu_ns(&self) -> Option<u64> {
        self.gpu
            .as_ref()
            .map(|scopes| scopes.iter().map(|scope| scope.ns).sum())
    }
}

/// Average, maximum and most recent value of a time over a window, in
/// milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct TimeStats {
    pub avg_ms: f64,
    pub max_ms: f64,
    pub last_ms: f64,
}

impl TimeStats {
    /// Statistics of `values`, in nanoseconds, oldest first. Empty gives
    /// zeros.
    pub fn from_ns(values: impl IntoIterator<Item = u64>) -> Self {
        let mut count = 0u64;
        let mut sum = 0u64;
        let mut max = 0u64;
        let mut last = 0u64;
        for value in values {
            count += 1;
            sum += value;
            max = max.max(value);
            last = value;
        }
        if count == 0 {
            return Self::default();
        }
        Self {
            avg_ms: ns_to_ms(sum) / count as f64,
            max_ms: ns_to_ms(max),
            last_ms: ns_to_ms(last),
        }
    }
}

fn ns_to_ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

/// A scope's time over a window of frames. A frame in which the scope did
/// not run counts as zero, so the averages of the children of a scope add
/// up to at most its own.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScopeStats {
    pub name: String,
    #[serde(flatten)]
    pub time: TimeStats,
    /// Average number of times the scope ran per frame.
    pub calls: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ScopeStats>,
}

/// One thread's scopes over a window of frames.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ThreadStats {
    pub name: String,
    pub scopes: Vec<ScopeStats>,
}

/// Whether GPU timings are recorded, and why not when they are not.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GpuTimingStatus {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A profile over a window of frames, as the profiler panel shows it and
/// `calyx_ctl profiler` prints it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ProfileReport {
    /// Index of the window's first frame.
    pub first_frame: u64,
    /// Number of frames in the window.
    pub frames: usize,
    /// Number of those frames whose GPU timings arrived.
    pub gpu_frames: usize,
    /// Wall-clock frame time.
    pub frame: TimeStats,
    /// Time the main thread spent in its top-level scopes.
    pub cpu: TimeStats,
    /// Time the GPU spent in the frame's passes, over the frames whose
    /// timings arrived; absent without any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<TimeStats>,
    pub gpu_timing: GpuTimingStatus,
    /// CPU scopes per thread, the main thread first.
    pub threads: Vec<ThreadStats>,
    /// GPU passes, over the frames whose timings arrived.
    pub gpu_passes: Vec<ScopeStats>,
}

impl ProfileReport {
    /// Aggregates `frames`, oldest first.
    pub fn from_frames(frames: &[&FrameSample], gpu_timing: GpuTimingStatus) -> Self {
        let gpu_frames: Vec<&[ScopeSample]> = frames
            .iter()
            .filter_map(|frame| frame.gpu.as_deref())
            .collect();
        let mut thread_names: Vec<&str> = Vec::new();
        if let Some(frame) = frames.last() {
            thread_names.push(&frame.main_thread);
        }
        for frame in frames {
            for thread in &frame.threads {
                if !thread_names.contains(&thread.name.as_str()) {
                    thread_names.push(&thread.name);
                }
            }
        }
        let threads = thread_names
            .into_iter()
            .map(|name| {
                let per_frame: Vec<&[ScopeSample]> = frames
                    .iter()
                    .map(|frame| {
                        frame
                            .threads
                            .iter()
                            .find(|thread| thread.name == name)
                            .map_or(&[][..], |thread| &thread.scopes[..])
                    })
                    .collect();
                ThreadStats {
                    name: name.to_owned(),
                    scopes: aggregate(&per_frame),
                }
            })
            .filter(|thread| !thread.scopes.is_empty())
            .collect();
        Self {
            first_frame: frames.first().map_or(0, |frame| frame.index),
            frames: frames.len(),
            gpu_frames: gpu_frames.len(),
            frame: TimeStats::from_ns(frames.iter().map(|frame| frame.frame_ns)),
            cpu: TimeStats::from_ns(frames.iter().map(|frame| frame.cpu_ns())),
            gpu: (!gpu_frames.is_empty())
                .then(|| TimeStats::from_ns(frames.iter().filter_map(|frame| frame.gpu_ns()))),
            gpu_timing,
            threads,
            gpu_passes: aggregate(&gpu_frames),
        }
    }
}

/// Aggregates the sibling scopes of several frames, oldest first, into one
/// list of scopes in order of first appearance.
pub fn aggregate(frames: &[&[ScopeSample]]) -> Vec<ScopeStats> {
    let mut names: Vec<&str> = Vec::new();
    for scopes in frames {
        for scope in scopes.iter() {
            if !names.contains(&scope.name.as_str()) {
                names.push(&scope.name);
            }
        }
    }
    let count = frames.len().max(1) as f64;
    names
        .into_iter()
        .map(|name| {
            let per_frame: Vec<Option<&ScopeSample>> = frames
                .iter()
                .map(|scopes| scopes.iter().find(|scope| scope.name == name))
                .collect();
            let children: Vec<&[ScopeSample]> = per_frame
                .iter()
                .map(|scope| scope.map_or(&[][..], |scope| &scope.children[..]))
                .collect();
            ScopeStats {
                name: name.to_owned(),
                time: TimeStats::from_ns(per_frame.iter().map(|scope| scope.map_or(0, |s| s.ns))),
                calls: per_frame
                    .iter()
                    .map(|scope| scope.map_or(0, |s| s.calls) as f64)
                    .sum::<f64>()
                    / count,
                children: aggregate(&children),
            }
        })
        .collect()
}
