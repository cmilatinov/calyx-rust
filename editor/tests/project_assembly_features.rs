//! The editor loads the project assembly as a DLL that links its own copy of
//! `engine`. Engine types cross that boundary, and legion keys components by
//! `TypeId`, so the DLL's `engine` and `legion` must be the very builds the
//! editor links. Cargo resolves features and profiles per build, and a crate
//! built with other features, another profile or other dependencies gets a
//! different metadata hash, which changes the hash of every crate above it and
//! every `TypeId` they define. So every compilation unit in engine's
//! dependency graph must be the same whether Cargo builds the editor or the
//! project assembly.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Lists every compilation unit in engine's dependency graph, engine
/// included, when `cargo build <selection>` runs in `dir`. Each unit is its
/// `--unit-graph` entry without its dependency edges, so units that differ in
/// package, features, profile or platform differ.
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
        .filter(|(_, unit)| unit["target"]["name"] == "engine" && unit["mode"] == "build")
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
        .expect("editor sits inside the workspace");
    // The editor's own build: `calyx_ctl launch` and `cargo run -p editor`
    // build the editor alone, a plain `cargo build` builds the workspace.
    let editor = engine_units(root, &["-p", "editor"]);
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
        // What the editor runs in the project to build its assembly.
        (
            "cargo build --lib in sandbox",
            engine_units(&root.join("sandbox"), &["--lib"]),
        ),
    ];

    let mut mismatches = Vec::new();
    for (build, units) in &builds {
        mismatches.extend(
            editor
                .difference(units)
                .map(|unit| format!("only cargo build -p editor builds {}", describe(unit))),
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
         engine/engine_feature_pins/Cargo.toml:\n{}",
        mismatches.join("\n")
    );
}
