//! Imports Unity Asset Store packages (`.unitypackage`) into a Calyx
//! project: models, textures and materials into a vendor folder the
//! project's licence rules can keep out of git, and a pack's demo scene into
//! a Calyx scene.
//!
//! The import is general: `.unitypackage` reading ([`package`]), Unity's
//! YAML ([`yaml`]), material conversion ([`material`]), prefab and variant
//! resolution ([`prefab`]), scene conversion ([`scene`]), and the FBX and
//! TGA conversion through headless Blender ([`blender`]). Everything one
//! pack needs on top (which scene, where its files go, how the scene is lit
//! and framed in Calyx) lives in a [`packs`] module.

pub mod blender;
pub mod ids;
pub mod json;
pub mod material;
pub mod package;
pub mod packs;
pub mod prefab;
pub mod scene;
pub mod yaml;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::blender::{MeshJob, TextureJob, NORMALS_IMPORT};
use crate::json::Json;
use crate::material::UnityMaterial;
use crate::package::{stem, UnityPackage};
use crate::packs::Pack;

pub const TYPE_MESH: &str = "792d264b-de6f-4431-b59f-76f18fdb3bfe";
pub const TYPE_TEXTURE: &str = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2";
pub const TYPE_MATERIAL: &str = "f98a7f41-84d4-482d-b7af-a670b07035ae";
pub const TYPE_SCENE: &str = "9946a2e7-e022-447e-8e60-528da548087f";

/// Where a pack's converted assets go and what they are called.
pub struct Layout<'a> {
    pub pack: &'a Pack,
    /// The folder the pack's scene goes in, holding `vendor/`.
    pub root: PathBuf,
    /// Where the import's scratch folder goes, outside every asset root.
    pub work_root: PathBuf,
}

impl Layout<'_> {
    pub fn vendor(&self) -> PathBuf {
        self.root.join("vendor")
    }

    /// The asset id of `name`, derived so it stays the same across imports.
    pub fn asset_id(&self, name: &str) -> String {
        ids::uuid_v5(&self.pack.namespace, name.as_bytes()).to_string()
    }

    fn vendor_name(&self, kind: &str, path: &str) -> String {
        format!("{}/vendor/{kind}/{}", self.pack.asset_prefix, stem(path))
    }

    pub fn mesh_name(&self, path: &str) -> String {
        self.vendor_name("models", path)
    }

    pub fn material_name(&self, path: &str) -> String {
        self.vendor_name("materials", path)
    }

    pub fn texture_name(&self, path: &str) -> String {
        self.vendor_name("textures", path)
    }

    /// Writes `<path_no_ext>.meta` for the asset `name` and returns its id.
    pub fn write_meta(
        &self,
        path_no_ext: &Path,
        name: &str,
        type_uuid: &str,
    ) -> Result<String, String> {
        let id = self.asset_id(name);
        let meta = Json::object([
            (
                "main",
                Json::object([
                    ("id", Json::Str(id.clone())),
                    ("name", Json::Str(name.to_string())),
                    (
                        "display_name",
                        Json::Str(name.rsplit('/').next().unwrap_or(name).to_string()),
                    ),
                    ("type_uuid", Json::Str(type_uuid.to_string())),
                ]),
            ),
            ("inner", Json::Array(Vec::new())),
        ]);
        write(&with_extension(path_no_ext, "meta"), &meta.pretty())?;
        Ok(id)
    }
}

fn with_extension(path_no_ext: &Path, extension: &str) -> PathBuf {
    let mut path = path_no_ext.as_os_str().to_owned();
    path.push(".");
    path.push(extension);
    PathBuf::from(path)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

/// Converts the package's models, textures and materials into the vendor
/// folder, replacing what an earlier import left there.
///
/// Files are rewritten in place and only the ones this import no longer
/// writes are removed afterwards, so an open editor sees assets change
/// rather than disappear and come back.
pub fn import_assets(
    package: &UnityPackage,
    layout: &Layout,
    blender: &Path,
) -> Result<String, String> {
    let vendor = layout.vendor();
    let subdirs = ["models", "textures", "materials"];
    let before = files_in(&vendor, &subdirs);
    for sub in subdirs {
        let dir = vendor.join(sub);
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    }
    write(
        &vendor.join("README.md"),
        &format!(
            "Built by `cargo run --release -p calyx_unity_import` from {}.\n\
             The pack's licence does not allow these files in git; this folder is ignored.\n",
            layout.pack.title
        ),
    )?;

    let work = layout
        .work_root
        .join(format!(".unity-import-{}", std::process::id()));
    std::fs::create_dir_all(&work)
        .map_err(|err| format!("cannot create {}: {err}", work.display()))?;
    let result = convert_models_and_textures(package, layout, blender, &work);
    let _ = std::fs::remove_dir_all(&work);
    let (models, textures) = result?;

    let materials = package.with_extension(".mat");
    for (path, guid) in &materials {
        let body = package
            .documents(guid)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("{path} is empty"))?
            .body;
        let material = UnityMaterial::from_body(&body);
        let texture = match &material.texture {
            Some(texture) => Some(layout.asset_id(&layout.texture_name(package.path(texture)?))),
            None => None,
        };
        let target = vendor.join("materials").join(stem(path));
        write(
            &with_extension(&target, "cxmat"),
            &material.to_calyx(texture.as_deref()).pretty(),
        )?;
        layout.write_meta(&target, &layout.material_name(path), TYPE_MATERIAL)?;
    }
    let written = written_files(package, layout);
    for stale in before.difference(&written) {
        let _ = std::fs::remove_file(stale);
    }
    Ok(format!(
        "imported {models} models, {textures} textures and {} materials into {}",
        materials.len(),
        vendor.display()
    ))
}

/// The files directly in `subdirs` of `root`.
fn files_in(root: &Path, subdirs: &[&str]) -> std::collections::HashSet<PathBuf> {
    subdirs
        .iter()
        .filter_map(|sub| std::fs::read_dir(root.join(sub)).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect()
}

/// Every file an import of `package` writes in the vendor folder's subfolders.
fn written_files(package: &UnityPackage, layout: &Layout) -> std::collections::HashSet<PathBuf> {
    let vendor = layout.vendor();
    let mut files = std::collections::HashSet::new();
    let kinds: [(&str, &str, &[&str]); 3] = [
        (".fbx", "models", &["obj", "meta"]),
        (".tga", "textures", &["png", "meta"]),
        (".mat", "materials", &["cxmat", "meta"]),
    ];
    for (extension, sub, outputs) in kinds {
        for (path, _) in package.with_extension(extension) {
            let base = vendor.join(sub).join(stem(&path));
            for output in outputs {
                files.insert(with_extension(&base, output));
            }
        }
    }
    files
}

fn convert_models_and_textures(
    package: &UnityPackage,
    layout: &Layout,
    blender: &Path,
    work: &Path,
) -> Result<(usize, usize), String> {
    let vendor = layout.vendor();
    // Each meta goes beside its file once the file exists, so an open editor
    // never sees a meta without its asset.
    let mut metas = Vec::new();
    let mut meshes = Vec::new();
    for (path, guid) in package.with_extension(".fbx") {
        let fbx = work.join(format!("{guid}.fbx"));
        write_bytes(&fbx, &package.assets[&guid])?;
        let obj = vendor.join("models").join(stem(&path));
        let (normal_mode, smooth_angle) =
            blender::normal_settings(package.metas.get(&guid).map(String::as_str).unwrap_or(""));
        meshes.push(MeshJob {
            fbx,
            obj: with_extension(&obj, "obj"),
            normal_mode,
            smooth_angle,
        });
        metas.push((obj, layout.mesh_name(&path), TYPE_MESH));
    }
    let mut textures = Vec::new();
    for (path, guid) in package.with_extension(".tga") {
        let tga = work.join(format!("{guid}.tga"));
        write_bytes(&tga, &package.assets[&guid])?;
        let png = vendor.join("textures").join(stem(&path));
        textures.push(TextureJob {
            tga,
            png: with_extension(&png, "png"),
            fog: (layout.pack.texture_fog)(package, &guid)?,
        });
        metas.push((png, layout.texture_name(&path), TYPE_TEXTURE));
    }
    blender::convert(blender, work, &meshes, &textures)?;
    for mesh in &meshes {
        if mesh.normal_mode != NORMALS_IMPORT && mesh.smooth_angle == 0.0 {
            let text = std::fs::read_to_string(&mesh.obj)
                .map_err(|err| format!("cannot read {}: {err}", mesh.obj.display()))?;
            blender::check_faceted(&text)
                .map_err(|err| format!("{}: {err}", mesh.obj.display()))?;
        }
    }
    for (path, name, type_uuid) in metas {
        layout.write_meta(&path, &name, type_uuid)?;
    }
    Ok((meshes.len(), textures.len()))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

/// Converts the pack's demo scene to `<root>/<scene name>.cxscene`.
pub fn build_scene(package: &UnityPackage, layout: &Layout) -> Result<String, String> {
    let pack = layout.pack;
    let scene_guid = package
        .guids
        .get(pack.demo_scene)
        .ok_or_else(|| format!("the package has no {}", pack.demo_scene))?;
    let meshes: HashMap<String, String> = package
        .with_extension(".fbx")
        .into_iter()
        .map(|(path, guid)| (guid, layout.asset_id(&layout.mesh_name(&path))))
        .collect();
    let materials: HashMap<String, String> = package
        .with_extension(".mat")
        .into_iter()
        .map(|(path, guid)| (guid, layout.asset_id(&layout.material_name(&path))))
        .collect();
    let ids = scene::AssetIds {
        meshes: &meshes,
        materials: &materials,
        default_material: pack.default_material,
    };
    let mut writer = scene::SceneWriter::new(pack.namespace);
    let converted = scene::convert(package, scene_guid, &ids, &mut writer)?;
    let extra = (pack.finish_scene)(&mut writer, &converted)?;
    let scene_path = layout.root.join(pack.scene_name);
    write(
        &with_extension(&scene_path, "cxscene"),
        &writer.to_json().pretty(),
    )?;
    layout.write_meta(
        &scene_path,
        &format!("{}/{}", pack.asset_prefix, pack.scene_name),
        TYPE_SCENE,
    )?;
    Ok(format!(
        "wrote {}: {} prefab instances, {} meshes, {} groups, {} cameras{extra}",
        with_extension(&scene_path, "cxscene").display(),
        converted.prefab_instances,
        converted.meshes,
        converted.groups,
        converted.cameras.len()
    ))
}
