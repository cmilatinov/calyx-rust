//! Reading a `.unitypackage`: a gzipped tar holding, per asset, a folder
//! named after the asset's GUID with `pathname` (its path in the Unity
//! project), `asset` (its bytes) and `asset.meta` (its import settings).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use crate::yaml::{self, Document};

/// The assets of a `.unitypackage`, keyed by GUID.
#[derive(Default)]
pub struct UnityPackage {
    /// Each asset's path in the Unity project, such as
    /// `Assets/Pack/Models/rock.fbx`.
    pub paths: HashMap<String, String>,
    /// Each asset's bytes. Folders have none.
    pub assets: HashMap<String, Vec<u8>>,
    /// Each asset's `.meta` text.
    pub metas: HashMap<String, String>,
    /// The GUID at each path.
    pub guids: HashMap<String, String>,
}

impl UnityPackage {
    /// Reads the package at `path`.
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path)
            .map_err(|err| format!("cannot open {}: {err}", path.display()))?;
        Self::from_reader(file).map_err(|err| format!("cannot read {}: {err}", path.display()))
    }

    /// Reads a package from its gzipped tar bytes.
    pub fn from_reader(reader: impl Read) -> std::io::Result<Self> {
        let mut tar = Vec::new();
        flate2::read::GzDecoder::new(reader).read_to_end(&mut tar)?;
        let mut package = UnityPackage::default();
        for (name, data) in tar_files(&tar)? {
            let name = name.replace('\\', "/");
            let name = name.trim_start_matches("./");
            let Some((guid, leaf)) = name.split_once('/') else {
                continue;
            };
            let guid = guid.to_string();
            match leaf {
                "pathname" => {
                    let text = String::from_utf8_lossy(data);
                    let path = text.lines().next().unwrap_or("").trim().to_string();
                    package.paths.insert(guid, path);
                }
                "asset" => {
                    package.assets.insert(guid, data.to_vec());
                }
                "asset.meta" => {
                    package
                        .metas
                        .insert(guid, String::from_utf8_lossy(data).into_owned());
                }
                _ => {}
            }
        }
        package.guids = package
            .paths
            .iter()
            .map(|(guid, path)| (path.clone(), guid.clone()))
            .collect();
        Ok(package)
    }

    /// `(path, guid)` of every asset with bytes whose path ends with
    /// `extension` (any case), sorted by path.
    pub fn with_extension(&self, extension: &str) -> Vec<(String, String)> {
        let extension = extension.to_ascii_lowercase();
        let mut found: Vec<(String, String)> = self
            .paths
            .iter()
            .filter(|(guid, path)| {
                path.to_ascii_lowercase().ends_with(&extension) && self.assets.contains_key(*guid)
            })
            .map(|(guid, path)| (path.clone(), guid.clone()))
            .collect();
        found.sort();
        found
    }

    /// The documents of the YAML asset `guid`.
    pub fn documents(&self, guid: &str) -> Result<Vec<Document>, String> {
        let bytes = self
            .assets
            .get(guid)
            .ok_or_else(|| format!("the package has no asset {guid}"))?;
        Ok(yaml::parse_documents(&String::from_utf8_lossy(bytes)))
    }

    /// The path of asset `guid`.
    pub fn path(&self, guid: &str) -> Result<&str, String> {
        self.paths
            .get(guid)
            .map(String::as_str)
            .ok_or_else(|| format!("the package has no asset {guid}"))
    }
}

/// The regular files of a tar archive, as `(name, bytes)`: ustar and GNU
/// headers, with ustar name prefixes and GNU long names. Other entries,
/// such as folders and pax headers, are skipped.
fn tar_files(tar: &[u8]) -> std::io::Result<Vec<(String, &[u8])>> {
    let invalid =
        |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_string());
    let field = |header: &[u8], start: usize, len: usize| {
        let bytes = &header[start..start + len];
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(len);
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    };
    let mut files = Vec::new();
    let mut long_name: Option<String> = None;
    let mut offset = 0;
    while offset + 512 <= tar.len() {
        let header = &tar[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let size_text = field(header, 124, 12);
        let size = usize::from_str_radix(size_text.trim(), 8)
            .map_err(|_| invalid("bad tar entry size"))?;
        let data_start = offset + 512;
        let data = tar
            .get(data_start..data_start + size)
            .ok_or_else(|| invalid("truncated tar entry"))?;
        let mut name = field(header, 0, 100);
        if &header[257..262] == b"ustar" {
            let prefix = field(header, 345, 155);
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        }
        match header[156] {
            b'L' => {
                let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
                long_name = Some(String::from_utf8_lossy(&data[..end]).into_owned());
            }
            b'0' | 0 => files.push((long_name.take().unwrap_or(name), data)),
            _ => long_name = None,
        }
        offset = data_start + size.div_ceil(512) * 512;
    }
    Ok(files)
}

/// The file name of `path` without its extension.
pub fn stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

#[cfg(test)]
pub(crate) mod tests;
