use project::Project;
use std::path::PathBuf;
use utils::singleton_with_init;

#[derive(Default)]
pub struct ProjectManager {
    current_project: Option<Project>,
}

impl ProjectManager {
    pub fn load(&mut self, path: PathBuf) {
        match Project::load(path) {
            Ok(project) => self.current_project = Some(project),
            Err(e) => {
                println!("Unable to load project: {}", e);
            }
        }
    }

    pub fn current_project(&self) -> &Project {
        self.current_project
            .as_ref()
            .expect("Unable to load project")
    }
}

singleton_with_init!(ProjectManager);
