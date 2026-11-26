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
fn cancelling_steps_clears_them_and_leaves_other_operations() {
    let mut runtime = test_runtime();
    let slot = ResponseSlot {
        client: 1,
        request_id: 1,
    };
    runtime
        .pending
        .push(PendingOp::StepFrames { slot, remaining: 4 });
    runtime.pending.push(PendingOp::WaitFrames {
        slot,
        done_at_frame: 10,
    });
    runtime
        .pending
        .push(PendingOp::StepFrames { slot, remaining: 9 });

    runtime.cancel_pending_steps("interrupted");

    assert!(!runtime.has_pending_step());
    assert_eq!(runtime.pending.len(), 1, "the wait must survive");
}

#[test]
fn steps_are_cancelled_once_the_simulation_stops_running() {
    let mut runtime = test_runtime();
    let slot = ResponseSlot {
        client: 1,
        request_id: 1,
    };
    runtime
        .pending
        .push(PendingOp::StepFrames { slot, remaining: 4 });
    runtime.pending.push(PendingOp::WaitFrames {
        slot,
        done_at_frame: 10,
    });

    runtime.cancel_steps_unless_running(true);
    assert!(runtime.has_pending_step(), "a running step must survive");

    // A toolbar pause or a UI scene load leaves the step unable to count
    // down, so it must answer instead of waiting for the client timeout.
    runtime.cancel_steps_unless_running(false);
    assert!(!runtime.has_pending_step());
    assert_eq!(runtime.pending.len(), 1, "the wait must survive");
}

/// The only test of this binary that touches the profiler's capture.
#[test]
fn profile_reads_turn_capture_on_for_themselves_and_back_off() {
    profiler::set_capture(false);
    let mut runtime = test_runtime();
    let first = ResponseSlot {
        client: 1,
        request_id: 1,
    };
    let second = ResponseSlot {
        client: 2,
        request_id: 1,
    };
    runtime.push_profile_read(first, 10).unwrap();
    runtime.push_profile_read(second, 10).unwrap();
    assert!(profiler::capture_requested());
    runtime.drop_client(first.client);
    assert!(
        profiler::capture_requested(),
        "the second read still needs capture"
    );
    runtime.drop_client(second.client);
    assert!(!profiler::capture_requested());

    profiler::set_capture(true);
    runtime.push_profile_read(first, 10).unwrap();
    runtime.drop_client(first.client);
    assert!(
        profiler::capture_requested(),
        "a read that found capture on leaves it on"
    );

    profiler::set_capture(false);
    runtime.push_profile_read(first, 10).unwrap();
    profiler::set_capture(true);
    runtime.drop_client(first.client);
    assert!(
        profiler::capture_requested(),
        "a capture started during a read outlasts the read"
    );
    profiler::set_capture(false);
}

#[test]
fn a_profile_read_longer_than_the_history_is_refused() {
    let mut runtime = test_runtime();
    let slot = ResponseSlot {
        client: 1,
        request_id: 1,
    };
    let frames = profiler::HISTORY_FRAMES as u32 + 1;
    let (kind, message) = runtime.push_profile_read(slot, frames).unwrap_err();
    assert_eq!(kind, ErrorKind::BadRequest);
    assert!(message.contains("600"), "{message}");
    assert!(runtime.pending.is_empty());
}

#[test]
fn dropping_a_client_reports_an_abandoned_step() {
    let mut runtime = test_runtime();
    let stepping = ResponseSlot {
        client: 1,
        request_id: 1,
    };
    let waiting = ResponseSlot {
        client: 2,
        request_id: 1,
    };
    runtime.pending.push(PendingOp::StepFrames {
        slot: stepping,
        remaining: 4,
    });
    runtime.pending.push(PendingOp::WaitFrames {
        slot: waiting,
        done_at_frame: 10,
    });

    assert!(
        !runtime.drop_client(waiting.client),
        "dropping a waiter must not ask the caller to pause"
    );
    assert!(
        runtime.drop_client(stepping.client),
        "dropping a stepper must ask the caller to pause"
    );
    assert!(runtime.pending.is_empty());
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
