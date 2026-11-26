use super::*;

fn round_trip(command: Command) {
    let request = RequestEnvelope { id: 7, command };
    let json = serde_json::to_string(&request).expect("serialize request");
    let parsed: RequestEnvelope = serde_json::from_str(&json).expect("deserialize request");
    assert_eq!(request, parsed);
}

#[test]
fn commands_round_trip() {
    for command in [
        Command::Ping,
        Command::Info,
        Command::Shutdown,
        Command::Play { scene: None },
        Command::Play {
            scene: Some(PathBuf::from("sandbox/assets/scene.cxscene")),
        },
        Command::Pause,
        Command::Stop,
        Command::StepFrames { frames: 10 },
        Command::LoadScene {
            path: PathBuf::from("assets/scene.cxscene"),
        },
        Command::SaveScene { path: None },
        Command::OpenPrefab {
            path: PathBuf::from("assets/prefabs/crate.cxprefab"),
        },
        Command::SavePrefab,
        Command::ClosePrefab { discard: true },
        Command::InstantiatePrefab {
            path: PathBuf::from("assets/prefabs/crate.cxprefab"),
            parent: None,
            position: None,
            rotation_euler_deg: None,
            scale: None,
        },
        Command::InstantiatePrefab {
            path: PathBuf::from("assets/prefabs/crate.cxprefab"),
            parent: Some("House".into()),
            position: Some([1.0, 2.0, 3.0]),
            rotation_euler_deg: Some([0.0, 90.0, 0.0]),
            scale: Some([2.0, 2.0, 2.0]),
        },
        Command::UpdatePrefabInstances { path: None },
        Command::OpenAsset {
            path: PathBuf::from("assets/scene.cxscene"),
            unsaved_scene: None,
        },
        Command::OpenAsset {
            path: PathBuf::from("assets/scene.cxscene"),
            unsaved_scene: Some(UnsavedSceneAnswer::Discard),
        },
        Command::UpdatePrefabInstances {
            path: Some(PathBuf::from("assets/prefabs/crate.cxprefab")),
        },
        Command::SaveAsset {
            path: PathBuf::from("assets/look.cxpostfx"),
        },
        Command::CreatePostProcessProfile {
            object: "Volume".into(),
            path: PathBuf::from("assets/look.cxpostfx"),
        },
        Command::GetPostProcessProfile {
            path: PathBuf::from("assets/look.cxpostfx"),
        },
        Command::SetPostProcessProfile {
            path: PathBuf::from("assets/look.cxpostfx"),
            profile: serde_json::json!({ "bloom": { "enabled": true } }),
        },
        Command::GetSceneState,
        Command::ListObjects,
        Command::GetObject {
            object: "Tank".into(),
        },
        Command::GetComponent {
            object: "Tank".into(),
            component: "ComponentTransform".into(),
        },
        Command::ListComponentTypes,
        Command::GetComponentEnabled {
            object: "Tank".into(),
            component: "ComponentCamera".into(),
        },
        Command::GetSelection,
        Command::QueryPanels,
        Command::SetComponent {
            object: "Tank".into(),
            component: "ComponentTransform".into(),
            value: serde_json::json!({ "position": [0.0, 1.0, 2.0] }),
        },
        Command::SetComponentEnabled {
            object: "Tank".into(),
            component: "ComponentCamera".into(),
            enabled: false,
        },
        Command::SetTransform {
            object: "Tank".into(),
            position: Some([1.0, 2.0, 3.0]),
            rotation_euler_deg: None,
            scale: None,
            space: TransformSpace::World,
        },
        Command::CreateObject {
            name: "Probe".into(),
            parent: None,
        },
        Command::DeleteObject {
            object: "Probe".into(),
        },
        Command::SetParent {
            object: "Roof".into(),
            parent: Some("House".into()),
        },
        Command::SetParent {
            object: "Roof".into(),
            parent: None,
        },
        Command::AddComponent {
            object: "Probe".into(),
            component: "ComponentCamera".into(),
        },
        Command::Select {
            objects: vec!["Tank".into()],
        },
        Command::Pick {
            x: 0.5,
            y: 0.5,
            space: CoordSpace::Game,
            target: PickTarget::Game,
        },
        Command::SetSceneView {
            view: SceneView::Normals,
        },
        Command::ViewportCamera {
            position: Some([1.0, 2.0, 3.0]),
            look_at: Some([0.0, 0.0, 0.0]),
        },
        Command::FocusGame { grab: true },
        Command::InjectInput {
            events: vec![
                InputEventSpec::KeyPress {
                    key: "W".into(),
                    hold_frames: Some(60),
                },
                InputEventSpec::Wait { frames: 5 },
                InputEventSpec::Click {
                    x: 0.5,
                    y: 0.5,
                    button: PointerButtonSpec::Primary,
                    space: CoordSpace::Game,
                },
            ],
        },
        Command::Screenshot {
            target: CaptureTarget::Game,
            path: PathBuf::from("shot.png"),
            size: None,
        },
        Command::Screenshot {
            target: CaptureTarget::Game,
            path: PathBuf::from("shot.png"),
            size: Some([1920, 1080]),
        },
        Command::WaitFrames { frames: 3 },
        Command::SetProfilerCapture { enabled: true },
        Command::ReadProfile { frames: 120 },
    ] {
        round_trip(command);
    }
}

#[test]
fn responses_round_trip() {
    for response in [
        ResponseEnvelope::ok(1, serde_json::json!({ "pong": true })),
        ResponseEnvelope::error(2, ErrorKind::NotFound, "no such object"),
    ] {
        let json = serde_json::to_string(&response).expect("serialize response");
        let parsed: ResponseEnvelope = serde_json::from_str(&json).expect("deserialize response");
        assert_eq!(response, parsed);
    }
}

#[test]
fn wire_format_is_pinned() {
    let request = RequestEnvelope {
        id: 1,
        command: Command::InjectInput {
            events: vec![InputEventSpec::KeyPress {
                key: "W".into(),
                hold_frames: Some(60),
            }],
        },
    };
    assert_eq!(
        serde_json::to_string(&request).unwrap(),
        r#"{"id":1,"cmd":"inject_input","events":[{"type":"key_press","key":"W","hold_frames":60}]}"#
    );

    let response = ResponseEnvelope::error(3, ErrorKind::BadRequest, "bad");
    assert_eq!(
        serde_json::to_string(&response).unwrap(),
        r#"{"id":3,"status":"error","kind":"bad_request","message":"bad"}"#
    );

    let parsed: RequestEnvelope =
        serde_json::from_str(r#"{"id":9,"cmd":"step_frames","frames":4}"#).unwrap();
    assert_eq!(parsed.command, Command::StepFrames { frames: 4 });
}

/// A screenshot without a size stays the same on the wire, so older
/// clients still capture at the Game tab's size.
#[test]
fn screenshot_without_a_size_keeps_its_wire_format() {
    let unsized_json = r#"{"id":1,"cmd":"screenshot","target":"game","path":"shot.png"}"#;
    let unsized_shot = Command::Screenshot {
        target: CaptureTarget::Game,
        path: PathBuf::from("shot.png"),
        size: None,
    };
    let request = RequestEnvelope {
        id: 1,
        command: unsized_shot.clone(),
    };
    assert_eq!(serde_json::to_string(&request).unwrap(), unsized_json);
    let parsed: RequestEnvelope = serde_json::from_str(unsized_json).unwrap();
    assert_eq!(parsed.command, unsized_shot);

    let parsed: RequestEnvelope = serde_json::from_str(
        r#"{"id":1,"cmd":"screenshot","target":"game","path":"shot.png","size":[1920,1080]}"#,
    )
    .unwrap();
    assert_eq!(
        parsed.command,
        Command::Screenshot {
            target: CaptureTarget::Game,
            path: PathBuf::from("shot.png"),
            size: Some([1920, 1080]),
        }
    );
}

/// A plain `play` stays the same on the wire, so older clients still run
/// the open scene.
#[test]
fn play_without_a_scene_keeps_its_wire_format() {
    let request = RequestEnvelope {
        id: 1,
        command: Command::Play { scene: None },
    };
    assert_eq!(
        serde_json::to_string(&request).unwrap(),
        r#"{"id":1,"cmd":"play"}"#
    );
    let parsed: RequestEnvelope = serde_json::from_str(r#"{"id":1,"cmd":"play"}"#).unwrap();
    assert_eq!(parsed.command, Command::Play { scene: None });
}

/// `instantiate_prefab` without a parent or transform keeps its wire
/// format, so older clients still instantiate at the scene root.
#[test]
fn instantiate_prefab_without_a_parent_keeps_its_wire_format() {
    let line = r#"{"id":1,"cmd":"instantiate_prefab","path":"crate.cxprefab"}"#;
    let parsed: RequestEnvelope = serde_json::from_str(line).unwrap();
    assert_eq!(
        parsed.command,
        Command::InstantiatePrefab {
            path: PathBuf::from("crate.cxprefab"),
            parent: None,
            position: None,
            rotation_euler_deg: None,
            scale: None,
        }
    );
    assert_eq!(serde_json::to_string(&parsed).unwrap(), line);
}

#[test]
fn instantiate_prefab_under_a_parent_names_its_fields() {
    let parsed: RequestEnvelope = serde_json::from_str(
        r#"{"id":2,"cmd":"instantiate_prefab","path":"crate.cxprefab","parent":"House","position":[1.0,0.0,0.0],"rotation_euler_deg":[0.0,90.0,0.0],"scale":[2.0,2.0,2.0]}"#,
    )
    .unwrap();
    assert_eq!(
        parsed.command,
        Command::InstantiatePrefab {
            path: PathBuf::from("crate.cxprefab"),
            parent: Some("House".into()),
            position: Some([1.0, 0.0, 0.0]),
            rotation_euler_deg: Some([0.0, 90.0, 0.0]),
            scale: Some([2.0, 2.0, 2.0]),
        }
    );
}

/// A `set_parent` without a parent moves the object to the scene root.
#[test]
fn set_parent_without_a_parent_means_the_root() {
    let line = r#"{"id":3,"cmd":"set_parent","object":"Roof"}"#;
    let parsed: RequestEnvelope = serde_json::from_str(line).unwrap();
    assert_eq!(
        parsed.command,
        Command::SetParent {
            object: "Roof".into(),
            parent: None,
        }
    );
    assert_eq!(serde_json::to_string(&parsed).unwrap(), line);
}

#[test]
fn discovery_info_round_trips() {
    let info = DiscoveryInfo {
        port: 4655,
        pid: 1234,
    };
    let json = serde_json::to_string(&info).unwrap();
    assert_eq!(json, r#"{"port":4655,"pid":1234}"#);
    assert_eq!(serde_json::from_str::<DiscoveryInfo>(&json).unwrap(), info);
}
