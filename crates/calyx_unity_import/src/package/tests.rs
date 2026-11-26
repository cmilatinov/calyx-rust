use super::*;

/// A `.unitypackage` holding `entries` of `(guid, pathname, asset, meta)`.
pub(crate) fn package_bytes(entries: &[(&str, &str, Option<&str>, Option<&str>)]) -> Vec<u8> {
    let mut tar = Vec::new();
    let mut add = |name: String, data: &[u8]| {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..107].copy_from_slice(b"0000644");
        header[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|b| *b as u32).sum();
        header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        tar.extend_from_slice(&header);
        tar.extend_from_slice(data);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    };
    for (guid, pathname, asset, meta) in entries {
        add(
            format!("{guid}/pathname"),
            format!("{pathname}\n00\n").as_bytes(),
        );
        if let Some(asset) = asset {
            add(format!("{guid}/asset"), asset.as_bytes());
        }
        if let Some(meta) = meta {
            add(format!("{guid}/asset.meta"), meta.as_bytes());
        }
    }
    tar.extend_from_slice(&[0u8; 1024]);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut encoder, &tar).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn assets_are_keyed_by_guid_and_found_by_path() {
    let bytes = package_bytes(&[
        (
            "aaa",
            "Assets/Pack/Models/rock.FBX",
            Some("fbx"),
            Some("guid: aaa\n"),
        ),
        ("bbb", "Assets/Pack/Models/barrel.fbx", Some("fbx"), None),
        (
            "ccc",
            "Assets/Pack/Models",
            None,
            Some("folderAsset: yes\n"),
        ),
        ("ddd", "Assets/Pack/Materials/wood.mat", Some("mat"), None),
    ]);
    let package = UnityPackage::from_reader(bytes.as_slice()).unwrap();

    assert_eq!(package.paths["aaa"], "Assets/Pack/Models/rock.FBX");
    assert_eq!(package.guids["Assets/Pack/Materials/wood.mat"], "ddd");
    assert_eq!(package.metas["aaa"], "guid: aaa\n");
    assert!(!package.assets.contains_key("ccc"), "a folder has no asset");
    assert_eq!(
        package.with_extension(".fbx"),
        vec![
            (
                "Assets/Pack/Models/barrel.fbx".to_string(),
                "bbb".to_string()
            ),
            ("Assets/Pack/Models/rock.FBX".to_string(), "aaa".to_string()),
        ]
    );
}

#[test]
fn stems_drop_the_folder_and_extension() {
    assert_eq!(stem("Assets/Pack/Models/rock_01.fbx"), "rock_01");
    assert_eq!(stem("scene.unity"), "scene");
}
