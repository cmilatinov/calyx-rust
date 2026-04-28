use sharedlib::{Lib, Symbol};
use std::env;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::task_id::TaskId;
use engine::background::Background;
use engine::context::AssetContext;
use engine::core::{Ref, WeakRef};
use engine::error::BoxedError;
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::TypeInfo;
use log::trace;
use project::Project;
use rusty_pool::JoinHandle;
use serde_json::Value;

pub struct ProjectManager {
    current_project: Project,
    cargo_profile: String,
    target_profile_dir: String,
    assembly: Option<Lib>,
    context: AssetContext,
    background: Ref<Background>,
    project_manager: WeakRef<ProjectManager>,
}

impl ProjectManager {
    pub fn new(
        context: AssetContext,
        project_directory: impl Into<PathBuf>,
        background: Ref<Background>,
    ) -> Result<Ref<Self>, BoxedError> {
        let project_directory = dunce::canonicalize(project_directory.into()).map_err(Box::new)?;
        let current_project = Project::load(project_directory)?;
        let (cargo_profile, target_profile_dir) = Self::infer_build_profile();
        Ok(Ref::new_cyclic(move |weak| Self {
            current_project,
            cargo_profile,
            target_profile_dir,
            assembly: None,
            context,
            background,
            project_manager: weak,
        }))
    }

    pub fn load(&mut self, path: impl Into<PathBuf>) -> Result<(), BoxedError> {
        self.current_project = Project::load(path.into())?;
        Ok(())
    }

    pub fn current_project(&self) -> &Project {
        &self.current_project
    }

    fn root_project_dir(&self) -> PathBuf {
        self.current_project.root_directory().clone()
    }

    fn infer_build_profile() -> (String, String) {
        let current_exe = env::current_exe().ok();
        let profile = current_exe
            .as_ref()
            .and_then(|path| path.parent())
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                if cfg!(debug_assertions) {
                    "debug".to_string()
                } else {
                    "release".to_string()
                }
            });

        let cargo_profile = match profile.as_str() {
            "debug" => "dev".to_string(),
            other => other.to_string(),
        };

        (cargo_profile, profile)
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
        let _ = child.wait();
    }

    pub fn build_assemblies(&self) -> JoinHandle<()> {
        let root = self.root_project_dir();
        let package = self.current_project().name().clone();
        let profile = self.cargo_profile.clone();
        let project_manager_ref = self.project_manager.upgrade().unwrap();
        self.background.write().execute(TaskId::Build, move || {
            // std::thread::sleep(Duration::from_secs(10));
            let mut build = Command::new("cargo")
                .current_dir(root)
                .args([
                    "build",
                    "--package",
                    package.as_str(),
                    "--lib",
                    "--profile",
                    profile.as_str(),
                ])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            Self::pipe_stdout(&mut build);
            project_manager_ref.write().load_assemblies();
        })
    }

    pub fn load_assemblies(&mut self) {
        let root = self.root_project_dir();
        let meta_output = Command::new("cargo")
            .current_dir(root)
            .arg("metadata")
            .output()
            .expect("");
        let json: Value = serde_json::from_slice(&meta_output.stdout).unwrap();
        let mut target = PathBuf::from(json["target_directory"].as_str().unwrap());
        target.push(&self.target_profile_dir);
        target.push(engine::utils::lib_file_name(
            self.current_project().name().as_str(),
        ));
        unsafe {
            match Lib::new(target) {
                Ok(lib) => {
                    if let Ok(load_fn) =
                        lib.find_func::<extern "C" fn(&mut TypeRegistry), &str>("plugin_main")
                    {
                        let mut registry = self.context.registries.types.write();
                        load_fn.get()(&mut registry);
                        for (id, registration) in &registry.types {
                            if let TypeInfo::Struct(info) = &registration.type_info {
                                trace!("[{}] {}", id, info.type_name);
                            }
                        }
                    }
                    self.assembly = Some(lib);
                    let component_registry_ref = self.context.registries.components.clone();
                    component_registry_ref
                        .write()
                        .refresh_class_lists(&self.context.registries.types.read());
                }
                Err(err) => eprintln!("{}", err),
            }
        }
    }
}
