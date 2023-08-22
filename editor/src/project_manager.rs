use std::path::PathBuf;
use project::Project;
use utils::singleton_with_init;

pub struct ProjectManager {
    current_project: Option<Project>
}

impl ProjectManager {
    pub fn load(&mut self, path: PathBuf) {
        match Project::load(path) {
            Ok(project) => {
                self.current_project = Some(project)
            }
            Err(e) => {
                println!("Unable to load project: {}", e);
            }
        }
    }

    pub fn current_project(&self) -> &Project {
        self.current_project.as_ref().expect("Unable to load project")
    }
}

impl Default for ProjectManager {
    fn default() -> Self {
        ProjectManager {
            current_project: None
        }
    }
}

singleton_with_init!(ProjectManager);