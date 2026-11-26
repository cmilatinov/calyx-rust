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

#[test]
fn screenshot_passes_a_requested_size() {
    assert_eq!(
        command(&[
            "screenshot",
            "--target",
            "game",
            "--size",
            "1920x1080",
            "--out",
            "shot.png"
        ]),
        Command::Screenshot {
            target: CaptureTarget::Game,
            path: PathBuf::from("shot.png"),
            size: Some([1920, 1080]),
        }
    );
    assert_eq!(
        command(&["screenshot", "--target", "game", "--out", "shot.png"]),
        Command::Screenshot {
            target: CaptureTarget::Game,
            path: PathBuf::from("shot.png"),
            size: None,
        }
    );
}

/// Zero and oversized sizes parse; the editor refuses them with
/// `bad_request`, since only it knows the sizes it can render at.
#[test]
fn screenshot_size_must_be_width_x_height() {
    assert_eq!(parse_size("1920x1080"), Ok([1920, 1080]));
    assert_eq!(parse_size("640X480"), Ok([640, 480]));
    assert_eq!(parse_size("0x0"), Ok([0, 0]));
    for value in [
        "1920",
        "1920x",
        "x1080",
        "1920x1080x2",
        "-1x5",
        "wide x tall",
        "",
    ] {
        assert!(parse_size(value).is_err(), "{value:?}");
    }
    assert!(Cli::try_parse_from([
        "calyx_ctl",
        "screenshot",
        "--target",
        "game",
        "--size",
        "1920",
        "--out",
        "shot.png",
    ])
    .is_err());
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
