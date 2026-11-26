use super::*;

#[test]
fn clean_document_does_not_autosave() {
    let state = SceneSaveState::default();

    assert!(!state.should_autosave(Instant::now() + AUTOSAVE_DELAY));
}

#[test]
fn dirty_document_autosaves_after_debounce() {
    let mut state = SceneSaveState::default();
    let now = Instant::now();

    state.mark_dirty(now);

    assert!(!state.should_autosave(now + AUTOSAVE_DELAY - Duration::from_millis(1)));
    assert!(state.should_autosave(now + AUTOSAVE_DELAY));
}

#[test]
fn successful_autosave_suppresses_same_revision() {
    let mut state = SceneSaveState::default();
    let now = Instant::now();

    state.mark_dirty(now);
    let revision = state.current_revision();
    state.mark_autosave_started(now + AUTOSAVE_DELAY);
    state.mark_autosave_finished(revision, true);

    assert!(
        !state.should_autosave(now + AUTOSAVE_DELAY + AUTOSAVE_INTERVAL + Duration::from_secs(1))
    );
}

#[test]
fn dirty_sync_marks_matching_saved_fingerprint_clean() {
    let mut state = SceneSaveState::default();
    let now = Instant::now();

    state.mark_clean(Some("saved".into()));
    state.mark_dirty(now);
    state.sync_dirty_to_fingerprint(Some("saved".into()), now + Duration::from_secs(1));

    assert!(!state.is_dirty());
    assert!(!state.should_autosave(now + AUTOSAVE_DELAY + AUTOSAVE_INTERVAL));
}

#[test]
fn dirty_sync_keeps_different_fingerprint_dirty() {
    let mut state = SceneSaveState::default();
    let now = Instant::now();

    state.mark_clean(Some("saved".into()));
    state.sync_dirty_to_fingerprint(Some("changed".into()), now);

    assert!(state.is_dirty());
    assert!(state.should_autosave(now + AUTOSAVE_DELAY));
}

#[test]
fn recovery_baseline_keeps_recovered_scene_dirty() {
    let mut state = SceneSaveState::default();
    let now = Instant::now();

    state.mark_clean(Some("source".into()));
    state.sync_dirty_to_fingerprint(Some("recovery".into()), now);

    assert!(state.is_dirty());
    assert!(state.should_autosave(now + AUTOSAVE_DELAY));
}

#[test]
fn newer_edit_remains_pending_after_older_autosave_finishes() {
    let mut state = SceneSaveState::default();
    let now = Instant::now();

    state.mark_dirty(now);
    let autosaved_revision = state.current_revision();
    state.mark_autosave_started(now + AUTOSAVE_DELAY);
    state.mark_dirty(now + AUTOSAVE_DELAY + Duration::from_millis(1));
    assert!(state.should_commit_autosave(autosaved_revision));
    state.mark_autosave_finished(autosaved_revision, true);

    assert!(
        state.should_autosave(now + AUTOSAVE_DELAY + AUTOSAVE_INTERVAL + Duration::from_secs(1))
    );
}

#[test]
fn clean_generation_rejects_stale_autosave_result() {
    let mut state = SceneSaveState::default();
    let now = Instant::now();

    state.mark_dirty(now);
    let stale_revision = state.current_revision();
    state.mark_clean(Some("saved".into()));

    assert!(!state.should_commit_autosave(stale_revision));
}

#[test]
fn recovery_requires_autosave_newer_than_source() {
    let source = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
    let older_autosave = SystemTime::UNIX_EPOCH + Duration::from_secs(9);
    let newer_autosave = SystemTime::UNIX_EPOCH + Duration::from_secs(11);

    assert!(!is_newer_recovery(older_autosave, Some(source)));
    assert!(is_newer_recovery(newer_autosave, Some(source)));
    assert!(is_newer_recovery(newer_autosave, None));
}

#[test]
fn autosave_path_is_stable_for_source_scene() {
    let autosave = SceneAutosave::new(Path::new("sandbox"));
    let source = Path::new("assets/scene.cxscene");

    assert_eq!(
        autosave.path_for_scene(source),
        autosave.path_for_scene(source)
    );
    assert_eq!(
        autosave
            .path_for_scene(source)
            .extension()
            .and_then(|ext| ext.to_str()),
        Some("autosave")
    );
}

#[test]
fn prepared_json_file_replaces_existing_file() {
    let root = std::env::temp_dir().join(format!("calyx-scene-file-test-{}", Uuid::new_v4()));
    let target = root.join("scene.cxscene");
    fs::create_dir_all(&root).expect("test directory should be created");
    fs::write(&target, b"old contents").expect("existing file should be written");

    let prepared = prepare_json_file(
        &target,
        &serde_json::json!({ "updated": true }),
        "test scene",
    )
    .expect("replacement should be prepared");
    prepared.commit().expect("replacement should be committed");

    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&target).expect("replacement should be readable"))
            .expect("replacement should contain JSON");
    assert_eq!(value, serde_json::json!({ "updated": true }));

    fs::remove_dir_all(root).expect("test directory should be removed");
}

#[test]
fn dropping_prepared_json_file_removes_staged_file() {
    let root = std::env::temp_dir().join(format!("calyx-scene-file-test-{}", Uuid::new_v4()));
    let target = root.join("scene.cxscene");

    let prepared = prepare_json_file(&target, &serde_json::json!({ "stale": true }), "test scene")
        .expect("staged file should be written");
    let staged_file = prepared.temp_file.clone();
    assert!(staged_file.exists());

    drop(prepared);

    assert!(!staged_file.exists());
    fs::remove_dir_all(root).expect("test directory should be removed");
}
