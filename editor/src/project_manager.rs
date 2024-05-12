use std::io::{BufRead, BufReader};
use std::ops::DerefMut;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use sharedlib::{Lib, Symbol};

use engine::background::Background;
use engine::class_registry::ClassRegistry;
use engine::reflect::type_registry::TypeRegistry;
use engine::rusty_pool::JoinHandle;
use engine::serde_json::Value;
use engine::utils::singleton_with_init;
use project::Project;

use crate::task_id::TaskId;

#[derive(Default)]
pub struct ProjectManager {
    current_project: Option<Project>,
    assembly: Option<Lib>,
}

impl ProjectManager {
    pub fn load(&mut self, path: PathBuf) {
        match Project::load(path) {
            Ok(project) => self.current_project = Some(project),
            Err(e) => {
                eprintln!("Unable to load project: {}", e);
            }
        }
    }

    pub fn current_project(&self) -> &Project {
        self.current_project
            .as_ref()
            .expect("Unable to load project")
    }

    fn root_project_dir(&self) -> PathBuf {
        self.current_project
            .as_ref()
            .unwrap()
            .root_directory()
            .clone()
    }

    fn pipe_stdout(child: &mut Child) {
        let stdout = child.stdout.as_mut().unwrap();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                _ => {}
            }
        }
        child.wait().expect("TODO: panic message");
    }

    pub fn build_assemblies(&self) -> JoinHandle<()> {
        let root = self.root_project_dir();
        Background::get_mut().execute(TaskId::Build, move || {
            let mut build = Command::new("cargo")
                .current_dir(root)
                .args(["build", "--lib"])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            Self::pipe_stdout(&mut build);
            ProjectManager::get_mut().load_assemblies();
        })
    }

    pub fn load_assemblies(&mut self) {
        let root = self.root_project_dir();
        let meta_output = Command::new("cargo")
            .current_dir(root)
            .arg("metadata")
            .output()
            .expect("");
        let json: Value = engine::serde_json::from_slice(&meta_output.stdout).unwrap();
        let mut target = PathBuf::from(json["target_directory"].as_str().unwrap());
        target.push("debug");
        target.push(engine::utils::lib_file_name(
            self.current_project().name().as_str(),
        ));
        unsafe {
            match Lib::new(target) {
                Ok(lib) => {
                    if let Ok(load_fn) =
                        lib.find_func::<extern "C" fn(&mut TypeRegistry), &str>("plugin_main")
                    {
                        let mut registry = TypeRegistry::get_mut();
                        load_fn.get()(&mut registry);
                    }
                    self.assembly = Some(lib);
                    ClassRegistry::get_mut().refresh_class_lists();
                }
                Err(err) => eprintln!("{}", err),
            }
        }
    }
}

singleton_with_init!(ProjectManager);
