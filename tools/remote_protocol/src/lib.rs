//! Wire protocol shared between the editor's remote control server and the
//! `calyx_ctl` CLI.
//!
//! The transport is newline-delimited JSON over TCP on localhost: each request
//! and each response is a single JSON object on its own line. Requests carry a
//! client-chosen `id` that the matching response echoes back, so responses may
//! arrive out of order (commands such as [`Command::StepFrames`] complete
//! several frames after they were issued).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A single request line sent from a client to the editor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestEnvelope {
    /// Client-chosen correlation id echoed back in the response.
    pub id: u64,
    /// The command to execute.
    #[serde(flatten)]
    pub command: Command,
}

/// A single response line sent from the editor back to a client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseEnvelope {
    /// The request id this response answers, or `0` when the request line
    /// could not be parsed far enough to recover an id.
    pub id: u64,
    /// Success or failure payload.
    #[serde(flatten)]
    pub payload: ResponsePayload,
}

/// Success or failure payload of a response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponsePayload {
    /// The command succeeded; `data` holds command-specific results.
    Ok {
        /// Command-specific result value (may be `null`).
        data: serde_json::Value,
    },
    /// The command failed.
    Error {
        /// Machine-readable failure category.
        kind: ErrorKind,
        /// Human-readable failure description.
        message: String,
    },
}

impl ResponseEnvelope {
    /// Builds a success response for `id`.
    pub fn ok(id: u64, data: serde_json::Value) -> Self {
        Self {
            id,
            payload: ResponsePayload::Ok { data },
        }
    }

    /// Builds an error response for `id`.
    pub fn error(id: u64, kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            id,
            payload: ResponsePayload::Error {
                kind,
                message: message.into(),
            },
        }
    }
}

/// Machine-readable failure categories.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The request line was not valid JSON or not a valid request envelope.
    Parse,
    /// The command was understood but its arguments are invalid in the current
    /// editor state.
    BadRequest,
    /// A referenced object, component, asset, or panel does not exist.
    NotFound,
    /// A name selector matched more than one candidate.
    Ambiguous,
    /// The command is not supported in the current configuration.
    Unsupported,
    /// A deferred operation did not complete within its frame deadline.
    Timeout,
    /// An unexpected internal failure.
    Internal,
}

/// Commands accepted by the editor's remote control server.
///
/// Object selectors (`object` fields) accept either a game-object UUID or a
/// game-object name; component selectors (`component` fields) accept either a
/// component type UUID or a type name (matched against the fully qualified and
/// unqualified Rust type name).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Liveness probe; responds immediately with `null`.
    Ping,
    /// Returns editor/session metadata: version, project path, simulation
    /// state, frame index, scene file, and object count.
    Info,
    /// Closes the editor window after acknowledging the request.
    Shutdown,

    /// Starts (or resumes) scene simulation.
    Play,
    /// Pauses simulation, keeping the simulation scene copy.
    Pause,
    /// Stops simulation and discards the simulation scene copy.
    Stop,
    /// Ensures simulation is running, advances exactly `frames` editor frames,
    /// then pauses. Responds after the last frame.
    StepFrames {
        /// Number of frames to advance.
        frames: u32,
    },
    /// Loads a `.cxscene` file into the authoring slot (stops any simulation).
    LoadScene {
        /// Path to the scene file, absolute or relative to the editor's
        /// working directory.
        path: PathBuf,
    },
    /// Saves the authoring scene. Uses the scene's current file when `path` is
    /// omitted.
    SaveScene {
        /// Optional explicit destination path.
        path: Option<PathBuf>,
    },

    /// Returns the full active scene serialized as scene-file JSON.
    GetSceneState,
    /// Lists all game objects: id, name, parent, and component type names.
    ListObjects,
    /// Returns one object's id, name, parent, transforms, and components.
    GetObject {
        /// Game-object selector (UUID or name).
        object: String,
    },
    /// Returns one component's serialized JSON state.
    GetComponent {
        /// Game-object selector (UUID or name).
        object: String,
        /// Component type selector (UUID or type name).
        component: String,
    },
    /// Lists all registered component types (UUID and type name).
    ListComponentTypes,
    /// Returns the current editor selection.
    GetSelection,
    /// Returns window geometry, `pixels_per_point`, and the last known panel
    /// rects (egui points, window space).
    QueryPanels,

    /// Replaces a component's state from serialized JSON.
    SetComponent {
        /// Game-object selector (UUID or name).
        object: String,
        /// Component type selector (UUID or type name).
        component: String,
        /// Full serialized component value, as produced by `get_component`.
        value: serde_json::Value,
    },
    /// Overwrites parts of an object's transform.
    SetTransform {
        /// Game-object selector (UUID or name).
        object: String,
        /// New position, when given.
        position: Option<[f32; 3]>,
        /// New rotation as XYZ Euler angles in degrees, when given.
        rotation_euler_deg: Option<[f32; 3]>,
        /// New scale, when given.
        scale: Option<[f32; 3]>,
        /// Which space the values are expressed in.
        #[serde(default)]
        space: TransformSpace,
    },
    /// Creates an empty game object and returns its id.
    CreateObject {
        /// Name for the new object.
        name: String,
        /// Optional parent selector (UUID or name).
        parent: Option<String>,
    },
    /// Deletes a game object (and its descendants).
    DeleteObject {
        /// Game-object selector (UUID or name).
        object: String,
    },
    /// Adds a default-constructed component to an object.
    AddComponent {
        /// Game-object selector (UUID or name).
        object: String,
        /// Component type selector (UUID or type name).
        component: String,
    },
    /// Replaces the editor selection with the given objects.
    Select {
        /// Game-object selectors (UUID or name).
        objects: Vec<String>,
    },

    /// Returns the game object rendered at a pixel of the viewport or game
    /// panel texture.
    Pick {
        /// Horizontal coordinate in `space`.
        x: f32,
        /// Vertical coordinate in `space`.
        y: f32,
        /// Coordinate space of `x`/`y`.
        #[serde(default)]
        space: CoordSpace,
        /// Which render target to pick from.
        #[serde(default)]
        target: PickTarget,
    },
    /// Activates the Game tab and sets the cursor-grab flag so gameplay input
    /// reaches the simulation.
    FocusGame {
        /// Whether the game panel should hold input focus.
        grab: bool,
    },

    /// Schedules synthetic input events over the coming frames. Responds after
    /// the last scheduled event has been delivered.
    InjectInput {
        /// Event script, executed against a frame cursor.
        events: Vec<InputEventSpec>,
    },

    /// Captures a screenshot to a PNG file. Responds once the file is written.
    Screenshot {
        /// What to capture.
        #[serde(default)]
        target: CaptureTarget,
        /// Destination PNG path, absolute or relative to the editor's working
        /// directory.
        path: PathBuf,
    },
    /// Responds after `frames` further editor frames have run.
    WaitFrames {
        /// Number of frames to wait.
        frames: u32,
    },
}

/// Coordinate spaces for pointer coordinates.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoordSpace {
    /// Egui points in window space (what `query_panels` reports).
    #[default]
    Window,
    /// Normalized 0..1 within the Game panel's image rect.
    Game,
    /// Normalized 0..1 within the Viewport panel's image rect.
    Viewport,
}

/// Render targets for `pick`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PickTarget {
    /// The editor viewport renderer.
    #[default]
    Viewport,
    /// The game camera renderer.
    Game,
}

/// Capture targets for `screenshot`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTarget {
    /// The whole editor window including UI.
    #[default]
    Window,
    /// The game camera render texture.
    Game,
    /// The editor viewport render texture.
    Viewport,
}

/// Spaces for `set_transform` values.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransformSpace {
    /// Relative to the object's parent.
    #[default]
    Local,
    /// Absolute world space.
    World,
}

/// Pointer buttons for synthetic pointer events.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PointerButtonSpec {
    /// Left mouse button.
    #[default]
    Primary,
    /// Right mouse button.
    Secondary,
    /// Middle mouse button.
    Middle,
}

/// Modifier keys held during a synthetic key event.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModifiersSpec {
    /// Alt key.
    #[serde(default)]
    pub alt: bool,
    /// Ctrl key.
    #[serde(default)]
    pub ctrl: bool,
    /// Shift key.
    #[serde(default)]
    pub shift: bool,
}

/// One step of an `inject_input` script.
///
/// Events execute against a frame cursor that starts at the next editor frame:
/// each event fires at the cursor's frame, and only [`InputEventSpec::Wait`]
/// and the hold/release halves of [`InputEventSpec::KeyPress`] and
/// [`InputEventSpec::Click`] advance or extend the schedule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEventSpec {
    /// Presses a key (and keeps it held until a matching `key_up`).
    KeyDown {
        /// Key name in egui notation, e.g. `"W"`, `"Space"`, `"Escape"`.
        key: String,
        /// Modifiers held during the event.
        modifiers: Option<ModifiersSpec>,
    },
    /// Releases a key.
    KeyUp {
        /// Key name in egui notation.
        key: String,
        /// Modifiers held during the event.
        modifiers: Option<ModifiersSpec>,
    },
    /// Presses a key now and releases it `hold_frames` frames later
    /// (default 1).
    KeyPress {
        /// Key name in egui notation.
        key: String,
        /// How many frames to hold the key.
        hold_frames: Option<u32>,
    },
    /// Emits text input (for text fields).
    Text {
        /// The text to type.
        text: String,
    },
    /// Moves the pointer.
    PointerMove {
        /// Horizontal coordinate in `space`.
        x: f32,
        /// Vertical coordinate in `space`.
        y: f32,
        /// Coordinate space of `x`/`y`.
        #[serde(default)]
        space: CoordSpace,
    },
    /// Moves the pointer and presses a button.
    PointerDown {
        /// Horizontal coordinate in `space`.
        x: f32,
        /// Vertical coordinate in `space`.
        y: f32,
        /// Button to press.
        #[serde(default)]
        button: PointerButtonSpec,
        /// Coordinate space of `x`/`y`.
        #[serde(default)]
        space: CoordSpace,
    },
    /// Moves the pointer and releases a button.
    PointerUp {
        /// Horizontal coordinate in `space`.
        x: f32,
        /// Vertical coordinate in `space`.
        y: f32,
        /// Button to release.
        #[serde(default)]
        button: PointerButtonSpec,
        /// Coordinate space of `x`/`y`.
        #[serde(default)]
        space: CoordSpace,
    },
    /// Moves the pointer, presses a button, and releases it the next frame.
    Click {
        /// Horizontal coordinate in `space`.
        x: f32,
        /// Vertical coordinate in `space`.
        y: f32,
        /// Button to click.
        #[serde(default)]
        button: PointerButtonSpec,
        /// Coordinate space of `x`/`y`.
        #[serde(default)]
        space: CoordSpace,
    },
    /// Scrolls by the given delta in egui points.
    Scroll {
        /// Horizontal scroll delta.
        dx: f32,
        /// Vertical scroll delta.
        dy: f32,
    },
    /// Advances the frame cursor without emitting an event.
    Wait {
        /// Number of frames to skip.
        frames: u32,
    },
}

/// Contents of the discovery file the editor writes next to the project when
/// remote control is enabled (`<project>/.calyx/remote.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryInfo {
    /// TCP port the server is listening on (localhost only).
    pub port: u16,
    /// Process id of the editor.
    pub pid: u32,
}

#[cfg(test)]
mod tests {
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
            Command::Play,
            Command::Pause,
            Command::Stop,
            Command::StepFrames { frames: 10 },
            Command::LoadScene {
                path: PathBuf::from("assets/scene.cxscene"),
            },
            Command::SaveScene { path: None },
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
            Command::GetSelection,
            Command::QueryPanels,
            Command::SetComponent {
                object: "Tank".into(),
                component: "ComponentTransform".into(),
                value: serde_json::json!({ "position": [0.0, 1.0, 2.0] }),
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
            },
            Command::WaitFrames { frames: 3 },
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
            let parsed: ResponseEnvelope =
                serde_json::from_str(&json).expect("deserialize response");
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
}
