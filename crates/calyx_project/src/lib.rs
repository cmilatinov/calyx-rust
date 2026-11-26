use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use calyx_engine::runtime::RuntimeSettings;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tinytemplate::TinyTemplate;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    name: String,
    creation_date: String,
    /// The scene play starts in, relative to the project root; read by
    /// [`calyx_engine::runtime::ProjectRuntimeSettings`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    startup_scene: Option<PathBuf>,
    /// Written with its defaults so new projects show the runtime settings
    /// [`calyx_engine::runtime::ProjectRuntimeSettings`] reads back.
    #[serde(default)]
    runtime: RuntimeSettings,
    #[serde(default, skip_serializing_if = "InspectorSettings::is_empty")]
    inspector: InspectorSettings,
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
            startup_scene: None,
            runtime: RuntimeSettings::default(),
            inspector: InspectorSettings::default(),
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
mod tests;
