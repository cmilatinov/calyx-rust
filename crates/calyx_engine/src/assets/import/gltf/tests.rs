use super::*;

#[test]
fn every_node_gets_a_distinct_name_and_given_names_win() {
    let names = unique_names(
        [
            Some("joint"),
            Some("joint"),
            None,
            Some(""),
            Some("joint_1"),
            Some("node_2"),
        ]
        .into_iter(),
    );

    assert_eq!(
        names,
        [
            "joint",
            "joint_1_1",
            "node_2_2",
            "node_3",
            "joint_1",
            "node_2"
        ]
    );
}

#[test]
fn materials_textures_and_samplers_are_read_from_every_image_source() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/models/gltf/Materials.gltf");
    let scene = import(&path).expect("the material fixture imports");

    let names = scene
        .materials
        .iter()
        .map(|material| material.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Painted", "material_1", "Unused"]);
    let painted = &scene.materials[0];
    assert_eq!(painted.alpha_cutoff, None);
    assert_eq!(painted.normal_texture, None, "the missing image is skipped");
    assert_eq!(
        painted.metallic_roughness_texture, painted.occlusion_texture,
        "one texture serves both slots of the ORM image"
    );
    assert_eq!(scene.materials[1].alpha_cutoff, Some(0.3));
    assert_eq!(
        scene
            .meshes
            .iter()
            .map(|mesh| mesh.material)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), None]
    );

    let textures = scene
        .textures
        .iter()
        .map(|texture| {
            (
                texture.name.as_str(),
                (texture.image.width(), texture.image.height()),
            )
        })
        .collect::<Vec<_>>();
    // The buffer view image twice, with two samplers, then the external and data URI
    // images.
    assert_eq!(
        textures,
        [
            ("image_0", (2, 2)),
            ("Materials_orm", (2, 2)),
            ("Emissive", (1, 1)),
            ("image_0", (2, 2)),
        ]
    );
    assert!(std::sync::Arc::ptr_eq(
        &scene.textures[0].image,
        &scene.textures[3].image
    ));
    assert_eq!(scene.textures[0].sampler, ImportedSampler::default());
    assert_eq!(
        scene.textures[3].sampler,
        ImportedSampler {
            wrap_u: TextureWrap::MirroredRepeat,
            wrap_v: TextureWrap::ClampToEdge,
            mag_filter: TextureFilter::Nearest,
            min_filter: TextureFilter::Nearest,
            mipmap_filter: Some(TextureFilter::Linear),
        }
    );
}

#[test]
fn external_images_are_read_from_the_glb_json_chunk() {
    let json = br#"{"asset":{"version":"2.0"},"images":[
            {"uri":"textures/base%20color.png"},
            {"uri":"data:image/png;base64,AAAA"},
            {"bufferView":0,"mimeType":"image/png"}],
            "buffers":[{"byteLength":4}],"bufferViews":[{"buffer":0,"byteLength":4}]}  "#;
    let binary = [0u8; 4];
    let length = 12 + 8 + json.len() + 8 + binary.len();
    let mut glb = Vec::new();
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(length as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(json);
    glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&binary);
    let root = std::env::temp_dir().join(format!("calyx-glb-images-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("the folder is created");
    let path = root.join("model.glb");
    std::fs::write(&path, glb).expect("the GLB is written");

    let images = external_image_paths(&path);

    std::fs::remove_dir_all(&root).expect("the folder is removed");
    assert_eq!(images, [root.join("textures/base color.png")]);
}

#[test]
fn step_keys_hold_each_value_until_the_next_key() {
    let keys = step_keys(&[0.0, 1.0, 2.0], &[[1.0; 4], [2.0; 4], [3.0; 4]]);
    assert_eq!(
        keys,
        vec![
            (0.0, [1.0; 4]),
            (1.0, [1.0; 4]),
            (1.0, [2.0; 4]),
            (2.0, [2.0; 4]),
            (2.0, [3.0; 4]),
        ]
    );
}

#[test]
fn cubic_spline_is_resampled_at_thirty_hertz_through_its_keys() {
    // One component from 0 to 1 over one second, with zero tangents: a smoothstep.
    let zero = [0.0; 4];
    let values = [zero, [0.0; 4], zero, zero, [1.0; 4], zero];
    let keys = resample_cubic_spline(&[0.0, 1.0], &values);

    assert_eq!(keys.len(), 31);
    assert_eq!(keys[0], (0.0, [0.0; 4]));
    assert_eq!(keys[30], (1.0, [1.0; 4]));
    let (time, value) = keys[15];
    assert!((time - 0.5).abs() < 1e-9);
    assert!((value[0] - 0.5).abs() < 1e-6);
    let (time, value) = keys[6];
    let s = time as f32;
    assert!((value[0] - (3.0 * s * s - 2.0 * s * s * s)).abs() < 1e-6);
}

#[test]
fn cubic_spline_follows_its_tangents() {
    // Constant value with a unit out-tangent leaving the first key bends upwards.
    let values = [[0.0; 4], [0.0; 4], [1.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]];
    let keys = resample_cubic_spline(&[0.0, 1.0], &values);
    let (time, value) = keys[15];
    let s = time as f32;
    assert!((value[0] - (s * s * s - 2.0 * s * s + s)).abs() < 1e-6);
    assert!(value[0] > 0.0);
}
