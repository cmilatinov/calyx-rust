use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tinytemplate::TinyTemplate;

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    name: String,
    creation_date: String,
    #[serde(skip_serializing, skip_deserializing)]
    root_directory: PathBuf,
}

#[derive(Debug, Serialize)]
struct CargoTemplateCtx {
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
                &CargoTemplateCtx {
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

    pub fn load(directory: PathBuf) -> Result<Self, String> {
        let toml_path = directory.join("project.toml");

        let mut file = match File::open(toml_path) {
            Ok(file) => file,
            Err(e) => return Err(format!("Failed to find project file 'project.toml': {}", e)),
        };

        let mut toml_string = String::new();
        if let Err(e) = file.read_to_string(&mut toml_string) {
            return Err(format!("Failed to read project file 'project.toml': {}", e));
        }

        let mut project: Project = match toml::from_str(&toml_string) {
            Ok(project) => project,
            Err(e) => {
                return Err(format!(
                    "Failed to deserialize project file 'project.toml': {}",
                    e
                ));
            }
        };
        project.root_directory = directory;

        Ok(project)
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
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::Project;

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
        assert!(toml_content.contains(&format!("name = \"{}\"", name)));
        assert!(toml_content.contains(&format!(
            "root_directory = \"{}\"",
            expected_path.clone().display()
        )));

        // Load the project
        let loaded_project = Project::load(expected_path.clone()).unwrap();

        // Validate loaded project
        assert_eq!(loaded_project.name, name);
        assert_eq!(loaded_project.root_directory, expected_path);

        // Clean up
        fs::remove_dir_all(Path::new("temp")).unwrap();
    }
}
