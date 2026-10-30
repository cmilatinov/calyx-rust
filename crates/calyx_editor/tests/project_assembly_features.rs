//! The editor loads the project assembly as a DLL that links its own copy of
//! `calyx_engine`. Engine types cross that boundary, and legion keys components by
//! `TypeId`, so the DLL's `calyx_engine` and `legion` must be the very builds the
//! editor links. Cargo resolves features and profiles per build, and a crate
//! built with other features, another profile or other dependencies gets a
//! different metadata hash, which changes the hash of every crate above it and
//! every `TypeId` they define. So every compilation unit in engine's
//! dependency graph must be the same whether Cargo builds the editor or the
//! project assembly.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Lists every compilation unit in engine's dependency graph, engine
/// included, when `cargo build <selection>` runs in `dir`. Each unit is its
/// `--unit-graph` entry without its dependency edges or profile name, so units
/// that differ in package, features, profile settings or platform differ.
fn engine_units(dir: &Path, selection: &[&str]) -> BTreeSet<String> {
    let output = Command::new(env!("CARGO"))
        .current_dir(dir)
        .args(["build", "--unit-graph", "-Z", "unstable-options"])
        .args(selection)
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo build --unit-graph failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let graph: Value = serde_json::from_slice(&output.stdout).expect("the unit graph is JSON");
    let units = graph["units"]
        .as_array()
        .expect("the unit graph lists units");

    let mut pending: Vec<usize> = units
        .iter()
        .enumerate()
        .filter(|(_, unit)| unit["target"]["name"] == "calyx_engine" && unit["mode"] == "build")
        .map(|(index, _)| index)
        .collect();
    assert!(!pending.is_empty(), "{} builds engine", dir.display());
    let mut visited = BTreeSet::new();
    let mut found = BTreeSet::new();
    while let Some(index) = pending.pop() {
        if !visited.insert(index) {
            continue;
        }
        let mut unit = units[index].clone();
        let dependencies = unit
            .as_object_mut()
            .and_then(|unit| unit.remove("dependencies"))
            .unwrap_or_default();
        // A test build names its profile `test`, but builds engine with the
        // same settings, and the name does not reach the metadata hash.
        if let Some(profile) = unit["profile"].as_object_mut() {
            profile.remove("name");
        }
        pending.extend(
            dependencies
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|dependency| dependency["index"].as_u64())
                .map(|index| index as usize),
        );
        found.insert(unit.to_string());
    }
    found
}

/// Lists engine's units when the editor builds the project assembly in `dir`,
/// with the editor's own cargo arguments.
fn assembly_units(dir: &Path) -> BTreeSet<String> {
    let args = calyx_editor::project_assembly_build_args("dev", Path::new("target"));
    let selection: Vec<&str> = args
        .iter()
        .skip_while(|arg| *arg == "build")
        .map(String::as_str)
        .collect();
    engine_units(dir, &selection)
}

/// Writes a project outside the workspace, the way a user project depends on
/// engine: its own workspace root, so none of this workspace's manifest
/// settings apply to it. It keeps this workspace's lockfile so it resolves the
/// same versions.
fn external_project(root: &Path) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("external_assembly_project");
    fs::create_dir_all(dir.join("src")).expect("creates the external project");
    let engine = root
        .join("crates/calyx_engine")
        .display()
        .to_string()
        .replace('\\', "/");
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "external_assembly_project"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
calyx_engine = {{ path = "{engine}" }}

[workspace]
"#
        ),
    )
    .expect("writes the external manifest");
    fs::write(dir.join("src/lib.rs"), "").expect("writes the external library");
    fs::copy(root.join("Cargo.lock"), dir.join("Cargo.lock")).expect("copies the lockfile");
    dir
}

/// Names a unit for the failure message.
fn describe(unit: &str) -> String {
    let unit: Value = serde_json::from_str(unit).expect("units are JSON");
    let package = unit["pkg_id"].as_str().unwrap_or_default();
    let package = package.rsplit('#').next().unwrap_or(package);
    format!(
        "{package} {} {} features {} profile {}",
        unit["target"]["name"], unit["mode"], unit["features"], unit["profile"]
    )
}

#[test]
fn project_assembly_links_the_editors_engine_build() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the editor sits inside the workspace's crates folder");
    // The editor's own build: `calyx_ctl launch` and `cargo run -p calyx_editor`
    // build the editor alone, a plain `cargo build` builds the workspace.
    let editor = engine_units(root, &["-p", "calyx_editor"]);
    assert!(
        editor
            .iter()
            .any(|unit| unit.contains("\"name\":\"legion\"")),
        "engine's dependency graph holds legion"
    );
    let builds = [
        (
            "cargo build --workspace",
            engine_units(root, &["--workspace"]),
        ),
        // `cargo test --workspace`, which builds dev-dependencies too. The
        // editor's own tests load the sandbox assembly, and reflection
        // downcasts engine types by `TypeId`, so they need its engine build.
        (
            "cargo test --workspace",
            engine_units(root, &["--workspace", "--tests"]),
        ),
        // What the editor runs in the project to build its assembly.
        (
            "the sandbox assembly build",
            assembly_units(&root.join("sandbox")),
        ),
        (
            "an external project's assembly build",
            assembly_units(&external_project(root)),
        ),
    ];

    let mut mismatches = Vec::new();
    for (build, units) in &builds {
        mismatches.extend(
            editor
                .difference(units)
                .map(|unit| format!("only cargo build -p calyx_editor builds {}", describe(unit))),
        );
        mismatches.extend(
            units
                .difference(&editor)
                .map(|unit| format!("only {build} builds {}", describe(unit))),
        );
    }
    assert!(
        mismatches.is_empty(),
        "Cargo builds engine's dependencies differently for the editor than for \
         the project assembly, so the assembly links another engine build whose \
         TypeIds the editor does not know. Pin the editor's features, or make a \
         build-only dependency a normal one, in \
         crates/calyx_engine_feature_pins/Cargo.toml:\n{}",
        mismatches.join("\n")
    );
}
