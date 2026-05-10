use sharedlib::{Lib, Symbol};
use std::path::PathBuf;
use std::process::Command;

use crate::task_id::TaskId;
use engine::background::Background;
use engine::context::AssetContext;
use engine::core::{Ref, WeakRef};
use engine::error::BoxedError;
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::TypeInfo;
use log::{error, info, trace, warn};
use project::Project;
use rusty_pool::JoinHandle;
use serde_json::Value;

pub struct ProjectManager {
    current_project: Project,
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
        info!("Loading project from {}", project_directory.display());
        let current_project = Project::load(project_directory)?;
        Ok(Ref::new_cyclic(move |weak| Self {
            current_project,
            assembly: None,
            context,
            background,
            project_manager: weak,
        }))
    }

    pub fn load(&mut self, path: impl Into<PathBuf>) -> Result<(), BoxedError> {
        let path = path.into();
        info!("Switching project to {}", path.display());
        self.current_project = Project::load(path)?;
        Ok(())
    }

    pub fn current_project(&self) -> &Project {
        &self.current_project
    }

    fn root_project_dir(&self) -> PathBuf {
        self.current_project.root_directory().clone()
    }

    pub fn build_assemblies(&self) -> JoinHandle<()> {
        let root = self.root_project_dir();
        let project_manager_ref = self.project_manager.upgrade().unwrap();
        self.background.write().execute(TaskId::Build, move || {
            info!("Building project assemblies in {}", root.display());
            let output = Command::new("cargo")
                .current_dir(root)
                .args(["build", "--profile", "release-with-debug"])
                .output();
            match output {
                Ok(output) if output.status.success() => {
                    log_command_output("cargo build", &output.stdout, &output.stderr);
                    info!("Project assemblies built successfully");
                    project_manager_ref.write().load_assemblies();
                }
                Ok(output) => {
                    log_command_output("cargo build", &output.stdout, &output.stderr);
                    error!(
                        "Project assembly build failed with status {}",
                        output.status
                    );
                }
                Err(err) => {
                    error!("Failed to start project assembly build: {err}");
                }
            }
        })
    }

    pub fn load_assemblies(&mut self) {
        let root = self.root_project_dir();
        trace!("Loading project assemblies from {}", root.display());
        let meta_output = match Command::new("cargo")
            .current_dir(root)
            .arg("metadata")
            .output()
        {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                log_command_output("cargo metadata", &output.stdout, &output.stderr);
                error!("Failed to read cargo metadata; status {}", output.status);
                return;
            }
            Err(err) => {
                error!("Failed to start cargo metadata: {err}");
                return;
            }
        };
        let json: Value = match serde_json::from_slice(&meta_output.stdout) {
            Ok(json) => json,
            Err(err) => {
                error!("Failed to parse cargo metadata: {err}");
                return;
            }
        };
        let Some(target_directory) = json["target_directory"].as_str() else {
            error!("Cargo metadata did not include a target directory");
            return;
        };
        let mut target = PathBuf::from(target_directory);
        target.push("release-with-debug");
        target.push(engine::utils::lib_file_name(
            self.current_project().name().as_str(),
        ));
        unsafe {
            match Lib::new(target) {
                Ok(lib) => {
                    if let Ok(load_fn) =
                        lib.find_func::<extern "C" fn(&mut TypeRegistry), &str>("plugin_main")
                    {
                        info!("Loading plugin type registrations");
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
                    info!("Project assemblies loaded");
                }
                Err(err) => error!("Failed to load project assembly: {err}"),
            }
        }
    }
}

fn log_command_output(command: &str, stdout: &[u8], stderr: &[u8]) {
    for line in String::from_utf8_lossy(stdout).lines() {
        trace!("{command} stdout: {line}");
    }
    for line in String::from_utf8_lossy(stderr).lines() {
        warn!("{command} stderr: {line}");
    }
}
