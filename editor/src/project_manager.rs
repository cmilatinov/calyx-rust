use sharedlib::{Lib, Symbol};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub struct ProjectManager {
    current_project: Project,
    cargo_profile: String,
    target_profile_dir: String,
    runtime_target_dir: PathBuf,
    engine_fingerprint: Option<SystemTime>,
    loaded_assembly_path: Option<PathBuf>,
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
        let (cargo_profile, target_profile_dir, runtime_target_dir) = Self::infer_build_profile();
        let engine_fingerprint = Self::engine_fingerprint(current_project.root_directory());
        Ok(Ref::new_cyclic(move |weak| Self {
            current_project,
            cargo_profile,
            target_profile_dir,
            runtime_target_dir,
            engine_fingerprint,
            loaded_assembly_path: None,
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

    fn infer_build_profile() -> (String, String, PathBuf) {
        let current_exe = env::current_exe().ok();
        let profile_dir = current_exe.as_ref().and_then(|path| path.parent());
        let profile = current_exe
            .as_ref()
            .and_then(|_| profile_dir)
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

        let runtime_target_dir = profile_dir
            .and_then(|path| path.parent())
            .map(|path| path.join("editor-runtime"))
            .unwrap_or_else(|| PathBuf::from("target").join("editor-runtime"));

        (cargo_profile, profile, runtime_target_dir)
    }

    fn newest_modified_at(path: &Path) -> Option<SystemTime> {
        let metadata = fs::metadata(path).ok()?;
        if metadata.is_file() {
            return metadata.modified().ok();
        }

        let mut newest = metadata.modified().ok();
        let entries = fs::read_dir(path).ok()?;
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if let Some(modified) = Self::newest_modified_at(&entry_path) {
                newest = Some(match newest {
                    Some(current) => current.max(modified),
                    None => modified,
                });
            }
        }
        newest
    }

    fn engine_fingerprint(project_root: &Path) -> Option<SystemTime> {
        let workspace_root = project_root.parent()?;
        let watch_paths = [
            workspace_root.join("Cargo.toml"),
            workspace_root.join("Cargo.lock"),
            workspace_root.join(".cargo"),
            workspace_root.join("engine"),
        ];

        let mut newest: Option<SystemTime> = None;
        for path in watch_paths {
            let Some(modified) = Self::newest_modified_at(&path) else {
                continue;
            };
            newest = Some(match newest {
                Some(current) => current.max(modified),
                None => modified,
            });
        }
        newest
    }

    fn engine_changed_since_start(&self) -> bool {
        Self::engine_fingerprint(self.current_project.root_directory()) != self.engine_fingerprint
    }

    fn ensure_dll_search_path(dir: &Path) {
        #[cfg(windows)]
        {
            let Ok(current_path) = env::var("PATH") else {
                return;
            };
            let mut paths: Vec<PathBuf> = env::split_paths(&current_path).collect();
            if paths.iter().any(|path| path == dir) {
                return;
            }
            paths.insert(0, dir.to_path_buf());
            if let Ok(updated_path) = env::join_paths(paths) {
                env::set_var("PATH", updated_path);
            }
        }
    }

    fn stage_assembly(build_artifact: &Path) -> Result<PathBuf, BoxedError> {
        let output_dir = build_artifact.parent().ok_or("Missing assembly output directory")?;
        let staged_dir = output_dir.join("loaded");
        fs::create_dir_all(&staged_dir).map_err(Box::new)?;

        let stem = build_artifact
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("Missing assembly file stem")?;
        let extension = build_artifact
            .extension()
            .and_then(|value| value.to_str())
            .ok_or("Missing assembly file extension")?;
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(Box::new)?
            .as_nanos();
        let staged_path = staged_dir.join(format!("{stem}-{unique}.{extension}"));
        fs::copy(build_artifact, &staged_path).map_err(Box::new)?;

        let pdb_path = build_artifact.with_extension("pdb");
        if pdb_path.exists() {
            let staged_pdb = staged_path.with_extension("pdb");
            let _ = fs::copy(pdb_path, staged_pdb);
        }

        Ok(staged_path)
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
        if self.engine_changed_since_start() {
            return self.background.write().execute(TaskId::Build, move || {
                eprintln!(
                    "Engine files changed while the editor is running. Restart the editor before rebuilding the sandbox plugin."
                );
            });
        }

        let root = self.root_project_dir();
        let package = self.current_project().name().clone();
        let profile = self.cargo_profile.clone();
        let runtime_target_dir = self.runtime_target_dir.clone();
        let project_manager_ref = self.project_manager.upgrade().unwrap();
        self.background.write().execute(TaskId::Build, move || {
            // std::thread::sleep(Duration::from_secs(10));
            let mut build = Command::new("cargo")
                .current_dir(root)
                .env("CARGO_TARGET_DIR", &runtime_target_dir)
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
        let mut build_artifact = self.runtime_target_dir.clone();
        build_artifact.push(&self.target_profile_dir);
        Self::ensure_dll_search_path(&build_artifact);
        build_artifact.push(engine::utils::lib_file_name(
            self.current_project().name().as_str(),
        ));
        let staged_path = match Self::stage_assembly(&build_artifact) {
            Ok(path) => path,
            Err(err) => {
                eprintln!("{err}");
                return;
            }
        };
        unsafe {
            match Lib::new(&staged_path) {
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
                    let previous_path = self.loaded_assembly_path.replace(staged_path);
                    let previous_lib = self.assembly.replace(lib);
                    drop(previous_lib);
                    if let Some(previous_path) = previous_path {
                        let _ = fs::remove_file(&previous_path);
                        let _ = fs::remove_file(previous_path.with_extension("pdb"));
                    }
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
