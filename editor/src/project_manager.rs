use sharedlib::{Lib, Symbol};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

use crate::task_id::TaskId;
use engine::background::Background;
use engine::context::AssetContext;
use engine::core::{Ref, WeakRef};
use engine::error::BoxedError;
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::TypeInfo;
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
        log::info!("Loading project from {}", project_directory.display());
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
        log::info!("Switching project to {}", path.display());
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
            let profile = assembly_profile();
            let args = cargo_build_args(profile);
            log::info!(
                "Building project assemblies in {} with profile {}",
                root.display(),
                profile
            );
            let output = Command::new("cargo").current_dir(root).args(&args).output();
            let command = format!("cargo {}", args.join(" "));
            match output {
                Ok(output) if output.status.success() => {
                    log_command_output(command.as_str(), &output.stdout, &output.stderr);
                    log::info!("Project assemblies built successfully");
                    project_manager_ref.write().load_assemblies();
                }
                Ok(output) => {
                    log_command_output(command.as_str(), &output.stdout, &output.stderr);
                    log::error!(
                        "Project assembly build failed with status {}",
                        output.status
                    );
                }
                Err(err) => {
                    log::error!("Failed to start project assembly build: {err}");
                }
            }
        })
    }

    pub fn load_assemblies(&mut self) {
        let root = self.root_project_dir();
        log::trace!("Loading project assemblies from {}", root.display());
        let meta_output = match Command::new("cargo")
            .current_dir(root)
            .arg("metadata")
            .output()
        {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                log_command_output("cargo metadata", &output.stdout, &output.stderr);
                log::error!("Failed to read cargo metadata; status {}", output.status);
                return;
            }
            Err(err) => {
                log::error!("Failed to start cargo metadata: {err}");
                return;
            }
        };
        let json: Value = match serde_json::from_slice(&meta_output.stdout) {
            Ok(json) => json,
            Err(err) => {
                log::error!("Failed to parse cargo metadata: {err}");
                return;
            }
        };
        let Some(target_directory) = json["target_directory"].as_str() else {
            log::error!("Cargo metadata did not include a target directory");
            return;
        };
        let mut target = PathBuf::from(target_directory);
        target.push(assembly_target_dir(assembly_profile()));
        target.push(engine::utils::lib_file_name(
            self.current_project().name().as_str(),
        ));
        unsafe {
            match Lib::new(&target) {
                Ok(lib) => {
                    if let Ok(load_fn) =
                        lib.find_func::<extern "C" fn(&mut TypeRegistry), &str>("plugin_main")
                    {
                        log::info!(
                            "Loading plugin type registrations from crate {} ({})",
                            self.current_project().name(),
                            target.display()
                        );
                        let mut registry = self.context.registries.types.write();
                        let before = registry.types.keys().copied().collect::<HashSet<_>>();
                        load_fn.get()(&mut registry);
                        let mut registered_count = 0usize;
                        for (id, registration) in
                            registry.types.iter().filter(|(id, _)| !before.contains(id))
                        {
                            registered_count += 1;
                            let (kind, type_name, details) =
                                type_registration_summary(&registration.type_info);
                            log::trace!(
                                "Registered project type uuid={} crate={} module={} type={} kind={} traits={} {}",
                                id,
                                crate_name(type_name),
                                module_path(type_name),
                                type_name,
                                kind,
                                registration.trait_meta.len(),
                                details
                            );
                        }
                        log::info!(
                            "Loaded {registered_count} project type registrations from crate {}",
                            self.current_project().name()
                        );
                    }
                    self.assembly = Some(lib);
                    let component_registry_ref = self.context.registries.components.clone();
                    component_registry_ref
                        .write()
                        .refresh_class_lists(&self.context.registries.types.read());
                    log::info!("Project assemblies loaded");
                }
                Err(err) => log::error!("Failed to load project assembly: {err}"),
            }
        }
    }
}

fn log_command_output(command: &str, stdout: &[u8], stderr: &[u8]) {
    for line in String::from_utf8_lossy(stdout).lines() {
        log::trace!("{command} stdout: {line}");
    }
    for line in String::from_utf8_lossy(stderr).lines() {
        log::warn!("{command} stderr: {line}");
    }
}

fn type_registration_summary(type_info: &TypeInfo) -> (&'static str, &'static str, String) {
    match type_info {
        TypeInfo::Struct(info) => (
            "struct",
            info.type_name,
            format!("fields={}", info.fields.len()),
        ),
        TypeInfo::Enum(info) => (
            "enum",
            info.type_name,
            format!("variants={}", info.variants.len()),
        ),
        TypeInfo::List(info) => (
            "list",
            info.type_name,
            format!(
                "element_type={} element_uuid={}",
                info.element.type_name, info.element.type_uuid
            ),
        ),
        TypeInfo::Option(info) => (
            "option",
            info.type_name,
            format!(
                "value_type={} value_uuid={}",
                info.value.type_name, info.value.type_uuid
            ),
        ),
        TypeInfo::Map(info) => (
            "map",
            info.type_name,
            format!(
                "key_type={} key_uuid={} value_type={} value_uuid={}",
                info.key.type_name, info.key.type_uuid, info.value.type_name, info.value.type_uuid
            ),
        ),
        TypeInfo::None => ("unknown", "<unknown>", "metadata=none".to_string()),
    }
}

fn crate_name(type_name: &str) -> &str {
    type_name.split("::").next().unwrap_or(type_name)
}

fn module_path(type_name: &str) -> &str {
    type_name.rsplit_once("::").map_or("", |(module, _)| module)
}

fn assembly_profile() -> &'static str {
    env!("CALYX_EDITOR_PROFILE")
}

fn cargo_build_args(profile: &str) -> Vec<String> {
    let mut args = vec!["build".to_string(), "--lib".to_string()];
    if profile != "dev" {
        args.push("--profile".to_string());
        args.push(profile.to_string());
    }
    args
}

fn assembly_target_dir(profile: &str) -> &str {
    match profile {
        "dev" | "test" => "debug",
        "release" | "bench" => "release",
        profile => profile,
    }
}

#[cfg(test)]
mod tests {
    use super::{assembly_target_dir, cargo_build_args};

    #[test]
    fn dev_profile_uses_default_cargo_build() {
        assert_eq!(cargo_build_args("dev"), ["build", "--lib"]);
        assert_eq!(assembly_target_dir("dev"), "debug");
    }

    #[test]
    fn custom_profile_is_forwarded_to_cargo_and_target_dir() {
        assert_eq!(
            cargo_build_args("release-with-debug"),
            ["build", "--lib", "--profile", "release-with-debug"]
        );
        assert_eq!(
            assembly_target_dir("release-with-debug"),
            "release-with-debug"
        );
    }
}
