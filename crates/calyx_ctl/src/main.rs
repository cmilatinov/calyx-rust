//! Command line client for the Calyx editor's remote control server.
//!
//! Speaks the `calyx_remote_protocol` JSON-lines protocol over localhost TCP. The
//! server port is discovered from `<project>/.calyx/remote.json`, written by
//! an editor launched with `CALYX_REMOTE_PORT` set (see the `launch`
//! subcommand).
//!
//! Output contract (for scripting): the response payload is printed to stdout
//! as one JSON object. Exit codes: `0` command succeeded, `1` the editor
//! answered with an error, `2` transport/discovery/timeout failure.

mod client;
mod launch;

use std::path::PathBuf;
use std::time::Duration;

use calyx_remote_protocol::{
    CaptureTarget, Command, CoordSpace, InputEventSpec, PickTarget, PointerButtonSpec,
    ResponsePayload, SceneView, TransformSpace, UnsavedSceneAnswer,
};
use clap::{Parser, Subcommand, ValueEnum};

use client::{discover, Client, CtlError};

#[derive(Parser)]
#[command(name = "calyx_ctl", about = "Drive a running Calyx editor remotely")]
struct Cli {
    /// Project directory (used for discovery and by `launch`).
    #[arg(long, global = true, default_value = "./sandbox")]
    project: PathBuf,
    /// Connect to this port instead of reading the discovery file.
    #[arg(long, global = true)]
    port: Option<u16>,
    /// Base timeout in seconds for a single command round trip.
    #[arg(long, global = true, default_value_t = 30)]
    timeout: u64,
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Build and start the editor with remote control enabled, then wait for
    /// readiness and print the discovery info.
    Launch {
        /// Build and run the release profile.
        #[arg(long)]
        release: bool,
        /// Skip `cargo build` and go straight to `cargo run`.
        #[arg(long)]
        no_build: bool,
        /// Port for the editor to bind (0 = ephemeral).
        #[arg(long, default_value_t = 0)]
        listen_port: u16,
        /// Seconds to wait for the editor to become ready.
        #[arg(long, default_value_t = 120)]
        ready_timeout: u64,
    },
    /// Liveness probe.
    Ping,
    /// Editor/session metadata.
    Info,
    /// Close the editor.
    Shutdown,
    /// Start or resume simulation.
    Play {
        /// Run this scene file instead of the editor's open scene on a
        /// fresh start.
        #[arg(long)]
        scene: Option<PathBuf>,
    },
    /// Pause simulation (keeps the play copy).
    Pause,
    /// Stop simulation (discards the play copy).
    Stop,
    /// Advance exactly N simulated frames, then pause.
    Step {
        #[arg(long, default_value_t = 1)]
        frames: u32,
    },
    /// Wait for frames to elapse or for a simulation state.
    Wait {
        /// Wait this many editor frames.
        #[arg(long)]
        frames: Option<u32>,
        /// Wait until is_simulating matches this value.
        #[arg(long)]
        simulating: Option<bool>,
    },
    /// Scene file operations.
    #[command(subcommand)]
    Scene(SceneAction),
    /// Prefab mode: edit a prefab asset in isolation from the scene.
    #[command(subcommand)]
    Prefab(PrefabAction),
    /// Asset operations, as the content browser does them.
    #[command(subcommand)]
    Asset(AssetAction),
    /// Post-processing profile assets: create one from a volume, and read or
    /// edit a loaded one as its inspector does.
    #[command(subcommand)]
    Profile(ProfileAction),
    /// List all game objects.
    Objects,
    /// Game object operations.
    #[command(subcommand)]
    Object(ObjectAction),
    /// Component operations.
    #[command(subcommand)]
    Component(ComponentAction),
    /// Overwrite parts of an object's transform.
    Transform {
        /// Game-object selector (UUID or name).
        object: String,
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], allow_negative_numbers = true)]
        position: Option<Vec<f32>>,
        /// Rotation as XYZ Euler angles in degrees.
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], allow_negative_numbers = true)]
        rotation: Option<Vec<f32>>,
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], allow_negative_numbers = true)]
        scale: Option<Vec<f32>>,
        /// Interpret the values in world space instead of local space.
        #[arg(long)]
        world: bool,
    },
    /// Replace the editor selection.
    Select {
        /// Game-object selectors (UUID or name).
        objects: Vec<String>,
    },
    /// Query which object is rendered at a texture position.
    Pick {
        x: f32,
        y: f32,
        #[arg(long, value_enum, default_value_t = SpaceArg::Viewport)]
        space: SpaceArg,
        #[arg(long, value_enum, default_value_t = PickTargetArg::Viewport)]
        target: PickTargetArg,
    },
    /// Show the lit scene or one G-buffer target in the editor's scene view.
    View {
        #[arg(value_enum)]
        view: SceneViewArg,
    },
    /// Place the editor's viewport camera, or print where it is without
    /// options.
    ViewportCamera {
        /// World position to move the camera to.
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], allow_negative_numbers = true)]
        position: Option<Vec<f32>>,
        /// World point for the camera to face.
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], allow_negative_numbers = true)]
        look_at: Option<Vec<f32>>,
    },
    /// Activate the Game tab and grab (or release) gameplay input focus.
    FocusGame {
        /// Release the input grab instead of taking it.
        #[arg(long)]
        release_grab: bool,
    },
    /// Inject synthetic input.
    #[command(subcommand)]
    Input(InputAction),
    /// Capture a PNG screenshot.
    Screenshot {
        #[arg(long, value_enum, default_value_t = CaptureTargetArg::Window)]
        target: CaptureTargetArg,
        /// Destination PNG path (relative paths resolve against the editor's
        /// working directory, i.e. the repo root when using `launch`).
        #[arg(long)]
        out: PathBuf,
    },
    /// Read or control the frame profiler.
    #[command(subcommand)]
    Profiler(ProfilerAction),
    /// Send a raw protocol command as JSON (see calyx_remote_protocol).
    Raw {
        /// JSON object, e.g. '{"cmd":"step_frames","frames":10}'.
        json: String,
    },
}

#[derive(Subcommand)]
enum ProfilerAction {
    /// Profile the next N frames (turning capture on for them when it is
    /// off) and print their averages and maximums as JSON.
    Read {
        /// Number of frames to profile, at most the 600 the profiler keeps.
        #[arg(long, default_value_t = 120)]
        frames: u32,
    },
    /// Turn capture on, so the profiler panel keeps recording.
    Start,
    /// Turn capture off.
    Stop,
}

#[derive(Subcommand)]
enum SceneAction {
    /// Load a .cxscene file into the authoring slot.
    Load { path: PathBuf },
    /// Save the authoring scene (to its current file when no path is given).
    Save { path: Option<PathBuf> },
    /// Dump the full active scene as scene-file JSON.
    State,
}

#[derive(Subcommand)]
enum PrefabAction {
    /// Open a .cxprefab asset in prefab mode, setting the scene aside.
    Open { path: PathBuf },
    /// Save the open prefab to its asset file.
    Save,
    /// Close the open prefab and return to the scene.
    Close {
        /// Drop unsaved prefab changes instead of refusing to close.
        #[arg(long)]
        discard: bool,
    },
    /// Instantiate a prefab asset, linked to the prefab, as one undo step:
    /// under `--parent`, or at the scene root without it. The instance root
    /// keeps the prefab root's local transform except for the parts given.
    Instantiate {
        path: PathBuf,
        /// Game-object selector (UUID or name) of the parent.
        #[arg(long)]
        parent: Option<String>,
        /// Local position of the instance root.
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], allow_negative_numbers = true)]
        position: Option<Vec<f32>>,
        /// Local rotation of the instance root as XYZ Euler angles in degrees.
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], allow_negative_numbers = true)]
        rotation: Option<Vec<f32>>,
        /// Local scale of the instance root.
        #[arg(long, num_args = 3, value_names = ["X", "Y", "Z"], allow_negative_numbers = true)]
        scale: Option<Vec<f32>>,
    },
    /// Update the scene's instances of a prefab (of every prefab when no
    /// path is given) from the saved prefab, as one undo step.
    Update { path: Option<PathBuf> },
}

#[derive(Subcommand)]
enum AssetAction {
    /// Open an asset as a double-click in the content browser does.
    Open {
        path: PathBuf,
        /// Answer the unsaved-scene prompt; without it the prompt stays open.
        #[arg(long, value_enum)]
        unsaved: Option<UnsavedArg>,
    },
    /// Write a loaded asset back to its file, as Save in its inspector does.
    Save { path: PathBuf },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// Create a .cxpostfx profile from a volume's effects, as the volume
    /// inspector's "Create Profile" does, and point the volume at it.
    Create { object: String, path: PathBuf },
    /// Print a loaded profile.
    Get { path: PathBuf },
    /// Replace a loaded profile from JSON, as its inspector edits it, without
    /// saving the file.
    Set {
        path: PathBuf,
        /// The whole profile, as `profile get` prints it.
        json: String,
    },
}

#[derive(Subcommand)]
enum ObjectAction {
    /// Show an object's transforms and components.
    Get { object: String },
    /// Create an empty game object.
    Create {
        name: String,
        #[arg(long)]
        parent: Option<String>,
    },
    /// Delete a game object and its descendants.
    Delete { object: String },
    /// Move an object under another object, or to the scene root with
    /// `--root`, keeping its world transform, as one undo step.
    Parent {
        /// Game-object selector (UUID or name) of the object to move.
        object: String,
        /// Game-object selector (UUID or name) of the new parent.
        #[arg(required_unless_present = "root", conflicts_with = "root")]
        parent: Option<String>,
        /// Move the object to the scene root.
        #[arg(long)]
        root: bool,
    },
}

#[derive(Subcommand)]
enum ComponentAction {
    /// Print a component's serialized state.
    Get { object: String, component: String },
    /// Replace a component's state from JSON.
    Set {
        object: String,
        component: String,
        /// Full serialized component value, as printed by `component get`.
        json: String,
    },
    /// Add a default-constructed component to an object.
    Add { object: String, component: String },
    /// Print whether a component is enabled.
    Enabled { object: String, component: String },
    /// Enable a component.
    Enable { object: String, component: String },
    /// Disable a component; systems such as cameras and physics skip it.
    Disable { object: String, component: String },
    /// List all registered component types.
    Types,
}

#[derive(Subcommand)]
enum InputAction {
    /// Press a key for a number of frames.
    Key {
        /// Egui key name, e.g. W, Space, Escape, ArrowLeft.
        key: String,
        #[arg(long, default_value_t = 1)]
        hold_frames: u32,
    },
    /// Click at a position.
    Click {
        x: f32,
        y: f32,
        #[arg(long, value_enum, default_value_t = SpaceArg::Game)]
        space: SpaceArg,
        #[arg(long, value_enum, default_value_t = ButtonArg::Primary)]
        button: ButtonArg,
    },
    /// Move the pointer to a position.
    Move {
        x: f32,
        y: f32,
        #[arg(long, value_enum, default_value_t = SpaceArg::Game)]
        space: SpaceArg,
    },
    /// Type text.
    Text { text: String },
    /// Send a raw list of input event specs as JSON.
    Raw {
        /// JSON array of input events (see calyx_remote_protocol InputEventSpec).
        json: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum SpaceArg {
    Window,
    Game,
    Viewport,
}

impl From<SpaceArg> for CoordSpace {
    fn from(value: SpaceArg) -> Self {
        match value {
            SpaceArg::Window => CoordSpace::Window,
            SpaceArg::Game => CoordSpace::Game,
            SpaceArg::Viewport => CoordSpace::Viewport,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum PickTargetArg {
    Viewport,
    Game,
}

#[derive(Clone, Copy, ValueEnum)]
enum SceneViewArg {
    Lit,
    Albedo,
    Normals,
    Material,
    Emissive,
}

#[derive(Clone, Copy, ValueEnum)]
enum CaptureTargetArg {
    Window,
    Game,
    Viewport,
}

#[derive(Clone, Copy, ValueEnum)]
enum ButtonArg {
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Copy, ValueEnum)]
enum UnsavedArg {
    /// Save the open scene first (refused for a scene with no file).
    Save,
    /// Open without saving.
    Discard,
    /// Keep the open scene.
    Cancel,
}

impl From<UnsavedArg> for UnsavedSceneAnswer {
    fn from(value: UnsavedArg) -> Self {
        match value {
            UnsavedArg::Save => UnsavedSceneAnswer::Save,
            UnsavedArg::Discard => UnsavedSceneAnswer::Discard,
            UnsavedArg::Cancel => UnsavedSceneAnswer::Cancel,
        }
    }
}

impl From<ButtonArg> for PointerButtonSpec {
    fn from(value: ButtonArg) -> Self {
        match value {
            ButtonArg::Primary => PointerButtonSpec::Primary,
            ButtonArg::Secondary => PointerButtonSpec::Secondary,
            ButtonArg::Middle => PointerButtonSpec::Middle,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    std::process::exit(run(cli));
}

fn run(cli: Cli) -> i32 {
    let base_timeout = Duration::from_secs(cli.timeout);

    if let Action::Launch {
        release,
        no_build,
        listen_port,
        ready_timeout,
    } = &cli.action
    {
        return match launch::launch(
            &cli.project,
            *listen_port,
            *release,
            *no_build,
            Duration::from_secs(*ready_timeout),
        ) {
            Ok(info) => {
                println!("{}", serde_json::to_string(&info).unwrap());
                0
            }
            Err(error) => {
                eprintln!("{error}");
                2
            }
        };
    }

    let port = match cli.port {
        Some(port) => port,
        None => match discover(&cli.project) {
            Ok(info) => info.port,
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        },
    };

    if let Action::Wait {
        frames: None,
        simulating: Some(expected),
    } = &cli.action
    {
        return wait_for_simulation_state(port, *expected, base_timeout);
    }

    let (command, timeout) = match build_command(&cli.action, base_timeout) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };

    let payload =
        Client::connect(port, base_timeout).and_then(|mut client| client.call(command, timeout));
    match payload {
        Ok(payload) => {
            println!("{}", serde_json::to_string_pretty(&payload).unwrap());
            match payload {
                ResponsePayload::Ok { .. } => 0,
                ResponsePayload::Error { .. } => 1,
            }
        }
        Err(error) => {
            eprintln!("{error}");
            2
        }
    }
}

/// Polls `info` until `is_simulating` matches `expected` or the timeout
/// elapses.
fn wait_for_simulation_state(port: u16, expected: bool, timeout: Duration) -> i32 {
    let deadline = std::time::Instant::now() + timeout;
    let mut client = match Client::connect(port, timeout) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    loop {
        match client.call(Command::Info, Duration::from_secs(5)) {
            Ok(ResponsePayload::Ok { data }) => {
                if data.get("is_simulating").and_then(|v| v.as_bool()) == Some(expected) {
                    println!("{}", serde_json::to_string_pretty(&data).unwrap());
                    return 0;
                }
            }
            Ok(ResponsePayload::Error { .. }) => {}
            Err(CtlError::Timeout(_)) => {}
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        }
        if std::time::Instant::now() > deadline {
            eprintln!("timed out waiting for is_simulating == {expected}");
            return 2;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Extends the base timeout for commands that wait on editor frames
/// (conservatively assuming at least 20 fps).
fn frame_timeout(base: Duration, frames: u32) -> Duration {
    base + Duration::from_millis(frames as u64 * 50)
}

/// The three values of a `num_args = 3` option.
fn triple(values: &Option<Vec<f32>>) -> Option<[f32; 3]> {
    values.as_ref().map(|v| [v[0], v[1], v[2]])
}

fn build_command(action: &Action, base: Duration) -> Result<(Command, Duration), String> {
    let simple = |command: Command| Ok((command, base));
    match action {
        Action::Launch { .. } => unreachable!("launch is handled before dispatch"),
        Action::Ping => simple(Command::Ping),
        Action::Info => simple(Command::Info),
        Action::Shutdown => simple(Command::Shutdown),
        Action::Play { scene } => simple(Command::Play {
            scene: scene.clone(),
        }),
        Action::Pause => simple(Command::Pause),
        Action::Stop => simple(Command::Stop),
        Action::Step { frames } => Ok((
            Command::StepFrames { frames: *frames },
            frame_timeout(base, *frames),
        )),
        Action::Wait { frames, simulating } => match (frames, simulating) {
            (Some(frames), None) => Ok((
                Command::WaitFrames { frames: *frames },
                frame_timeout(base, *frames),
            )),
            _ => Err("pass exactly one of --frames or --simulating".into()),
        },
        Action::Profiler(profiler) => match profiler {
            // The GPU reports a frame's timings a few frames after it ran.
            ProfilerAction::Read { frames } => Ok((
                Command::ReadProfile { frames: *frames },
                frame_timeout(base, frames.saturating_add(120)),
            )),
            ProfilerAction::Start => simple(Command::SetProfilerCapture { enabled: true }),
            ProfilerAction::Stop => simple(Command::SetProfilerCapture { enabled: false }),
        },
        Action::Scene(scene) => match scene {
            SceneAction::Load { path } => simple(Command::LoadScene { path: path.clone() }),
            SceneAction::Save { path } => simple(Command::SaveScene { path: path.clone() }),
            SceneAction::State => simple(Command::GetSceneState),
        },
        Action::Prefab(prefab) => match prefab {
            PrefabAction::Open { path } => simple(Command::OpenPrefab { path: path.clone() }),
            PrefabAction::Save => simple(Command::SavePrefab),
            PrefabAction::Close { discard } => simple(Command::ClosePrefab { discard: *discard }),
            PrefabAction::Instantiate {
                path,
                parent,
                position,
                rotation,
                scale,
            } => simple(Command::InstantiatePrefab {
                path: path.clone(),
                parent: parent.clone(),
                position: triple(position),
                rotation_euler_deg: triple(rotation),
                scale: triple(scale),
            }),
            PrefabAction::Update { path } => {
                simple(Command::UpdatePrefabInstances { path: path.clone() })
            }
        },
        Action::Asset(asset) => match asset {
            AssetAction::Open { path, unsaved } => simple(Command::OpenAsset {
                path: path.clone(),
                unsaved_scene: unsaved.map(Into::into),
            }),
            AssetAction::Save { path } => simple(Command::SaveAsset { path: path.clone() }),
        },
        Action::Profile(profile) => match profile {
            ProfileAction::Create { object, path } => simple(Command::CreatePostProcessProfile {
                object: object.clone(),
                path: path.clone(),
            }),
            ProfileAction::Get { path } => {
                simple(Command::GetPostProcessProfile { path: path.clone() })
            }
            ProfileAction::Set { path, json } => {
                let profile: serde_json::Value = serde_json::from_str(json)
                    .map_err(|error| format!("profile is not valid JSON: {error}"))?;
                simple(Command::SetPostProcessProfile {
                    path: path.clone(),
                    profile,
                })
            }
        },
        Action::Objects => simple(Command::ListObjects),
        Action::Object(object) => match object {
            ObjectAction::Get { object } => simple(Command::GetObject {
                object: object.clone(),
            }),
            ObjectAction::Create { name, parent } => simple(Command::CreateObject {
                name: name.clone(),
                parent: parent.clone(),
            }),
            ObjectAction::Delete { object } => simple(Command::DeleteObject {
                object: object.clone(),
            }),
            ObjectAction::Parent { object, parent, .. } => simple(Command::SetParent {
                object: object.clone(),
                parent: parent.clone(),
            }),
        },
        Action::Component(component) => match component {
            ComponentAction::Get { object, component } => simple(Command::GetComponent {
                object: object.clone(),
                component: component.clone(),
            }),
            ComponentAction::Set {
                object,
                component,
                json,
            } => {
                let value = serde_json::from_str(json)
                    .map_err(|error| format!("invalid component JSON: {error}"))?;
                simple(Command::SetComponent {
                    object: object.clone(),
                    component: component.clone(),
                    value,
                })
            }
            ComponentAction::Add { object, component } => simple(Command::AddComponent {
                object: object.clone(),
                component: component.clone(),
            }),
            ComponentAction::Enabled { object, component } => {
                simple(Command::GetComponentEnabled {
                    object: object.clone(),
                    component: component.clone(),
                })
            }
            ComponentAction::Enable { object, component } => simple(Command::SetComponentEnabled {
                object: object.clone(),
                component: component.clone(),
                enabled: true,
            }),
            ComponentAction::Disable { object, component } => {
                simple(Command::SetComponentEnabled {
                    object: object.clone(),
                    component: component.clone(),
                    enabled: false,
                })
            }
            ComponentAction::Types => simple(Command::ListComponentTypes),
        },
        Action::Transform {
            object,
            position,
            rotation,
            scale,
            world,
        } => simple(Command::SetTransform {
            object: object.clone(),
            position: triple(position),
            rotation_euler_deg: triple(rotation),
            scale: triple(scale),
            space: if *world {
                TransformSpace::World
            } else {
                TransformSpace::Local
            },
        }),
        Action::Select { objects } => simple(Command::Select {
            objects: objects.clone(),
        }),
        Action::Pick {
            x,
            y,
            space,
            target,
        } => simple(Command::Pick {
            x: *x,
            y: *y,
            space: (*space).into(),
            target: match target {
                PickTargetArg::Viewport => PickTarget::Viewport,
                PickTargetArg::Game => PickTarget::Game,
            },
        }),
        Action::ViewportCamera { position, look_at } => simple(Command::ViewportCamera {
            position: triple(position),
            look_at: triple(look_at),
        }),
        Action::View { view } => simple(Command::SetSceneView {
            view: match view {
                SceneViewArg::Lit => SceneView::Lit,
                SceneViewArg::Albedo => SceneView::Albedo,
                SceneViewArg::Normals => SceneView::Normals,
                SceneViewArg::Material => SceneView::Material,
                SceneViewArg::Emissive => SceneView::Emissive,
            },
        }),
        Action::FocusGame { release_grab } => simple(Command::FocusGame {
            grab: !release_grab,
        }),
        Action::Input(input) => {
            let events = match input {
                InputAction::Key { key, hold_frames } => vec![InputEventSpec::KeyPress {
                    key: key.clone(),
                    hold_frames: Some(*hold_frames),
                }],
                InputAction::Click {
                    x,
                    y,
                    space,
                    button,
                } => vec![InputEventSpec::Click {
                    x: *x,
                    y: *y,
                    button: (*button).into(),
                    space: (*space).into(),
                }],
                InputAction::Move { x, y, space } => vec![InputEventSpec::PointerMove {
                    x: *x,
                    y: *y,
                    space: (*space).into(),
                }],
                InputAction::Text { text } => vec![InputEventSpec::Text { text: text.clone() }],
                InputAction::Raw { json } => serde_json::from_str(json)
                    .map_err(|error| format!("invalid input event JSON: {error}"))?,
            };
            let total_frames = events
                .iter()
                .map(|event| match event {
                    InputEventSpec::KeyPress { hold_frames, .. } => hold_frames.unwrap_or(1),
                    InputEventSpec::Wait { frames } => *frames,
                    _ => 1,
                })
                .sum();
            Ok((
                Command::InjectInput { events },
                frame_timeout(base, total_frames),
            ))
        }
        Action::Screenshot { target, out } => Ok((
            Command::Screenshot {
                target: match target {
                    CaptureTargetArg::Window => CaptureTarget::Window,
                    CaptureTargetArg::Game => CaptureTarget::Game,
                    CaptureTargetArg::Viewport => CaptureTarget::Viewport,
                },
                path: out.clone(),
            },
            frame_timeout(base, 150),
        )),
        Action::Raw { json } => {
            let command = serde_json::from_str(json)
                .map_err(|error| format!("invalid command JSON: {error}"))?;
            Ok((command, frame_timeout(base, 300)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(args: &[&str]) -> Command {
        let cli = Cli::try_parse_from(std::iter::once("calyx_ctl").chain(args.iter().copied()))
            .expect("arguments should parse");
        build_command(&cli.action, Duration::from_secs(1))
            .expect("command should build")
            .0
    }

    #[test]
    fn prefab_instantiate_passes_the_parent_and_local_transform() {
        assert_eq!(
            command(&[
                "prefab",
                "instantiate",
                "crate.cxprefab",
                "--parent",
                "House",
                "--position",
                "1",
                "-2",
                "3",
                "--rotation",
                "0",
                "90",
                "0",
                "--scale",
                "2",
                "2",
                "2",
            ]),
            Command::InstantiatePrefab {
                path: PathBuf::from("crate.cxprefab"),
                parent: Some("House".into()),
                position: Some([1.0, -2.0, 3.0]),
                rotation_euler_deg: Some([0.0, 90.0, 0.0]),
                scale: Some([2.0, 2.0, 2.0]),
            }
        );
        assert_eq!(
            command(&["prefab", "instantiate", "crate.cxprefab"]),
            Command::InstantiatePrefab {
                path: PathBuf::from("crate.cxprefab"),
                parent: None,
                position: None,
                rotation_euler_deg: None,
                scale: None,
            }
        );
    }

    #[test]
    fn object_parent_moves_under_a_parent_or_to_the_root() {
        assert_eq!(
            command(&["object", "parent", "Roof", "House"]),
            Command::SetParent {
                object: "Roof".into(),
                parent: Some("House".into()),
            }
        );
        assert_eq!(
            command(&["object", "parent", "Roof", "--root"]),
            Command::SetParent {
                object: "Roof".into(),
                parent: None,
            }
        );
    }

    /// Leaving the parent out by mistake must not move the object to the
    /// root, so the root is asked for by name.
    #[test]
    fn object_parent_needs_a_parent_or_root() {
        for args in [
            &["calyx_ctl", "object", "parent", "Roof"][..],
            &["calyx_ctl", "object", "parent", "Roof", "House", "--root"][..],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
        }
    }
}
