//! Where the generator writes, what each asset is called, and its id.
//!
//! ```text
//! <out>/models/<model>.obj            the model, with <model>.meta
//! <out>/models/outline/<model>.obj    its ink hull, when it has one
//! <out>/materials/<map>/<role>.cxmat  each role the models use, per map,
//!                                     with ink when any model has a hull
//! ```
//!
//! An asset's name is its path under the assets folder without extension
//! (`generated/models/crate`), and its id is the version 5 UUID of that
//! name in [`NAMESPACE`], so regenerating keeps every id and the scenes
//! that use them.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

use crate::ids;
use crate::palette::{self, Map};
use crate::params::Model;

/// The namespace of every generated asset's id.
pub const NAMESPACE: Uuid = Uuid::from_u128(0xe6b60a1c_4e82_4a6f_9275_a09402916599);

/// The asset type ids `.meta` files carry.
pub const TYPE_MESH: &str = "792d264b-de6f-4431-b59f-76f18fdb3bfe";
pub const TYPE_MATERIAL: &str = "f98a7f41-84d4-482d-b7af-a670b07035ae";

/// The output folder and the asset-name prefix it maps to.
#[derive(Clone, Debug)]
pub struct Layout {
    pub root: PathBuf,
    /// `root`'s path under its project's `assets` folder, `/`-separated.
    pub prefix: String,
}

/// One asset the generator writes: its file, its name and its id.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub path: PathBuf,
    pub name: String,
    pub id: Uuid,
    pub type_uuid: &'static str,
}

impl Asset {
    /// The `.meta` file beside the asset.
    pub fn meta_path(&self) -> PathBuf {
        self.path.with_extension("meta")
    }

    /// The `.meta` file's contents, as the editor writes them (field order,
    /// pretty-printed, no trailing newline), so loading the asset never
    /// rewrites its meta.
    pub fn meta(&self) -> String {
        #[derive(Serialize)]
        struct Main<'a> {
            id: String,
            name: &'a str,
            display_name: &'a str,
            type_uuid: &'a str,
        }
        #[derive(Serialize)]
        struct Meta<'a> {
            main: Main<'a>,
            inner: Vec<Main<'a>>,
        }
        let meta = Meta {
            main: Main {
                id: self.id.to_string(),
                name: &self.name,
                display_name: self.name.rsplit('/').next().unwrap_or(&self.name),
                type_uuid: self.type_uuid,
            },
            inner: Vec::new(),
        };
        serde_json::to_string_pretty(&meta).expect("metas serialize")
    }
}

/// One model's files.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshPlan {
    pub model: Model,
    pub mesh: Asset,
    pub outline: Option<Asset>,
}

/// One role's material on one map.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialPlan {
    pub map: Map,
    pub role: &'static palette::Role,
    pub asset: Asset,
}

/// Everything one run writes.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub meshes: Vec<MeshPlan>,
    pub materials: Vec<MaterialPlan>,
}

impl Plan {
    /// Every file the run writes, metas included.
    pub fn files(&self) -> BTreeSet<PathBuf> {
        let assets = self
            .meshes
            .iter()
            .flat_map(|m| std::iter::once(&m.mesh).chain(m.outline.as_ref()))
            .chain(self.materials.iter().map(|m| &m.asset));
        assets
            .flat_map(|asset| [asset.path.clone(), asset.meta_path()])
            .collect()
    }
}

impl Layout {
    /// The layout of `root`, which must lie inside a project's `assets`
    /// folder so the editor loads what it writes.
    pub fn new(root: &Path) -> Result<Layout, String> {
        // Resolve `..` lexically, so every spelling of a folder gives the
        // same prefix and so the same ids.
        let mut parts: Vec<String> = Vec::new();
        for component in root.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
                Component::ParentDir => {
                    parts.pop();
                }
                _ => {}
            }
        }
        let after_assets = parts
            .iter()
            .rposition(|part| part == "assets")
            .map(|i| &parts[i + 1..])
            .filter(|rest| !rest.is_empty())
            .ok_or_else(|| {
                format!(
                    "{} is not a folder inside a project's assets folder",
                    root.display()
                )
            })?;
        Ok(Layout {
            root: root.to_path_buf(),
            prefix: after_assets.join("/"),
        })
    }

    fn asset(&self, relative: &[&str], extension: &str, type_uuid: &'static str) -> Asset {
        let mut path = self.root.clone();
        for part in relative {
            path.push(part);
        }
        path.set_extension(extension);
        let name = format!("{}/{}", self.prefix, relative.join("/"));
        Asset {
            path,
            id: ids::uuid_v5(&NAMESPACE, name.as_bytes()),
            name,
            type_uuid,
        }
    }

    /// The material of `role` on `map`.
    pub fn material(&self, map: Map, role: &'static palette::Role) -> MaterialPlan {
        MaterialPlan {
            map,
            role,
            asset: self.asset(
                &["materials", map.folder(), role.name],
                "cxmat",
                TYPE_MATERIAL,
            ),
        }
    }

    /// What generating `models` writes. Materials come per map, in palette
    /// order, for each role a model uses, and ink when a model has a hull.
    pub fn plan(&self, models: &[Model]) -> Plan {
        let meshes: Vec<MeshPlan> = models
            .iter()
            .map(|model| MeshPlan {
                model: model.clone(),
                mesh: self.asset(&["models", &model.name], "obj", TYPE_MESH),
                outline: model
                    .outline
                    .map(|_| self.asset(&["models", "outline", &model.name], "obj", TYPE_MESH)),
            })
            .collect();
        let used = |role: &palette::Role| {
            models.iter().any(|m| m.role == role.name)
                || (role.name == palette::INK && models.iter().any(|m| m.outline.is_some()))
        };
        let materials = Map::ALL
            .iter()
            .flat_map(|map| {
                palette::ROLES
                    .iter()
                    .filter(|role| used(role))
                    .map(|role| self.material(*map, role))
            })
            .collect();
        Plan { meshes, materials }
    }

    /// The folders the generator owns: it removes files in them that a run
    /// no longer writes.
    pub fn owned_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![
            self.root.join("models"),
            self.root.join("models").join("outline"),
        ];
        dirs.extend(
            Map::ALL
                .iter()
                .map(|map| self.root.join("materials").join(map.folder())),
        );
        dirs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prefix_is_the_path_under_assets() {
        let layout = Layout::new(Path::new("sandbox/assets/generated")).unwrap();
        assert_eq!(layout.prefix, "generated");
        let nested = Layout::new(Path::new("/x/assets/a/assets/props/clay")).unwrap();
        assert_eq!(nested.prefix, "props/clay");
        assert!(Layout::new(Path::new("sandbox/generated")).is_err());
        assert!(Layout::new(Path::new("sandbox/assets")).is_err());
    }

    #[test]
    fn parent_components_resolve_before_the_prefix_is_taken() {
        for spelling in [
            "sandbox/assets/generated/../generated",
            "sandbox/assets/./generated",
            "sandbox/assets/x/../generated/",
        ] {
            let layout = Layout::new(Path::new(spelling)).unwrap();
            assert_eq!(layout.prefix, "generated", "{spelling}");
        }
        assert!(Layout::new(Path::new("sandbox/assets/generated/..")).is_err());
        assert!(Layout::new(Path::new("sandbox/assets/../generated")).is_err());
    }

    #[test]
    fn a_meta_is_written_as_the_editor_writes_it() {
        let layout = Layout::new(Path::new("assets/generated")).unwrap();
        let crate_box = layout.asset(&["models", "crate"], "obj", TYPE_MESH);
        assert_eq!(
            crate_box.meta(),
            r#"{
  "main": {
    "id": "ab200de6-0ee5-55ab-885c-66964e1f6681",
    "name": "generated/models/crate",
    "display_name": "crate",
    "type_uuid": "792d264b-de6f-4431-b59f-76f18fdb3bfe"
  },
  "inner": []
}"#
        );
    }
}
