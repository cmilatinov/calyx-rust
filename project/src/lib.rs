use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use engine::runtime::RuntimeSettings;
use serde::{Deserialize, Serialize};
use tinytemplate::TinyTemplate;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    name: String,
    creation_date: String,
    /// Written with its defaults so new projects show the runtime settings
    /// [`engine::runtime::ProjectRuntimeSettings`] reads back.
    #[serde(default)]
    runtime: RuntimeSettings,
    #[serde(default, skip_serializing_if = "InspectorSettings::is_empty")]
    inspector: InspectorSettings,
    /// Settings this struct does not model, such as `startup_scene`, kept so
    /// [`Self::save`] writes them back.
    #[serde(flatten)]
    other: toml::Table,
    #[serde(skip)]
    root_directory: PathBuf,
}

/// The editor inspector's settings, under `[inspector]`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InspectorSettings {
    /// The components the game object inspector shows first, in order.
    /// `None` leaves the editor's defaults; a list, even an empty one,
    /// replaces them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<Vec<PinnedComponent>>,
}

impl InspectorSettings {
    fn is_empty(&self) -> bool {
        self.pinned.is_none()
    }
}

/// A component type pinned to the top of the inspector.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedComponent {
    /// The component's type UUID.
    pub uuid: Uuid,
    /// The component's display name, only there to make the file readable.
    pub name: String,
}

#[derive(Debug, Serialize)]
struct CargoTemplateContext {
    project_name: String,
}

impl Project {
    pub fn generate(name: String, folder: Option<String>) -> Self {
        // Check first that the template exists so the function fails before creating any folders
        let mut tt = TinyTemplate::new();
        tt.add_template("cargo template", include_str!("../assets/cargo.template"))
            .expect("Unable to extract cargo template.");

        let base_directory = match folder {
            Some(curr_folder) => Path::new(&curr_folder).to_path_buf(),
            None => Path::new(".").to_path_buf(),
        };

        // Create folders if they're missing
        let project_directory = base_directory.join(&name);
        let assets_directory = project_directory.join("assets");
        fs::create_dir_all(&assets_directory).expect("Unable to create assets directory.");

        let project = Project {
            name,
            creation_date: Utc::now().to_rfc3339(),
            runtime: RuntimeSettings::default(),
            inspector: InspectorSettings::default(),
            other: toml::Table::new(),
            root_directory: project_directory,
        };

        let toml_string = toml::to_string(&project).expect("Failed to serialize to TOML");
        let toml_path = project.root_directory.join("project.toml");
        let mut file = File::create(toml_path).expect("Failed to create TOML file");

        file.write_all(toml_string.as_bytes())
            .expect("Failed to write to TOML file");

        let mut cargo_file = File::create(project.root_directory.join("Cargo.toml"))
            .expect("Unable to create cargo file.");

        let cargo_content = tt
            .render(
                "cargo template",
                &CargoTemplateContext {
                    project_name: project.name.clone(),
                },
            )
            .expect("Unable to apply template to cargo.toml");

        cargo_file
            .write_all(cargo_content.as_bytes())
            .expect("Unable to write content of cargo.toml");

        File::create(assets_directory.join("lib.rs")).expect("Unable to create lib.rs");

        project
    }

    pub fn load(directory: PathBuf) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let toml_path = directory.join("project.toml");

        let mut file = match File::open(toml_path) {
            Ok(file) => file,
            Err(e) => return Err(format!("Failed to find project file 'project.toml': {e}").into()),
        };

        let mut toml_string = String::new();
        if let Err(e) = file.read_to_string(&mut toml_string) {
            return Err(format!("Failed to read project file 'project.toml': {e}").into());
        }

        let mut project: Project = match toml::from_str(&toml_string) {
            Ok(project) => project,
            Err(e) => {
                return Err(
                    format!("Failed to deserialize project file 'project.toml': {e}").into(),
                );
            }
        };
        project.root_directory = directory;

        Ok(project)
    }

    /// Writes the project back to its `project.toml`.
    pub fn save(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let toml_string = toml::to_string(self)
            .map_err(|e| format!("Failed to serialize project file 'project.toml': {e}"))?;
        fs::write(self.root_directory.join("project.toml"), toml_string)
            .map_err(|e| format!("Failed to write project file 'project.toml': {e}"))?;
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        self.root_directory.exists()
            && self.assets_directory().exists()
            && self.root_directory.join("project.toml").exists()
            && self.root_directory.join("Cargo.toml").exists()
            && self.assets_directory().join("lib.rs").exists()
    }
}

impl Project {
    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn creation_date(&self) -> &String {
        &self.creation_date
    }

    pub fn root_directory(&self) -> &PathBuf {
        &self.root_directory
    }

    pub fn assets_directory(&self) -> PathBuf {
        self.root_directory.join("assets")
    }

    pub fn inspector(&self) -> &InspectorSettings {
        &self.inspector
    }

    pub fn inspector_mut(&mut self) -> &mut InspectorSettings {
        &mut self.inspector
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use engine::runtime::ProjectRuntimeSettings;
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
    fn pinned_list_round_trips_and_keeps_other_settings() {
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
}
