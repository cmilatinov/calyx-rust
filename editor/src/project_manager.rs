use sharedlib::{Lib, Symbol};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::task_id::TaskId;
use engine::background::Background;
use engine::context::AssetContext;
use engine::core::{Ref, WeakRef};
use engine::error::BoxedError;
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::TypeInfo;
use project::Project;
use rusty_pool::JoinHandle;

pub struct ProjectManager {
    current_project: Project,
    context: AssetContext,
    background: Ref<Background>,
    project_manager: WeakRef<ProjectManager>,
    // Keep the dynamic assembly after contexts/registries in field order so
    // plugin-backed trait objects and scene components drop before the library
    // is unloaded during editor shutdown.
    assembly: Option<Lib>,
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
            context,
            background,
            project_manager: weak,
            assembly: None,
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
        let project_name = self.current_project().name().to_string();
        let target_dir = assembly_build_target_dir();
        let project_manager_ref = self.project_manager.upgrade().unwrap();
        self.background.write().execute(TaskId::Build, move || {
            let profile = assembly_profile();
            let args = cargo_build_args(profile, &target_dir);
            let source = assembly_artifact_path(&target_dir, profile, &project_name);
            let loaded_existing = source.exists();
            if loaded_existing {
                log::info!(
                    "Loading existing project assembly before background build: {}",
                    source.display()
                );
                project_manager_ref.write().load_assemblies();
            }
            log::info!(
                "Building project assemblies in {} with profile {}",
                root.display(),
                profile
            );
            let output = Command::new("cargo").current_dir(root).args(&args).output();
            let command = format!("cargo {}", args.join(" "));
            match output {
                Ok(output) if output.status.success() => {
                    log_command_output(
                        command.as_str(),
                        &output.stdout,
                        &output.stderr,
                        CommandOutputStatus::Success,
                    );
                    log::info!("Project assemblies built successfully");
                    if !source.exists() {
                        log::error!("Project assembly was not produced at {}", source.display());
                        return;
                    }
                    project_manager_ref.write().load_assemblies();
                }
                Ok(output) => {
                    log_command_output(
                        command.as_str(),
                        &output.stdout,
                        &output.stderr,
                        CommandOutputStatus::Failure,
                    );
                    log::error!(
                        "Project assembly build failed with status {}",
                        output.status
                    );
                    if source.exists() && !loaded_existing {
                        log::warn!(
                            "Loading existing project assembly after failed build: {}",
                            source.display()
                        );
                        project_manager_ref.write().load_assemblies();
                    }
                }
                Err(err) => {
                    log::error!("Failed to start project assembly build: {err}");
                    if source.exists() && !loaded_existing {
                        log::warn!(
                            "Loading existing project assembly after build command failed: {}",
                            source.display()
                        );
                        project_manager_ref.write().load_assemblies();
                    }
                }
            }
        })
    }

    pub fn load_existing_assemblies(&mut self) -> bool {
        let source = self.assembly_source_path();
        if !source.exists() {
            log::info!("No project assembly found at {}", source.display());
            return false;
        }
        log::info!("Loading existing project assembly: {}", source.display());
        self.load_assemblies()
    }

    fn assembly_source_path(&self) -> PathBuf {
        assembly_artifact_path(
            &assembly_build_target_dir(),
            assembly_profile(),
            self.current_project().name().as_str(),
        )
    }

    pub fn load_assemblies(&mut self) -> bool {
        let root = self.root_project_dir();
        log::trace!("Loading project assemblies from {}", root.display());
        let source = self.assembly_source_path();
        let target = match shadow_copy_assembly(&source, self.current_project().name().as_str()) {
            Ok(target) => target,
            Err(err) => {
                log::error!(
                    "Failed to prepare project assembly {}: {err}",
                    source.display()
                );
                return false;
            }
        };
        unsafe {
            let _preloaded_dependencies =
                match PreloadedAssemblyDependencies::load(target.parent(), target.file_name()) {
                    Ok(dependencies) => Some(dependencies),
                    Err(err) => {
                        log::warn!(
                            "Failed to preload assembly dependencies for {}: {err}",
                            target.display()
                        );
                        None
                    }
                };
            let _assembly_load_path = match AssemblyLoadPath::push(target.parent()) {
                Ok(path) => Some(path),
                Err(err) => {
                    log::warn!(
                        "Failed to add assembly directory for {}: {err}",
                        target.display()
                    );
                    None
                }
            };
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
                    true
                }
                Err(err) => {
                    log::error!("Failed to load project assembly: {err}");
                    false
                }
            }
        }
    }
}

struct PreloadedAssemblyDependencies {
    #[allow(dead_code)]
    libs: Vec<AssemblyDependency>,
}

impl PreloadedAssemblyDependencies {
    unsafe fn load(
        path: Option<&Path>,
        assembly_file_name: Option<&std::ffi::OsStr>,
    ) -> std::io::Result<Self> {
        let Some(path) = path else {
            return Ok(Self { libs: Vec::new() });
        };
        let mut libs = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let dependency = entry.path();
            if dependency.file_name() == assembly_file_name {
                continue;
            }
            if is_runtime_library(&dependency) {
                libs.push(AssemblyDependency::load(&dependency)?);
            }
        }
        Ok(Self { libs })
    }
}

struct AssemblyDependency {
    #[allow(dead_code)]
    lib: Lib,
}

impl AssemblyDependency {
    unsafe fn load(path: &Path) -> std::io::Result<Self> {
        Lib::new(path)
            .map(|lib| Self { lib })
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error.to_string()))
    }
}

struct AssemblyLoadPath;

impl AssemblyLoadPath {
    fn push(path: Option<&Path>) -> std::io::Result<Self> {
        set_dll_directory(path)?;
        Ok(Self)
    }
}

impl Drop for AssemblyLoadPath {
    fn drop(&mut self) {
        if let Err(err) = set_dll_directory(None) {
            log::warn!("Failed to reset assembly DLL search path: {err}");
        }
    }
}

#[cfg(windows)]
fn set_dll_directory(path: Option<&Path>) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn SetDllDirectoryW(lp_path_name: *const u16) -> i32;
    }

    let wide_path;
    let path_ptr = if let Some(path) = path {
        wide_path = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        wide_path.as_ptr()
    } else {
        std::ptr::null()
    };

    let result = unsafe { SetDllDirectoryW(path_ptr) };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn set_dll_directory(_path: Option<&Path>) -> std::io::Result<()> {
    Ok(())
}

enum CommandOutputStatus {
    Success,
    Failure,
}

fn log_command_output(command: &str, stdout: &[u8], stderr: &[u8], status: CommandOutputStatus) {
    for line in String::from_utf8_lossy(stdout).lines() {
        log::trace!("{command} stdout: {line}");
    }
    for line in String::from_utf8_lossy(stderr).lines() {
        match status {
            CommandOutputStatus::Success => log::info!("{command} output: {line}"),
            CommandOutputStatus::Failure => log::warn!("{command} stderr: {line}"),
        }
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

fn cargo_build_args(profile: &str, target_dir: &Path) -> Vec<String> {
    let mut args = vec![
        "build".to_string(),
        "--lib".to_string(),
        "--target-dir".to_string(),
        target_dir.display().to_string(),
    ];
    if profile != "dev" {
        args.push("--profile".to_string());
        args.push(profile.to_string());
    }
    args
}

fn assembly_build_target_dir() -> PathBuf {
    let Some(mut profile_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    else {
        return PathBuf::from("target");
    };
    if profile_dir.file_name().is_some_and(|name| name == "deps") {
        profile_dir.pop();
    }
    profile_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("target"))
}

fn assembly_artifact_path(target_dir: &Path, profile: &str, project_name: &str) -> PathBuf {
    target_dir
        .join(assembly_target_dir(profile))
        .join(engine::utils::lib_file_name(project_name))
}

fn shadow_copy_assembly(source: &Path, project_name: &str) -> std::io::Result<PathBuf> {
    let mut target_dir = std::env::current_exe()?;
    target_dir.pop();
    target_dir.push("project_assemblies");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    target_dir.push(format!("{project_name}_{timestamp}"));
    std::fs::create_dir_all(&target_dir)?;

    copy_runtime_dependencies(source, &target_dir)?;
    copy_rust_runtime_dependencies(&target_dir)?;
    let target = target_dir.join(
        source
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("assembly")),
    );
    std::fs::copy(source, &target)?;
    Ok(target)
}

fn copy_runtime_dependencies(source: &Path, target_dir: &Path) -> std::io::Result<()> {
    let Some(source_dir) = source.parent() else {
        return Ok(());
    };
    let source_file_name = source.file_name();
    for entry in std::fs::read_dir(source_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name() == source_file_name || !is_runtime_library(&path) {
            continue;
        }
        if let Some(file_name) = path.file_name() {
            std::fs::copy(&path, target_dir.join(file_name))?;
        }
    }
    Ok(())
}

fn copy_rust_runtime_dependencies(target_dir: &Path) -> std::io::Result<()> {
    for runtime_dir in rust_runtime_dirs()? {
        if !runtime_dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(runtime_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_rust_runtime_library(&path) {
                continue;
            }
            if let Some(file_name) = path.file_name() {
                std::fs::copy(&path, target_dir.join(file_name))?;
            }
        }
    }
    Ok(())
}

fn rust_runtime_dirs() -> std::io::Result<Vec<PathBuf>> {
    let sysroot = rustc_print("sysroot")?;
    let target_libdir = rustc_print("target-libdir")?;
    Ok(vec![sysroot.join("bin"), target_libdir])
}

fn rustc_print(value: &str) -> std::io::Result<PathBuf> {
    let output = Command::new("rustc").args(["--print", value]).output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("rustc --print {value} failed with status {}", output.status),
        ));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(path))
}

fn is_runtime_library(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(runtime_library_extension()))
}

fn is_rust_runtime_library(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    is_runtime_library(path) && (name.starts_with("std-") || name.starts_with("libstd-"))
}

fn runtime_library_extension() -> &'static str {
    #[cfg(windows)]
    return "dll";
    #[cfg(unix)]
    return "so";
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
    use super::{
        assembly_artifact_path, assembly_build_target_dir, assembly_target_dir, cargo_build_args,
        copy_runtime_dependencies, is_rust_runtime_library,
    };
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dev_profile_uses_default_cargo_build() {
        assert_eq!(
            cargo_build_args("dev", Path::new("target")),
            ["build", "--lib", "--target-dir", "target"]
        );
        assert_eq!(assembly_target_dir("dev"), "debug");
    }

    #[test]
    fn custom_profile_is_forwarded_to_cargo_and_target_dir() {
        assert_eq!(
            cargo_build_args("release-with-debug", Path::new("target")),
            [
                "build",
                "--lib",
                "--target-dir",
                "target",
                "--profile",
                "release-with-debug"
            ]
        );
        assert_eq!(
            assembly_target_dir("release-with-debug"),
            "release-with-debug"
        );
    }

    #[test]
    fn assembly_paths_use_isolated_target_dir() {
        let target = assembly_build_target_dir();
        assert!(target.ends_with("target"));
        assert_eq!(
            assembly_artifact_path(&target, "dev", "sandbox"),
            target
                .join("debug")
                .join(engine::utils::lib_file_name("sandbox"))
        );
    }

    #[test]
    fn runtime_dependencies_are_copied_next_to_shadow_assembly() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("calyx-assembly-copy-{suffix}"));
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();

        let source = source_dir.join(engine::utils::lib_file_name("sandbox"));
        let dependency = source_dir.join(engine::utils::lib_file_name("engine"));
        fs::write(&source, b"sandbox").unwrap();
        fs::write(&dependency, b"engine").unwrap();

        copy_runtime_dependencies(&source, &target_dir).unwrap();

        assert!(target_dir
            .join(engine::utils::lib_file_name("engine"))
            .exists());
        assert!(!target_dir
            .join(engine::utils::lib_file_name("sandbox"))
            .exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rust_runtime_library_detection_matches_std_only() {
        #[cfg(windows)]
        assert!(is_rust_runtime_library(Path::new(
            "std-00eb2f7586512494.dll"
        )));
        #[cfg(unix)]
        assert!(is_rust_runtime_library(Path::new(
            "libstd-00eb2f7586512494.so"
        )));
        #[cfg(windows)]
        assert!(!is_rust_runtime_library(Path::new(
            "rustc_driver-1815a83be396bd1c.dll"
        )));
        #[cfg(unix)]
        assert!(!is_rust_runtime_library(Path::new(
            "librustc_driver-1815a83be396bd1c.so"
        )));
    }
}
