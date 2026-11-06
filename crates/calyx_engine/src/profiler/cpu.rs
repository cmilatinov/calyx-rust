//! Reads puffin's per-thread scope streams into [`ThreadSample`]s.

use std::sync::{Arc, Mutex};

use puffin::{FrameData, Reader, ScopeCollection, Stream};

use super::report::{ScopeSample, ThreadSample};

/// Frames puffin handed to the profiler's sink and nobody read yet.
pub(super) static PENDING_FRAMES: Mutex<Vec<Arc<FrameData>>> = Mutex::new(Vec::new());

/// Installs the sink that collects puffin's frames into
/// [`PENDING_FRAMES`]. Called once, before scopes are first turned on, so
/// the sink sees every scope's details.
pub(super) fn install_sink() {
    puffin::GlobalProfiler::lock().add_sink(Box::new(|frame| {
        if let Ok(mut pending) = PENDING_FRAMES.lock() {
            pending.push(frame);
        }
    }));
}

/// Takes the frames puffin completed since the last call.
pub(super) fn take_frames() -> Vec<Arc<FrameData>> {
    PENDING_FRAMES
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

/// Reads one puffin frame into one sample per thread, learning the details
/// of the scopes it introduces into `scopes`. Threads whose stream does not
/// parse are left out.
pub(super) fn threads_of(frame: &FrameData, scopes: &mut ScopeCollection) -> Vec<ThreadSample> {
    for details in &frame.scope_delta {
        scopes.insert(details.clone());
    }
    let Some(unpacked) = frame.unpacked().ok() else {
        return Vec::new();
    };
    unpacked
        .thread_streams
        .iter()
        .filter_map(|(info, stream)| {
            let roots = read_scopes(&stream.stream, Reader::from_start(&stream.stream), scopes)?;
            Some(ThreadSample {
                name: info.name.clone(),
                scopes: roots,
            })
        })
        .collect()
}

fn read_scopes(
    stream: &Stream,
    reader: Reader<'_>,
    scopes: &ScopeCollection,
) -> Option<Vec<ScopeSample>> {
    let mut samples = Vec::new();
    for scope in reader {
        let scope = scope.ok()?;
        let children = read_scopes(
            stream,
            Reader::with_offset(stream, scope.child_begin_position).ok()?,
            scopes,
        )?;
        let name = scopes
            .fetch_by_id(&scope.id)
            .map_or_else(|| "?".to_owned(), |details| details.name().to_string());
        let name = if scope.record.data.is_empty() {
            name
        } else {
            format!("{name} ({})", scope.record.data)
        };
        let ns = u64::try_from(scope.record.duration_ns).unwrap_or(0);
        ScopeSample::merge_into(&mut samples, ScopeSample::new(name, ns, children));
    }
    Some(samples)
}
