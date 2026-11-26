use std::fs;
use std::path::{Path, PathBuf};

use calyx_engine::runtime::ProjectRuntimeSettings;
use uuid::Uuid;

use super::{PinnedComponent, Project};

#[test]
fn project_generate_and_load() {
    // Generate project
    let name = "TestProject".to_string();
    let folder = "temp".to_string();
    let project = Project::generate(name.clone(), Some(folder.clone()));

    // Validate project creation
    assert_eq!(project.name, name);
    let expected_path = Path::new("temp").join("TestProject");
    assert_eq!(project.root_directory, expected_path.clone());

    // Check assets folder was created
    let assets_path = expected_path.join("assets");
    assert!(Path::new(&assets_path).is_dir());

    // Check the .toml file
    let toml_path = expected_path.join("project.toml");
    let toml_content = fs::read_to_string(toml_path).expect("Unable to load toml content");
    assert!(toml_content.contains(&format!("name = \"{name}\"")));
    assert!(toml_content.contains("[runtime.window]"));

    // Load the project
    let loaded_project = Project::load(expected_path.clone()).unwrap();

    // Validate loaded project
    assert_eq!(loaded_project.name, name);
    assert_eq!(loaded_project.root_directory, expected_path);
    let runtime = ProjectRuntimeSettings::load(&expected_path).unwrap();
    assert_eq!(runtime.startup_scene_path(), None);
    assert_eq!(runtime.window().non_zero_size(), Some((1280, 720)));

    // Clean up
    fs::remove_dir_all(Path::new("temp")).unwrap();
}

/// A fresh directory holding `manifest` as its `project.toml`.
fn project_dir(test: &str, manifest: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("calyx-project-{test}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("project.toml"), manifest).unwrap();
    dir
}

const MANIFEST: &str = "name = \"Pins\"\n\
    creation_date = \"2026-01-01T00:00:00Z\"\n\
    startup_scene = \"assets/scene.cxscene\"\n";

#[test]
fn manifest_without_inspector_section_has_no_pinned_list() {
    let dir = project_dir("no-pins", MANIFEST);

    let project = Project::load(dir.clone()).unwrap();

    assert_eq!(project.inspector().pinned, None);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pinned_list_round_trips_and_keeps_the_startup_scene() {
    let dir = project_dir("round-trip", MANIFEST);
    let transform = Uuid::from_u128(2);
    let mut project = Project::load(dir.clone()).unwrap();

    project.inspector_mut().pinned = Some(vec![PinnedComponent {
        uuid: transform,
        name: "Transform".into(),
    }]);
    project.save().unwrap();

    let manifest = fs::read_to_string(dir.join("project.toml")).unwrap();
    assert!(manifest.contains("[[inspector.pinned]]"), "{manifest}");
    let loaded = Project::load(dir.clone()).unwrap();
    assert_eq!(
        loaded.inspector().pinned,
        Some(vec![PinnedComponent {
            uuid: transform,
            name: "Transform".into(),
        }])
    );
    let runtime = ProjectRuntimeSettings::load(&dir).unwrap();
    assert_eq!(
        runtime.startup_scene_path(),
        Some(dir.join("assets/scene.cxscene"))
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn empty_pinned_list_is_kept() {
    let dir = project_dir("empty-pins", MANIFEST);
    let mut project = Project::load(dir.clone()).unwrap();

    project.inspector_mut().pinned = Some(Vec::new());
    project.save().unwrap();

    let loaded = Project::load(dir.clone()).unwrap();
    assert_eq!(loaded.inspector().pinned, Some(Vec::new()));
    fs::remove_dir_all(dir).unwrap();
}
