use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use sharedlib::{Func, Lib, Symbol};

use engine::background::Background;
use engine::class_registry::ClassRegistry;
use engine::rusty_pool::JoinHandle;
use project::Project;
use reflect::type_registry::TypeRegistry;
use utils::singleton_with_init;

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
                println!("Unable to load project: {}", e);
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
            let mut child = Command::new("cargo")
                .current_dir(root)
                .arg("build")
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            Self::pipe_stdout(&mut child);
            ProjectManager::get_mut().load_assemblies();
        })
    }

    pub fn rebuild_assemblies(&self) -> JoinHandle<()> {
        let project = self.current_project.as_ref().unwrap();
        let root = project.root_directory().clone();
        let name = project.name().clone();
        Background::get_mut().execute(TaskId::Build, move || {
            let mut clean = Command::new("cargo")
                .current_dir(root.clone())
                .args(["clean", "-p", name.as_str()])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            Self::pipe_stdout(&mut clean);
            let mut build = Command::new("cargo")
                .current_dir(root)
                .arg("build")
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            Self::pipe_stdout(&mut build);
            ProjectManager::get_mut().load_assemblies();
        })
    }

    pub fn load_assemblies(&mut self) {
        let mut root = self.root_project_dir();
        root.push("target");
        root.push("debug");
        root.push(format!("lib{}", self.current_project().name()));
        root.set_extension("so");
        println!("{:?}", root);
        unsafe {
            let lib = Lib::new(root).unwrap();
            let load_fn: Func<extern "C" fn(&mut TypeRegistry)> =
                lib.find_func("plugin_main").unwrap();
            {
                let mut registry = TypeRegistry::get_mut();
                load_fn.get()(&mut registry);
            }
            self.assembly = Some(lib);
            ClassRegistry::get_mut().refresh_class_lists();
        }
    }
}

singleton_with_init!(ProjectManager);
