use libloading::{Library, Symbol};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::task_id::TaskId;
use engine::background::Background;
use engine::context::AssetContext;
use engine::core::Ref;
use engine::error::BoxedError;
use engine::reflect::type_registry::{PluginEngineBuild, TypeRegistry};
use engine::reflect::TypeInfo;
use engine::resource::Resource;
use engine::runtime::{ProjectRuntimeSettings, RuntimeSettings};
use engine::scene::{Scene, SceneManager, SuspendedScenes};
use engine::utils::TypeUuid;
use project::Project;
use rusty_pool::JoinHandle;

#[derive(Default, Resource, TypeUuid)]
#[uuid = "1f7d437c-a775-4f7f-8474-a6b4a348704f"]
#[repr(C)]
pub struct ProjectAssemblyStatus {
    loaded: bool,
}

impl ProjectAssemblyStatus {
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn set_loaded(&mut self, loaded: bool) {
        self.loaded = loaded;
    }
}

pub struct ProjectManager {
    current_project: Project,
    runtime_settings: ProjectRuntimeSettings,
    context: AssetContext,
    background: Ref<Background>,
    assembly_status: Ref<ProjectAssemblyStatus>,
    builds: AssemblyBuildQueue,
    // Keep the dynamic assembly after contexts/registries in field order so
    // plugin-backed trait objects and scene components drop before the library
    // is unloaded during editor shutdown.
    assembly: Option<LoadedAssembly>,
}

impl ProjectManager {
    pub fn new(
        context: AssetContext,
        project_directory: impl Into<PathBuf>,
        background: Ref<Background>,
        assembly_status: Ref<ProjectAssemblyStatus>,
    ) -> Result<Ref<Self>, BoxedError> {
        let project_directory = dunce::canonicalize(project_directory.into()).map_err(Box::new)?;
        log::info!("Loading project from {}", project_directory.display());
        let runtime_settings = ProjectRuntimeSettings::load(&project_directory)?;
        let current_project = Project::load(project_directory)?;
        Ok(Ref::new(Self {
            current_project,
            runtime_settings,
            context,
            background,
            assembly_status,
            builds: AssemblyBuildQueue::new(),
            assembly: None,
        }))
    }

    pub fn load(&mut self, path: impl Into<PathBuf>) -> Result<(), BoxedError> {
        let path = path.into();
        log::info!("Switching project to {}", path.display());
        let runtime_settings = ProjectRuntimeSettings::load(&path)?;
        self.current_project = Project::load(path)?;
        self.runtime_settings = runtime_settings;
        Ok(())
    }

    pub fn current_project(&self) -> &Project {
        &self.current_project
    }

    /// Returns the runtime defaults declared by the current project.
    pub fn runtime_settings(&self) -> &RuntimeSettings {
        self.runtime_settings.runtime()
    }

    /// Starts the shared runtime from the project's configured startup scene.
    ///
    /// A project without a startup scene leaves the current editor scene alone.
    pub fn start_simulation(
        &self,
        scenes: &mut SceneManager,
    ) -> Result<Option<PathBuf>, BoxedError> {
        if scenes.has_simulation_scene() {
            scenes.start_simulation();
            return Ok(None);
        }
        let Some(path) = self.runtime_settings.startup_scene_path() else {
            scenes.start_simulation();
            return Ok(None);
        };
        if configured_scene_is_current(scenes, &path) {
            scenes.start_simulation();
            return Ok(Some(path));
        }
        let scene = self
            .context
            .registries
            .assets
            .read()
            .reload_by_path::<Scene>(&path)
            .map_err(Box::new)?;
        scenes.start_simulation_from_scene(scene.readonly());
        Ok(Some(path))
    }

    fn root_project_dir(&self) -> PathBuf {
        self.current_project.root_directory().clone()
    }

    /// Builds the project assembly on the background pool.
    ///
    /// The task only runs cargo and reports the library path; the UI thread
    /// loads it from [`Self::load_completed_build`].
    pub fn build_assemblies(&self) -> JoinHandle<()> {
        let root = self.root_project_dir();
        let project_name = self.current_project().name().to_string();
        let target_dir = assembly_build_target_dir();
        let reporter = self.builds.reporter();
        self.background.write().execute(TaskId::Build, move || {
            let profile = assembly_profile();
            let args = cargo_build_args(profile, &target_dir);
            let source = assembly_artifact_path(&target_dir, profile, &project_name);
            let outcome = match build_assembly(&root, profile, &args, source) {
                AssemblyBuildOutcome::Built(source) => {
                    snapshot_built_assembly(source, &project_name)
                }
                failed => failed,
            };
            // The receiver only goes away with the project manager.
            let _ = reporter.send(outcome);
        })
    }

    /// Takes the next assembly reported by a finished background build, for
    /// [`Self::load_build`].
    ///
    /// Call once per frame on the UI thread so the library swap and type
    /// re-registration never race with code using the plugin's types.
    pub fn completed_build(&self) -> Option<AssemblyLoad> {
        self.builds.next_load(self.assemblies_loaded())
    }

    /// Loads an assembly [`Self::completed_build`] returned, rebuilding the
    /// scenes `scenes` reaches through its types.
    pub fn load_build(&mut self, load: AssemblyLoad, scenes: &mut SceneManager) -> bool {
        match load {
            AssemblyLoad::Snapshot(target) => self.load_shadow_copy(&target, scenes),
            AssemblyLoad::Source(source) => self.load_assembly(&source, scenes),
        }
    }

    pub fn load_existing_assemblies(&mut self, scenes: &mut SceneManager) -> bool {
        let source = self.assembly_source_path();
        if !source.exists() {
            log::info!("No project assembly found at {}", source.display());
            self.refresh_assembly_status();
            return false;
        }
        if predates_editor(&source) {
            log::info!(
                "Project assembly {} is older than the editor, so it may be built against \
                 another engine; rebuilding it instead of loading it",
                source.display()
            );
            self.refresh_assembly_status();
            return false;
        }
        log::info!("Loading existing project assembly: {}", source.display());
        self.load_assembly(&source, scenes)
    }

    fn assembly_source_path(&self) -> PathBuf {
        assembly_artifact_path(
            &assembly_build_target_dir(),
            assembly_profile(),
            self.current_project().name().as_str(),
        )
    }

    fn load_assembly(&mut self, source: &Path, scenes: &mut SceneManager) -> bool {
        log::trace!("Loading project assembly from {}", source.display());
        let target = match shadow_copy_assembly(source, self.current_project().name().as_str()) {
            Ok(target) => target,
            Err(err) => {
                log::error!(
                    "Failed to prepare project assembly {}: {err}",
                    source.display()
                );
                self.refresh_assembly_status();
                return false;
            }
        };
        self.load_shadow_copy(&target, scenes)
    }

    /// Loads a shadow copy prepared by [`shadow_copy_assembly`], replacing the
    /// loaded assembly, if any, and rebuilding the scenes `scenes` reaches
    /// through the new assembly's types.
    fn load_shadow_copy(&mut self, target: &Path, scenes: &mut SceneManager) -> bool {
        unsafe {
            let preloaded_dependencies =
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
            match Library::new(target) {
                Ok(lib) => {
                    let suspended = {
                        let load_fn: Symbol<extern "C" fn(&mut TypeRegistry)> =
                            match lib.get(b"plugin_main\0") {
                                Ok(load_fn) => load_fn,
                                Err(err) => {
                                    log::error!(
                                        "Project assembly {} is missing plugin_main: {err}",
                                        target.display()
                                    );
                                    self.refresh_assembly_status();
                                    return false;
                                }
                            };
                        // Open scenes hold storages and registrations hold
                        // trait objects whose code is in the loaded assembly,
                        // so take both apart while it is still loaded.
                        let suspended = self.assemblies_loaded().then(|| self.unload_types(scenes));
                        log::info!(
                            "Loading plugin type registrations from crate {} ({})",
                            self.current_project().name(),
                            target.display()
                        );
                        let mut registry = self.context.registries.types.write();
                        registry.begin_plugin_load();
                        let before = registry.types.keys().copied().collect::<HashSet<_>>();
                        load_fn(&mut registry);
                        match registry.plugin_engine_build() {
                            PluginEngineBuild::Same => {}
                            PluginEngineBuild::Other => log::error!(
                                "Project assembly {} links another engine build than the editor, \
                                 so the engine components its code reads, writes or adds are not \
                                 the editor's. Cargo resolved engine's dependencies differently \
                                 for the project than for the editor; pin the editor's features \
                                 in engine/engine_feature_pins/Cargo.toml.",
                                target.display()
                            ),
                            PluginEngineBuild::Unknown => log::warn!(
                                "Project assembly {} did not report which engine build it links: \
                                 it predates that check or does not call register_crate. If it \
                                 links another engine build, the engine components its code reads, \
                                 writes or adds are not the editor's; rebuild it.",
                                target.display()
                            ),
                        }
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
                        suspended
                    };
                    // Replacing the previous assembly unloads it, so its
                    // shadow copy is no longer in use.
                    self.assembly = Some(LoadedAssembly {
                        _lib: lib,
                        _dependencies: preloaded_dependencies,
                    });
                    remove_stale_shadow_copies_of(target);
                    let component_registry_ref = self.context.registries.components.clone();
                    component_registry_ref
                        .write()
                        .refresh_class_lists(&self.context.registries.types.read());
                    if let Some(suspended) = suspended {
                        scenes.resume_scenes(suspended);
                        log::info!("Rebuilt open scenes through the reloaded project assembly");
                    }
                    self.refresh_assembly_status();
                    log::info!("Project assemblies loaded");
                    true
                }
                Err(err) => {
                    log::error!("Failed to load project assembly: {err}");
                    self.refresh_assembly_status();
                    false
                }
            }
        }
    }

    /// Drops everything the loaded assembly's code backs: the scenes `scenes`
    /// reaches, whose snapshots are returned, the assembly's type
    /// registrations and the components bound from them.
    fn unload_types(&self, scenes: &mut SceneManager) -> SuspendedScenes {
        let suspended = scenes.suspend_scenes();
        let mut types = self.context.registries.types.write();
        types.unload_plugins();
        self.context
            .registries
            .components
            .write()
            .refresh_class_lists(&types);
        suspended
    }

    pub fn assemblies_loaded(&self) -> bool {
        self.assembly.is_some()
    }

    fn refresh_assembly_status(&self) {
        self.assembly_status
            .write()
            .set_loaded(self.assemblies_loaded());
    }
}

/// Result of one background assembly build, reported to the UI thread.
#[derive(Debug, PartialEq)]
enum AssemblyBuildOutcome {
    /// The build succeeded; the path is a shadow copy of its library, taken
    /// before the worker is released so later builds cannot change it.
    Built(PathBuf),
    /// The build failed; the path is the previously built library, if any.
    Failed(PathBuf),
}

/// What the UI thread loads for a finished build.
#[derive(Debug, PartialEq)]
pub enum AssemblyLoad {
    /// A shadow copy taken by the build task, loaded as is.
    Snapshot(PathBuf),
    /// A previously built library, shadow-copied before loading.
    Source(PathBuf),
}

/// Hands finished background builds to the UI thread, one load per build.
struct AssemblyBuildQueue {
    sender: Sender<AssemblyBuildOutcome>,
    receiver: Mutex<Receiver<AssemblyBuildOutcome>>,
}

impl AssemblyBuildQueue {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver: Mutex::new(receiver),
        }
    }

    fn reporter(&self) -> Sender<AssemblyBuildOutcome> {
        self.sender.clone()
    }

    /// Returns the library to load for the next finished build, if any.
    ///
    /// A failed build keeps the loaded assembly, and only falls back to the
    /// previous library when nothing is loaded yet and that library is not
    /// older than the editor.
    fn next_load(&self, assembly_loaded: bool) -> Option<AssemblyLoad> {
        let outcome = self
            .receiver
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .try_recv()
            .ok()?;
        match outcome {
            AssemblyBuildOutcome::Built(target) => Some(AssemblyLoad::Snapshot(target)),
            AssemblyBuildOutcome::Failed(source) if !assembly_loaded && source.exists() => {
                if predates_editor(&source) {
                    log::error!(
                        "Project assembly build failed and {} is older than the editor, so it \
                         may be built against another engine; leaving it unloaded",
                        source.display()
                    );
                    return None;
                }
                log::warn!(
                    "Loading existing project assembly after failed build: {}",
                    source.display()
                );
                Some(AssemblyLoad::Source(source))
            }
            AssemblyBuildOutcome::Failed(_) => None,
        }
    }
}

/// Shadow-copies a freshly built library so the UI thread loads exactly this
/// build, even if another build replaces the artifact before it is loaded.
fn snapshot_built_assembly(source: PathBuf, project_name: &str) -> AssemblyBuildOutcome {
    match shadow_copy_assembly(&source, project_name) {
        Ok(target) => AssemblyBuildOutcome::Built(target),
        Err(err) => {
            log::error!(
                "Failed to prepare project assembly {}: {err}",
                source.display()
            );
            AssemblyBuildOutcome::Failed(source)
        }
    }
}

/// Runs cargo for the project assembly and reports where the library is.
fn build_assembly(
    root: &Path,
    profile: &str,
    args: &[String],
    source: PathBuf,
) -> AssemblyBuildOutcome {
    log::info!(
        "Building project assemblies in {} with profile {}",
        root.display(),
        profile
    );
    let output = Command::new("cargo").current_dir(root).args(args).output();
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
                return AssemblyBuildOutcome::Failed(source);
            }
            if let Ok(editor) = std::env::current_exe() {
                mark_built_for(&source, &editor);
            }
            AssemblyBuildOutcome::Built(source)
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
            AssemblyBuildOutcome::Failed(source)
        }
        Err(err) => {
            log::error!("Failed to start project assembly build: {err}");
            AssemblyBuildOutcome::Failed(source)
        }
    }
}

fn configured_scene_is_current(scenes: &SceneManager, startup_scene: &Path) -> bool {
    scene_paths_match(scenes.current_scene_meta().file.as_deref(), startup_scene)
}

fn scene_paths_match(current_scene: Option<&Path>, startup_scene: &Path) -> bool {
    let Some(current_scene) = current_scene else {
        return false;
    };
    current_scene == startup_scene
        || matches!(
            (dunce::canonicalize(current_scene), dunce::canonicalize(startup_scene)),
            (Ok(current_scene), Ok(startup_scene)) if current_scene == startup_scene
        )
}

/// A loaded project assembly and the dependencies preloaded for it.
///
/// Fields drop in declaration order, so the assembly unloads before the
/// dependencies it links against.
struct LoadedAssembly {
    _lib: Library,
    _dependencies: Option<PreloadedAssemblyDependencies>,
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
    lib: Library,
}

impl AssemblyDependency {
    unsafe fn load(path: &Path) -> std::io::Result<Self> {
        Library::new(path)
            .map(|lib| Self { lib })
            .map_err(std::io::Error::other)
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

/// Arguments of the `cargo` command that builds the project assembly.
pub fn cargo_build_args(profile: &str, target_dir: &Path) -> Vec<String> {
    let mut args = vec![
        "build".to_string(),
        "--lib".to_string(),
        "--target-dir".to_string(),
        target_dir.display().to_string(),
    ];
    if profile == "dev" {
        // The workspace manifest sets this for the editor's build, but a
        // project outside the workspace is its own root and ignores it.
        // Without it Cargo builds the project's build scripts and proc macros
        // with less debug info than the editor's, which gives the assembly
        // another engine build.
        args.push("--config".to_string());
        args.push("profile.dev.build-override.debug=true".to_string());
    } else {
        args.push("--profile".to_string());
        args.push(profile.to_string());
    }
    args
}

fn assembly_build_target_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|path| assembly_build_target_dir_from_exe(&path))
        .unwrap_or_else(|| PathBuf::from("target"))
}

fn assembly_build_target_dir_from_exe(executable: &Path) -> PathBuf {
    let Some(mut profile_dir) = executable.parent() else {
        return PathBuf::from("target");
    };
    if profile_dir.file_name().is_some_and(|name| name == "deps") {
        profile_dir = profile_dir.parent().unwrap_or(profile_dir);
    } else if profile_dir.file_name().is_some_and(|name| name == "out") {
        // Cargo's newer layout is <profile>/build/<package>/<hash>/out.
        if let Some(build_dir) = profile_dir
            .ancestors()
            .nth(3)
            .filter(|path| path.file_name().is_some_and(|name| name == "build"))
        {
            profile_dir = build_dir.parent().unwrap_or(profile_dir);
        }
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
    target_dir.push(shadow_copy_folder_name(
        project_name,
        std::process::id(),
        timestamp,
    ));
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

/// How long a shadow copy made by another editor process is kept.
///
/// Another editor may have queued a copy it has not loaded yet, which no file
/// lock protects. It loads queued copies within frames, so a copy this old is
/// no longer pending.
const FOREIGN_SHADOW_COPY_GRACE_MS: u128 = 60 * 60 * 1000;

/// The folder [`shadow_copy_assembly`] copies one load into.
fn shadow_copy_folder_name(project_name: &str, process_id: u32, timestamp: u128) -> String {
    format!("{project_name}_p{process_id}_{timestamp}")
}

/// The process and time a shadow-copy folder was made by, read from its name.
#[derive(Debug, PartialEq)]
struct ShadowCopyFolder {
    /// The editor process that made the copy, absent for folders named before
    /// the process id was recorded.
    process_id: Option<u32>,
    /// Milliseconds since the Unix epoch.
    timestamp: u128,
}

impl ShadowCopyFolder {
    fn parse(folder: &Path) -> Option<Self> {
        let name = folder.file_name()?.to_str()?;
        let (rest, timestamp) = name.rsplit_once('_')?;
        if rest.is_empty() {
            return None;
        }
        let timestamp = parse_digits(timestamp)?;
        let process_id = rest.rsplit_once('_').and_then(|(project, process)| {
            let process_id = parse_digits(process.strip_prefix('p')?)?;
            if project.is_empty() {
                return None;
            }
            u32::try_from(process_id).ok()
        });
        Some(Self {
            process_id,
            timestamp,
        })
    }

    /// Whether this copy is unused once `current`, made by `process_id`, is
    /// loaded.
    ///
    /// Only one assembly is loaded per process, so this process's own older
    /// copies are unused. Another process's copy may still be queued for
    /// loading there, so it goes only after [`FOREIGN_SHADOW_COPY_GRACE_MS`].
    fn is_stale(&self, current: &Self, process_id: u32) -> bool {
        if self.timestamp >= current.timestamp {
            return false;
        }
        self.process_id == Some(process_id)
            || current.timestamp - self.timestamp >= FOREIGN_SHADOW_COPY_GRACE_MS
    }
}

fn parse_digits(value: &str) -> Option<u128> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Whether `assembly` was built before `editor`, as told by their modification
/// times; `false` when either time is unknown.
///
/// The assembly links its own copy of the engine and shares engine types with
/// the editor, so an assembly built against an older engine than the editor's
/// can disagree with it on type layouts and trait vtables, which `TypeId`s do
/// not reveal. Cargo relinks the editor whenever the engine changes, so an
/// assembly older than the editor may predate the editor's engine and must be
/// rebuilt before it is loaded.
fn predates(assembly: &Path, editor: &Path) -> bool {
    let modified = |path: &Path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    };
    match (modified(assembly), modified(editor)) {
        (Some(assembly), Some(editor)) => assembly < editor,
        _ => false,
    }
}

/// Whether `assembly` [`predates`] the running editor executable.
fn predates_editor(assembly: &Path) -> bool {
    std::env::current_exe().is_ok_and(|editor| predates(assembly, &editor))
}

/// Records that a successful build checked `assembly` against `editor`'s
/// engine, by moving its modification time to now when it [`predates`] the
/// editor.
///
/// After an editor-only change Cargo finds the assembly fresh and leaves it
/// untouched, older than the relinked editor, so without this every startup
/// would reject the same compatible assembly and build it again.
fn mark_built_for(assembly: &Path, editor: &Path) {
    if !predates(assembly, editor) {
        return;
    }
    let touched = std::fs::File::options()
        .write(true)
        .open(assembly)
        .and_then(|file| file.set_modified(SystemTime::now()));
    if let Err(err) = touched {
        log::warn!(
            "Could not mark project assembly {} as built for this editor: {err}",
            assembly.display()
        );
    }
}

/// Removes the stale shadow copies next to the one `target` was loaded from.
fn remove_stale_shadow_copies_of(target: &Path) {
    if let Some(current) = target.parent() {
        remove_stale_shadow_copies(current, std::process::id());
    }
}

/// Removes every shadow-copy folder next to `current` that
/// [`ShadowCopyFolder::is_stale`] finds unused.
///
/// Newer copies belong to builds not loaded yet and are kept, as is anything
/// that is not a shadow-copy folder. A copy that cannot be removed, such as one
/// another editor still has loaded, is left for a later load.
fn remove_stale_shadow_copies(current: &Path, process_id: u32) {
    let (Some(root), Some(current_copy)) = (current.parent(), ShadowCopyFolder::parse(current))
    else {
        return;
    };
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) => {
            log::warn!(
                "Failed to list project assembly copies in {}: {err}",
                root.display()
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_stale = path.is_dir()
            && ShadowCopyFolder::parse(&path)
                .is_some_and(|copy| copy.is_stale(&current_copy, process_id));
        if !is_stale {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => log::trace!("Removed stale project assembly copy {}", path.display()),
            Err(err) => log::debug!(
                "Could not remove stale project assembly copy {}: {err}",
                path.display()
            ),
        }
    }
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
        return Err(std::io::Error::other(format!(
            "rustc --print {value} failed with status {}",
            output.status
        )));
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
        assembly_artifact_path, assembly_build_target_dir_from_exe, assembly_target_dir,
        cargo_build_args, copy_runtime_dependencies, is_rust_runtime_library, mark_built_for,
        predates, remove_stale_shadow_copies, scene_paths_match, shadow_copy_folder_name,
        AssemblyBuildOutcome, AssemblyBuildQueue, AssemblyLoad, ProjectAssemblyStatus,
        ShadowCopyFolder, FOREIGN_SHADOW_COPY_GRACE_MS,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn an_assembly_built_before_the_editor_predates_it() {
        let dir =
            std::env::temp_dir().join(format!("calyx_assembly_predates_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (assembly, editor) = (dir.join("project.dll"), dir.join("editor.exe"));
        let now = SystemTime::now();
        let touch = |path: &Path, time: SystemTime| {
            fs::write(path, b"").unwrap();
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        };

        touch(&assembly, now - Duration::from_secs(60));
        touch(&editor, now);
        assert!(predates(&assembly, &editor));

        touch(&assembly, now + Duration::from_secs(60));
        assert!(!predates(&assembly, &editor));
        assert!(!predates(&dir.join("missing.dll"), &editor));

        // A build that found the older assembly fresh marks it as built for
        // this editor, so the next startup loads it.
        touch(&assembly, now - Duration::from_secs(60));
        mark_built_for(&assembly, &editor);
        assert!(!predates(&assembly, &editor));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_assembly_status_defaults_to_unloaded() {
        assert!(!ProjectAssemblyStatus::default().is_loaded());
    }

    #[test]
    fn dev_profile_uses_default_cargo_build() {
        assert_eq!(
            cargo_build_args("dev", Path::new("target")),
            [
                "build",
                "--lib",
                "--target-dir",
                "target",
                "--config",
                "profile.dev.build-override.debug=true"
            ]
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
        let target = PathBuf::from("custom-output");
        for executable in [
            "custom-output/debug/editor",
            "custom-output/debug/deps/editor-hash",
            "custom-output/debug/build/editor/hash/out/editor-hash",
            "custom-output/release-with-debug/editor",
            "custom-output/release-with-debug/build/editor/hash/out/editor-hash",
        ] {
            assert_eq!(
                assembly_build_target_dir_from_exe(Path::new(executable)),
                target,
                "incorrect target directory for {executable}"
            );
        }
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

    #[test]
    fn configured_scene_path_matches_the_open_scene_after_canonicalization() {
        let root = std::env::temp_dir().join(format!(
            "calyx-project-manager-path-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let scene = root.join("scene.cxscene");
        fs::write(&scene, "{}").unwrap();

        assert!(scene_paths_match(
            Some(&scene),
            &root.join(".").join("scene.cxscene")
        ));
        assert!(!scene_paths_match(
            Some(&scene),
            &root.join("other.cxscene")
        ));

        fs::remove_dir_all(root).unwrap();
    }

    /// Editor play starts the startup scene a project's `project.toml` names,
    /// for any project: this one is generated on the fly, with no sandbox
    /// paths or constants.
    #[test]
    fn editor_play_runs_the_startup_scene_named_in_project_configuration() {
        use super::ProjectManager;
        use crate::scene_file::write_scene_file;
        use engine::component::ComponentID;
        use engine::context::GameContext;
        use engine::core::Ref;
        use engine::input::Input;
        use engine::test_support::{test_asset_context_with_assets, test_scene};

        let root = std::env::temp_dir().join(format!(
            "calyx-project-startup-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let levels = root.join("assets").join("levels");
        fs::create_dir_all(&levels).unwrap();
        let root = dunce::canonicalize(root).unwrap();
        fs::write(
            root.join("project.toml"),
            "name = \"startup-project\"
             creation_date = \"2026-01-01T00:00:00Z\"
             startup_scene = \"assets/levels/first.cxscene\"
             [runtime.window]
width = 800
height = 600
",
        )
        .unwrap();
        let mut startup = test_scene();
        startup.create(
            Some(ComponentID {
                name: "Configured startup object".into(),
                ..Default::default()
            }),
            None,
        );
        let startup_path = root.join("assets").join("levels").join("first.cxscene");
        write_scene_file(&startup_path, &startup).unwrap();

        let mut game = GameContext::new(test_asset_context_with_assets(vec![root.join("assets")]));
        game.scenes.current_scene_mut().create(
            Some(ComponentID {
                name: "Authoring object".into(),
                ..Default::default()
            }),
            None,
        );
        let project = ProjectManager::new(
            game.assets.clone(),
            &root,
            game.resources.background().clone(),
            Ref::new(ProjectAssemblyStatus::default()),
        )
        .unwrap();
        assert_eq!(
            project.read().runtime_settings().window.non_zero_size(),
            Some((800, 600))
        );

        let started = project.read().start_simulation(&mut game.scenes).unwrap();
        game.scenes.start_simulation();

        assert_eq!(started.as_deref(), Some(startup_path.as_path()));
        assert!(game.scenes.is_simulating());
        let names = |scene: &engine::scene::Scene| {
            scene
                .objects()
                .map(|object| scene.name(object))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(game.scenes.simulation_scene()),
            ["Configured startup object"]
        );
        assert_eq!(names(game.scenes.current_scene()), ["Authoring object"]);

        // The configured scene runs a frame like any other simulation.
        let input_context = egui::Context::default();
        let input = Input::from_ctx(&input_context, None, game.resources.input_mut(), false);
        let assets = game.assets.lock_read();
        game.scenes.prepare();
        game.scenes
            .update(&assets.registries, &mut game.resources, &input);
        drop(assets);
        game.scenes.stop_simulation();
        assert!(!game.scenes.is_simulating());

        drop((project, game));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_shadow_copies_are_removed_and_the_current_one_is_kept() {
        let root = std::env::temp_dir().join(format!(
            "calyx-shadow-cleanup-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let now: u128 = 10_000_000_000;
        let ancient = now - FOREIGN_SHADOW_COPY_GRACE_MS - 1;
        let folder = |project: &str, process_id: u32, timestamp: u128| {
            root.join(shadow_copy_folder_name(project, process_id, timestamp))
        };
        let own_older = folder("sandbox", 7, now - 1_000);
        let own_other_project = folder("other_game", 7, now - 500);
        let current = folder("sandbox", 7, now);
        let own_pending = folder("sandbox", 7, now + 1_000);
        let foreign_recent = folder("sandbox", 8, now - 1_000);
        let foreign_ancient = folder("sandbox", 8, ancient);
        let legacy_recent = root.join(format!("sandbox_{}", now - 1_000));
        let legacy_ancient = root.join(format!("sandbox_{ancient}"));
        let unrelated = root.join("notes");
        for dir in [
            &own_older,
            &own_other_project,
            &current,
            &own_pending,
            &foreign_recent,
            &foreign_ancient,
            &legacy_recent,
            &legacy_ancient,
            &unrelated,
        ] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join(engine::utils::lib_file_name("sandbox")), b"lib").unwrap();
        }

        remove_stale_shadow_copies(&current, 7);

        assert!(!own_older.exists());
        assert!(!own_other_project.exists());
        assert!(current
            .join(engine::utils::lib_file_name("sandbox"))
            .exists());
        assert!(own_pending.exists(), "a newer, not yet loaded copy is kept");
        assert!(
            foreign_recent.exists(),
            "another editor's recent copy may still be queued there"
        );
        assert!(legacy_recent.exists());
        assert!(!foreign_ancient.exists());
        assert!(!legacy_ancient.exists());
        assert!(
            unrelated.exists(),
            "folders that are not shadow copies are kept"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shadow_copy_folder_name_round_trips() {
        assert_eq!(
            ShadowCopyFolder::parse(Path::new(&shadow_copy_folder_name(
                "my_game",
                42,
                1_700_000_000_000
            ))),
            Some(ShadowCopyFolder {
                process_id: Some(42),
                timestamp: 1_700_000_000_000
            })
        );
        assert_eq!(
            ShadowCopyFolder::parse(Path::new("my_game_1700000000000")),
            Some(ShadowCopyFolder {
                process_id: None,
                timestamp: 1_700_000_000_000
            })
        );
        assert_eq!(ShadowCopyFolder::parse(Path::new("my_game")), None);
        assert_eq!(ShadowCopyFolder::parse(Path::new("notes")), None);
        assert_eq!(ShadowCopyFolder::parse(Path::new("_123")), None);
    }

    #[test]
    fn completed_build_is_loaded_once_on_the_polling_thread() {
        let queue = AssemblyBuildQueue::new();
        assert_eq!(queue.next_load(false), None);

        let built = Path::new("built.dll").to_path_buf();
        let reporter = queue.reporter();
        std::thread::spawn(move || reporter.send(AssemblyBuildOutcome::Built(built)).unwrap())
            .join()
            .unwrap();

        assert_eq!(
            queue.next_load(true),
            Some(AssemblyLoad::Snapshot(Path::new("built.dll").to_path_buf()))
        );
        assert_eq!(queue.next_load(true), None);
    }

    #[test]
    fn each_completed_build_is_loaded_once_in_order() {
        let queue = AssemblyBuildQueue::new();
        let reporter = queue.reporter();
        reporter
            .send(AssemblyBuildOutcome::Built(
                Path::new("first.dll").to_path_buf(),
            ))
            .unwrap();
        reporter
            .send(AssemblyBuildOutcome::Built(
                Path::new("second.dll").to_path_buf(),
            ))
            .unwrap();

        assert_eq!(
            queue.next_load(true),
            Some(AssemblyLoad::Snapshot(Path::new("first.dll").to_path_buf()))
        );
        assert_eq!(
            queue.next_load(true),
            Some(AssemblyLoad::Snapshot(
                Path::new("second.dll").to_path_buf()
            ))
        );
        assert_eq!(queue.next_load(true), None);
    }

    #[test]
    fn failed_build_keeps_the_loaded_assembly() {
        let root = std::env::temp_dir().join(format!(
            "calyx-assembly-failed-build-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let existing = root.join(engine::utils::lib_file_name("sandbox"));
        fs::write(&existing, b"sandbox").unwrap();
        let queue = AssemblyBuildQueue::new();

        queue
            .reporter()
            .send(AssemblyBuildOutcome::Failed(existing.clone()))
            .unwrap();
        assert_eq!(queue.next_load(true), None);

        queue
            .reporter()
            .send(AssemblyBuildOutcome::Failed(existing.clone()))
            .unwrap();
        assert_eq!(
            queue.next_load(false),
            Some(AssemblyLoad::Source(existing.clone()))
        );

        queue
            .reporter()
            .send(AssemblyBuildOutcome::Failed(root.join("missing.dll")))
            .unwrap();
        assert_eq!(queue.next_load(false), None);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_build_never_falls_back_to_an_assembly_older_than_the_editor() {
        let root = std::env::temp_dir().join(format!(
            "calyx-assembly-failed-stale-build-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let stale = root.join(engine::utils::lib_file_name("sandbox"));
        fs::write(&stale, b"sandbox").unwrap();
        // Older than the running test binary, which stands in for the editor.
        fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        let queue = AssemblyBuildQueue::new();

        queue
            .reporter()
            .send(AssemblyBuildOutcome::Failed(stale))
            .unwrap();
        assert_eq!(queue.next_load(false), None);

        fs::remove_dir_all(root).unwrap();
    }

    /// Reloads a real project assembly.
    mod assembly_reload {
        use super::super::{
            assembly_artifact_path, assembly_build_target_dir, assembly_profile, build_assembly,
            cargo_build_args, AssemblyBuildOutcome, ProjectAssemblyStatus, ProjectManager,
        };
        use engine::core::Ref;
        use engine::input::Input;
        use engine::scene::Scene;
        use std::path::{Path, PathBuf};
        use uuid::Uuid;

        /// The sandbox's tank controller, a project component with an update hook.
        const TANK_CONTROLLER: &str = "21b467c0-9409-4a7f-b750-418809609eb1";

        /// Builds the sandbox assembly the way the editor does and returns its
        /// library.
        fn build_sandbox_assembly() -> PathBuf {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sandbox");
            let target_dir = assembly_build_target_dir();
            let profile = assembly_profile();
            let source = assembly_artifact_path(&target_dir, profile, "sandbox");
            match build_assembly(
                &root,
                profile,
                &cargo_build_args(profile, &target_dir),
                source,
            ) {
                AssemblyBuildOutcome::Built(source) => source,
                AssemblyBuildOutcome::Failed(source) => {
                    panic!(
                        "the sandbox assembly failed to build at {}",
                        source.display()
                    )
                }
            }
        }

        #[test]
        fn a_reloaded_assembly_leaves_open_scenes_usable() {
            let source = build_sandbox_assembly();
            let mut game = engine::test_support::test_game_context();
            let project = ProjectManager::new(
                game.assets.clone(),
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../sandbox"),
                game.resources.background().clone(),
                Ref::new(ProjectAssemblyStatus::default()),
            )
            .unwrap();
            assert!(project.write().load_assembly(&source, &mut game.scenes));

            let tank_controller = Uuid::parse_str(TANK_CONTROLLER).unwrap();
            let scene = game.scenes.current_scene_mut();
            let tank = scene.create(None, None);
            scene.bind_component_dyn(tank, tank_controller);
            let tank = scene.uuid(tank);
            let cached = game.scenes.current_scene().clone();
            let cached = game
                .assets
                .registries
                .assets
                .read()
                .create("reloaded_scene".into(), cached)
                .unwrap();
            game.scenes.start_simulation();

            // A fresh shadow copy is another library, so loading it unloads the
            // one that created the scenes' project components, as a rebuild does.
            assert!(project.write().load_assembly(&source, &mut game.scenes));

            // Save, play and close the scene.
            let holds_tank = |scene: &Scene| {
                let saved = serde_json::to_value(scene.snapshot()).unwrap();
                saved["components"][tank.to_string()]
                    .get(TANK_CONTROLLER)
                    .is_some()
            };
            assert!(holds_tank(game.scenes.current_scene()));
            assert!(holds_tank(&cached.read()));
            assert!(!game.scenes.has_simulation_scene());
            game.scenes.start_simulation();
            let input_context = egui::Context::default();
            let input = Input::from_ctx(&input_context, None, game.resources.input_mut(), false);
            let assets = game.assets.lock_read();
            game.scenes.prepare();
            game.scenes
                .update(&assets.registries, &mut game.resources, &input);
            game.scenes.stop_simulation();
            game.scenes.load_empty_scene();

            // The cached scene still holds project components, so it must go
            // before the project manager unloads the assembly.
            drop((assets, cached, game));
        }
    }
}
