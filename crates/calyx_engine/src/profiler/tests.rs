use super::*;

fn scope(name: &str, ms: u64, children: Vec<ScopeSample>) -> ScopeSample {
    ScopeSample::new(name, ms * 1_000_000, children)
}

fn frame(
    index: u64,
    frame_ms: u64,
    scopes: Vec<ScopeSample>,
    gpu: Option<Vec<ScopeSample>>,
) -> FrameSample {
    FrameSample {
        index,
        frame_ns: frame_ms * 1_000_000,
        main_thread: "main".to_owned(),
        threads: vec![ThreadSample {
            name: "main".to_owned(),
            scopes,
        }],
        gpu,
    }
}

fn find<'a>(scopes: &'a [ScopeStats], name: &str) -> &'a ScopeStats {
    scopes
        .iter()
        .find(|scope| scope.name == name)
        .unwrap_or_else(|| panic!("no scope {name} in {scopes:?}"))
}

#[test]
fn merging_siblings_adds_their_times_calls_and_children() {
    let mut siblings = Vec::new();
    ScopeSample::merge_into(
        &mut siblings,
        scope("Hook", 2, vec![scope("Physics", 1, Vec::new())]),
    );
    ScopeSample::merge_into(&mut siblings, scope("Other", 5, Vec::new()));
    ScopeSample::merge_into(
        &mut siblings,
        scope(
            "Hook",
            3,
            vec![
                scope("Physics", 1, Vec::new()),
                scope("Mesh", 1, Vec::new()),
            ],
        ),
    );
    assert_eq!(siblings.len(), 2);
    let hook = &siblings[0];
    assert_eq!((hook.ns, hook.calls), (5_000_000, 2));
    let children: Vec<(&str, u64, u32)> = hook
        .children
        .iter()
        .map(|child| (child.name.as_str(), child.ns, child.calls))
        .collect();
    assert_eq!(
        children,
        [("Physics", 2_000_000, 2), ("Mesh", 1_000_000, 1)]
    );
}

#[test]
fn a_window_averages_and_maximises_each_nested_scope_counting_absent_frames_as_zero() {
    let frames = [
        frame(
            0,
            16,
            vec![scope(
                "Update",
                10,
                vec![
                    scope("Physics", 4, Vec::new()),
                    scope("Hooks", 6, Vec::new()),
                ],
            )],
            Some(vec![scope("Shadows", 2, Vec::new())]),
        ),
        frame(
            1,
            20,
            vec![scope("Update", 14, vec![scope("Hooks", 14, Vec::new())])],
            None,
        ),
    ];
    let window: Vec<&FrameSample> = frames.iter().collect();
    let report = ProfileReport::from_frames(&window, GpuTimingStatus::default());

    assert_eq!(
        (report.first_frame, report.frames, report.gpu_frames),
        (0, 2, 1)
    );
    assert_eq!(report.frame.avg_ms, 18.0);
    assert_eq!(report.frame.max_ms, 20.0);
    assert_eq!(report.frame.last_ms, 20.0);
    assert_eq!(report.cpu.avg_ms, 12.0);
    assert_eq!(report.cpu.max_ms, 14.0);
    let gpu = report.gpu.expect("one frame has GPU timings");
    assert_eq!((gpu.avg_ms, gpu.max_ms), (2.0, 2.0));

    assert_eq!(report.threads.len(), 1);
    let update = find(&report.threads[0].scopes, "Update");
    assert_eq!((update.time.avg_ms, update.time.max_ms), (12.0, 14.0));
    assert_eq!(update.calls, 1.0);
    let physics = find(&update.children, "Physics");
    assert_eq!(
        (
            physics.time.avg_ms,
            physics.time.max_ms,
            physics.time.last_ms
        ),
        (2.0, 4.0, 0.0),
        "the frame without physics counts as zero"
    );
    assert_eq!(physics.calls, 0.5);
    let hooks = find(&update.children, "Hooks");
    assert_eq!((hooks.time.avg_ms, hooks.time.max_ms), (10.0, 14.0));
    assert_eq!(find(&report.gpu_passes, "Shadows").time.avg_ms, 2.0);
}

#[test]
fn a_report_serializes_its_scope_tree_to_json() {
    let frames = [frame(
        7,
        16,
        vec![scope("Update", 3, vec![scope("Physics", 1, Vec::new())])],
        None,
    )];
    let window: Vec<&FrameSample> = frames.iter().collect();
    let status = GpuTimingStatus {
        available: false,
        reason: Some("no timestamps".to_owned()),
    };
    let json = serde_json::to_value(ProfileReport::from_frames(&window, status)).unwrap();
    assert_eq!(json["first_frame"], 7);
    assert_eq!(json["gpu_timing"]["available"], false);
    assert_eq!(json["gpu_timing"]["reason"], "no timestamps");
    assert!(json.get("gpu").is_none(), "no frame has GPU timings");
    let update = &json["threads"][0]["scopes"][0];
    assert_eq!(update["name"], "Update");
    assert_eq!(update["avg_ms"], 3.0);
    assert_eq!(update["children"][0]["name"], "Physics");
    assert_eq!(update["children"][0]["max_ms"], 1.0);
}

/// The only test that drives the global profiler, so no other test's frame
/// boundaries interleave with its own.
#[test]
fn captured_scopes_nest_per_thread_and_reach_the_history() {
    set_capture(true);
    new_frame();
    assert!(is_capturing());
    let first = first_captured_frame();
    {
        profile_scope!("profiler_test_outer");
        for _ in 0..2 {
            profile_scope!("profiler_test_inner");
            std::hint::black_box(());
        }
    }
    std::thread::Builder::new()
        .name("profiler_test_worker".to_owned())
        .spawn(|| {
            profile_scope!("profiler_test_work", "item");
        })
        .unwrap()
        .join()
        .unwrap();
    new_frame();
    set_capture(false);
    new_frame();
    assert!(!is_capturing());

    let RangeReport::Ready { report, complete } = report_range(first, 1) else {
        panic!("the captured frame is in the history");
    };
    assert!(complete, "no GPU timing is installed in unit tests");
    assert!(!report.gpu_timing.available);
    let this_thread = std::thread::current().name().unwrap().to_owned();
    let thread = report
        .threads
        .iter()
        .find(|thread| thread.name == this_thread)
        .expect("the test thread's scopes");
    let outer = find(&thread.scopes, "profiler_test_outer");
    assert_eq!(outer.calls, 1.0);
    let inner = find(&outer.children, "profiler_test_inner");
    assert_eq!(inner.calls, 2.0);
    assert!(inner.time.avg_ms <= outer.time.avg_ms);
    let worker = report
        .threads
        .iter()
        .find(|thread| thread.name == "profiler_test_worker")
        .expect("the worker thread's scopes");
    assert_eq!(worker.scopes[0].name, "profiler_test_work (item)");

    // Capture went off at the end of the next frame, so the one after it
    // is never recorded.
    assert!(matches!(report_range(first + 2, 1), RangeReport::Pending));
}
