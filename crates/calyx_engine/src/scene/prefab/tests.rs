use super::*;
use crate::assets::material::{MaterialTexture, ShaderVariableValue};
use crate::assets::texture::TextureColorSpace;
use crate::assets::AssetAccess;
use crate::test_support::test_game_context;
use egui_wgpu::wgpu;

#[test]
fn from_file_returns_error_for_path_without_extension() {
    let context = test_game_context().assets.lock_read();
    let path = std::env::temp_dir().join(format!("prefab-{}", Uuid::new_v4()));

    let Err(err) = Prefab::from_file(&context, &path) else {
        panic!("path without extension should fail");
    };

    assert_eq!(err.asset_type, Some(Prefab::asset_name()));
    assert_eq!(err.path.as_deref(), Some(path.as_path()));
}

#[test]
fn from_file_returns_error_for_unregistered_path() {
    let context = test_game_context().assets.lock_read();
    let path = std::env::temp_dir().join(format!("prefab-{}.fbx", Uuid::new_v4()));

    let Err(err) = Prefab::from_file(&context, &path) else {
        panic!("path without asset metadata should fail");
    };

    assert_eq!(err.asset_type, Some(Prefab::asset_name()));
    assert_eq!(err.path.as_deref(), Some(path.as_path()));
}

#[test]
fn default_material_id_returns_error_when_missing() {
    let context = test_game_context().assets.lock_read();
    let registry = context.registries.assets.read();

    let Err(err) = default_material_id(&registry) else {
        panic!("missing default material should fail");
    };

    assert_eq!(err.kind, AssetError::NotFound.kind);
}

#[test]
fn imported_sub_asset_names_are_unique() {
    let context = test_game_context().assets.lock_read();
    let registry = context.registries.assets.read();
    let parent = AssetMeta {
        id: Uuid::new_v4(),
        name: format!("models/{}", Uuid::new_v4()),
        display_name: String::new(),
        type_uuid: Prefab::type_uuid(),
        parent: None,
        children: Vec::new(),
        path: None,
    };
    let mut names = SubAssetNames::new(&registry, &parent, ["Tracks", "mesh"]);
    let mesh = Mesh::type_uuid();

    assert_eq!(names.allocate("Tracks", "mesh", mesh), "Tracks");
    assert_eq!(names.allocate("Tracks", "mesh", mesh), "Tracks_1");
    assert_eq!(names.allocate("", "mesh", mesh), "mesh");
    assert_eq!(
        names.allocate("Tracks", "texture", Texture::type_uuid()),
        "Tracks_2",
        "every type shares one namespace"
    );
}

fn node(name: &str, children: Vec<ImportedNode>) -> ImportedNode {
    ImportedNode {
        name: name.to_string(),
        transform: Mat4::identity(),
        meshes: Vec::new(),
        children,
    }
}

#[test]
fn nodes_with_shared_or_empty_names_stay_distinct_and_stable() {
    let root = node(
        "RootNode",
        vec![
            node("Wheel", Vec::new()),
            node("Wheel", Vec::new()),
            node("", vec![node("", Vec::new())]),
        ],
    );

    let (data, root_id) = Prefab::scene_data(&HashSet::new(), &[], &root);
    let (again, again_root_id) = Prefab::scene_data(&HashSet::new(), &[], &root);

    assert_eq!(data.components.len(), 5);
    assert_eq!(data.hierarchy[&root_id].len(), 3);
    let unnamed = data.hierarchy[&root_id][2];
    assert_eq!(data.hierarchy[&unnamed].len(), 1);
    assert_eq!(again_root_id, root_id);
    assert_eq!(again.hierarchy, data.hierarchy);
}

#[test]
fn malformed_model_file_returns_load_error_naming_the_file() {
    let assets = crate::test_support::TestAssets::models();
    let files: [(&str, &[u8]); 3] = [
        ("broken.fbx", b"Kaydara FBX Binary  \0 not really"),
        (
            "broken.gltf",
            br#"{"asset": {"version": "2.0"}, "nodes": ["#,
        ),
        ("broken.glb", b"glTF\x02\0\0\0\x18\0\0\0not really"),
    ];
    for (name, contents) in files {
        std::fs::write(assets.fixtures_root().join(name), contents).unwrap();
    }
    let context = assets.asset_context();
    let context = context.lock_read();

    for (name, _) in files {
        let path = assets.fixtures_root().join(name);
        let Err(err) = Prefab::from_file(&context, &path) else {
            panic!("malformed {name} should fail to import");
        };

        assert_eq!(err.kind, AssetError::LoadError.kind, "{name}");
        assert_eq!(err.asset_type, Some(Prefab::asset_name()), "{name}");
        assert_eq!(err.path.as_deref(), Some(path.as_path()), "{name}");
        assert!(err.source_message.is_some(), "{name}");
    }
}

/// Imports the sandbox tank, whose left and right parts share node names.
fn import_tank() -> Prefab {
    let assets = crate::test_support::TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let path = assets.sandbox_root().join("game/models/Tank_002.fbx");
    Prefab::from_file(&context, &path)
        .unwrap_or_else(|e| panic!("failed to import the tank: {e}"))
        .asset
}

#[test]
fn tank_parts_sharing_a_name_import_as_distinct_objects() {
    let tank = import_tank();
    let children = tank.data.hierarchy.values().flatten().collect::<Vec<_>>();
    let distinct = children.iter().collect::<HashSet<_>>();

    assert_eq!(distinct.len(), children.len(), "a node id appears twice");
    assert_eq!(tank.data.components.len(), children.len() + 1);
    let again = import_tank();
    assert_eq!(again.root, tank.root);
    assert_eq!(again.data.hierarchy, tank.data.hierarchy);
}

/// Name of each object holding a mesh, and whether the mesh is skinned, sorted by name.
fn mesh_objects(prefab: &Prefab) -> Vec<(String, bool)> {
    let mut objects = prefab
        .data
        .components
        .values()
        .filter_map(|components| {
            let name = components[&ComponentID::type_uuid()]["name"]
                .as_str()?
                .to_string();
            if components.contains_key(&ComponentSkinnedMesh::type_uuid()) {
                Some((name, true))
            } else if components.contains_key(&ComponentMesh::type_uuid()) {
                Some((name, false))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    objects.sort();
    objects
}

#[test]
fn every_part_of_a_shared_mesh_is_attached_with_its_node_skin() {
    // One mesh of two primitives, used by a skinned node and by an unskinned node.
    let assets = crate::test_support::TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let path = assets.fixtures_root().join("gltf/SharedMesh.gltf");
    let prefab = Prefab::from_file(&context, &path)
        .unwrap_or_else(|e| panic!("failed to import the shared mesh: {e}"))
        .asset;

    assert_eq!(
        mesh_objects(&prefab),
        [
            ("Skinned".to_string(), true),
            ("Skinned_1".to_string(), true),
            ("Static".to_string(), false),
            ("Static_1".to_string(), false),
        ]
    );
}

/// The material fixture's canonical asset name.
const MATERIALS_MODEL: &str = "gltf/Materials";

/// The id of the sub-asset `local_name` of the material fixture, as the registry names it.
fn materials_sub_asset(registry: &AssetRegistry, local_name: &str) -> Uuid {
    registry
        .asset_id(&format!("{MATERIALS_MODEL}/{local_name}"))
        .unwrap_or_else(|| panic!("the fixture has no sub-asset {local_name}"))
}

/// The material on the mesh component of the prefab object named `name`.
fn material_of(prefab: &Prefab, name: &str) -> Uuid {
    let components = prefab
        .data
        .components
        .values()
        .find(|components| components[&ComponentID::type_uuid()]["name"] == name)
        .unwrap_or_else(|| panic!("the prefab has no object {name}"));
    let mesh = &components[&ComponentMesh::type_uuid()];
    Uuid::parse_str(mesh["material"].as_str().unwrap()).unwrap()
}

fn texture_slot(material: &Material, name: &str) -> Option<Uuid> {
    let variable = material
        .variables
        .iter()
        .find(|variable| variable.name == name)
        .unwrap_or_else(|| panic!("the material has no {name}"));
    match &variable.value {
        ShaderVariableValue::Texture2D(MaterialTexture::Asset(texture)) => Some(texture.id()),
        ShaderVariableValue::Texture2D(MaterialTexture::Color(_)) => None,
        _ => panic!("{name} is not a texture"),
    }
}

fn float_value(material: &Material, name: &str) -> f32 {
    match material
        .variables
        .iter()
        .find(|variable| variable.name == name)
        .map(|variable| &variable.value)
    {
        Some(ShaderVariableValue::Float(value)) => *value,
        _ => panic!("{name} is not a float"),
    }
}

fn color_value(material: &Material, name: &str) -> [f32; 4] {
    match material
        .variables
        .iter()
        .find(|variable| variable.name == name)
        .map(|variable| &variable.value)
    {
        Some(ShaderVariableValue::Color(value)) => *value,
        Some(ShaderVariableValue::Texture2D(MaterialTexture::Color(value))) => *value,
        _ => panic!("{name} is not a color"),
    }
}

#[test]
fn gltf_materials_import_as_sub_assets_on_the_pbr_inputs() {
    let assets = crate::test_support::TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    let path = assets.fixtures_root().join("gltf/Materials.gltf");
    let loaded = Prefab::from_file(&context, &path)
        .unwrap_or_else(|e| panic!("failed to import the material fixture: {e}"));

    let painted_id = materials_sub_asset(&registry, "Painted");
    let painted = registry.load_by_id::<Material>(painted_id).unwrap();
    let painted = painted.read();
    let base_color = materials_sub_asset(&registry, "image_0");
    let orm = materials_sub_asset(&registry, "Materials_orm");
    let emissive = materials_sub_asset(&registry, "Emissive");
    assert_eq!(
        color_value(&painted, "base_color_factor"),
        [0.5, 0.25, 1.0, 1.0]
    );
    assert_eq!(
        texture_slot(&painted, "base_color_texture"),
        Some(base_color)
    );
    assert_eq!(float_value(&painted, "metallic"), 0.25);
    assert_eq!(float_value(&painted, "roughness"), 0.75);
    assert_eq!(
        texture_slot(&painted, "metallic_roughness_texture"),
        Some(orm)
    );
    assert_eq!(float_value(&painted, "normal_scale"), 0.5);
    assert_eq!(
        texture_slot(&painted, "normal_texture"),
        None,
        "the missing normal image leaves the slot's default"
    );
    assert_eq!(
        color_value(&painted, "normal_texture"),
        [0.5, 0.5, 1.0, 1.0]
    );
    assert_eq!(
        color_value(&painted, "emission_color"),
        [1.0, 0.5, 0.0, 1.0]
    );
    assert_eq!(float_value(&painted, "emission_strength"), 1.0);
    assert_eq!(texture_slot(&painted, "emissive_texture"), Some(emissive));
    assert_eq!(texture_slot(&painted, "occlusion_texture"), Some(orm));
    assert_eq!(float_value(&painted, "occlusion_strength"), 0.8);
    assert_eq!(float_value(&painted, "ambient_occlusion"), 1.0);
    assert_eq!(float_value(&painted, "alpha_cutoff"), 0.0, "opaque");
    assert_eq!(
        painted.shader.id(),
        registry.asset_id(IMPORTED_MATERIAL_SHADER_NAME).unwrap()
    );

    let masked = registry
        .load_by_id::<Material>(materials_sub_asset(&registry, "material_1"))
        .unwrap();
    let masked = masked.read();
    assert_eq!(float_value(&masked, "alpha_cutoff"), 0.3);
    assert_eq!(
        texture_slot(&masked, "base_color_texture"),
        Some(materials_sub_asset(&registry, "image_0_1")),
        "the base color image with another sampler is its own texture"
    );
    assert_eq!(color_value(&masked, "base_color_factor"), [1.0; 4]);
    assert_eq!(float_value(&masked, "roughness"), 1.0);

    let unused = registry
        .load_by_id::<Material>(materials_sub_asset(&registry, "Unused"))
        .unwrap();
    assert_eq!(
        color_value(&unused.read(), "base_color_factor"),
        [0.1, 0.2, 0.3, 1.0]
    );

    // One texture per image and sampler: the ORM image serves two slots once, and the
    // missing image makes none.
    let textures = loaded
        .sub_assets
        .iter()
        .filter(|id| {
            registry.asset_meta_from_id(**id).map(|meta| meta.type_uuid)
                == Some(Texture::type_uuid())
        })
        .count();
    assert_eq!(textures, 4);
}

#[test]
fn imported_textures_read_color_slots_as_srgb_and_data_slots_as_linear() {
    let assets = crate::test_support::TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    let path = assets.fixtures_root().join("gltf/Materials.gltf");
    Prefab::from_file(&context, &path).unwrap();

    let painted = registry
        .load_by_id::<Material>(materials_sub_asset(&registry, "Painted"))
        .unwrap();
    let painted = painted.read();
    let slot = |name: &str| {
        painted
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .map(|variable| painted.texture_color_space(variable.group, variable.binding))
            .unwrap()
    };
    assert_eq!(slot("base_color_texture"), TextureColorSpace::Srgb);
    assert_eq!(slot("emissive_texture"), TextureColorSpace::Srgb);
    assert_eq!(
        slot("metallic_roughness_texture"),
        TextureColorSpace::Linear
    );
    assert_eq!(slot("occlusion_texture"), TextureColorSpace::Linear);
    assert_eq!(slot("normal_texture"), TextureColorSpace::Linear);

    for name in ["image_0", "Emissive", "Materials_orm"] {
        let texture = registry
            .load_by_id::<Texture>(materials_sub_asset(&registry, name))
            .unwrap();
        let texture = texture.read();
        assert_eq!(
            texture.descriptor.format,
            wgpu::TextureFormat::Rgba8Unorm,
            "{name} is stored as read"
        );
        assert_eq!(
            texture.srgb_texture.as_ref().map(wgpu::Texture::format),
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            "{name} has an sRGB copy for color slots"
        );
    }
}

#[test]
fn imported_sub_asset_ids_are_stable_across_reimports_and_listed_in_the_meta() {
    let assets = crate::test_support::TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    let path = assets.fixtures_root().join("gltf/Materials.gltf");

    let first = registry.load_by_path::<Prefab>(&path).unwrap();
    let first_ids = registry.asset_meta_from_path(&path).unwrap().children;
    let first_root = first.read().root;
    registry.reload_by_path::<Prefab>(&path).unwrap();
    let again_ids = registry.asset_meta_from_path(&path).unwrap().children;

    assert_eq!(again_ids, first_ids);
    assert_eq!(first.read().root, first_root);
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path.with_extension("meta")).unwrap())
            .unwrap();
    let listed = meta["inner"]
        .as_array()
        .unwrap()
        .iter()
        .map(|child| {
            (
                child["name"].as_str().unwrap().to_string(),
                Uuid::parse_str(child["id"].as_str().unwrap()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let expected = [
        ("Panel", Mesh::type_uuid()),
        ("Panel_1", Mesh::type_uuid()),
        ("Panel_2", Mesh::type_uuid()),
        ("Painted", Material::type_uuid()),
        ("material_1", Material::type_uuid()),
        ("Unused", Material::type_uuid()),
        ("image_0", Texture::type_uuid()),
        ("Materials_orm", Texture::type_uuid()),
        ("Emissive", Texture::type_uuid()),
        ("image_0_1", Texture::type_uuid()),
    ];
    assert_eq!(
        listed,
        expected
            .iter()
            .map(|(name, _)| (
                name.to_string(),
                utils::uuid_from_str(&format!("{MATERIALS_MODEL}/{name}"))
            ))
            .collect::<Vec<_>>(),
        "each sub-asset is listed with the id hashed from its name"
    );
    for ((name, type_uuid), id) in expected.iter().zip(&first_ids) {
        assert_eq!(
            registry.asset_meta_from_id(*id).map(|meta| meta.type_uuid),
            Some(*type_uuid),
            "{name}"
        );
    }
    assert_eq!(
        first_ids,
        listed.iter().map(|(_, id)| *id).collect::<Vec<_>>()
    );
}

#[test]
fn a_sub_asset_keeps_its_name_when_another_type_frees_the_one_it_was_suffixed_from() {
    let assets = crate::test_support::TestAssets::models();
    let path = assets.fixtures_root().join("gltf/Materials.gltf");
    let source = std::fs::read_to_string(&path).unwrap();
    let painted = "\"name\": \"Painted\"";
    assert!(source.contains(painted));
    // The material takes the image's name, so the texture becomes `Emissive_1`.
    std::fs::write(&path, source.replace(painted, "\"name\": \"Emissive\"")).unwrap();
    let texture_id = {
        let context = assets.asset_context();
        let context = context.lock_read();
        let registry = context.registries.assets.read();
        registry.load_by_path::<Prefab>(&path).unwrap();
        let material = materials_sub_asset(&registry, "Emissive");
        let texture = materials_sub_asset(&registry, "Emissive_1");
        assert_eq!(
            registry.asset_meta_from_id(material).unwrap().type_uuid,
            Material::type_uuid()
        );
        assert_eq!(
            registry.asset_meta_from_id(texture).unwrap().type_uuid,
            Texture::type_uuid()
        );

        // Renaming the material frees `Emissive`, which the registry still holds as a
        // material.
        std::fs::write(&path, &source).unwrap();
        registry.reload_by_path::<Prefab>(&path).unwrap();
        assert_eq!(materials_sub_asset(&registry, "Emissive_1"), texture);
        texture
    };

    // A fresh registry reads the rewritten `.meta` and keeps the texture's id.
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    registry.load_by_path::<Prefab>(&path).unwrap();
    assert_eq!(materials_sub_asset(&registry, "Emissive_1"), texture_id);
    assert!(registry
        .asset_meta_from_path(&path)
        .unwrap()
        .children
        .contains(&texture_id));
}

#[test]
fn a_new_source_name_does_not_take_the_suffixed_name_of_an_existing_sub_asset() {
    let assets = crate::test_support::TestAssets::models();
    let path = assets.fixtures_root().join("gltf/Materials.gltf");
    let source = std::fs::read_to_string(&path).unwrap();
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    registry.load_by_path::<Prefab>(&path).unwrap();
    let masked = materials_sub_asset(&registry, "material_1");

    // The first material is renamed to the stem of the unnamed second one's name.
    std::fs::write(
        &path,
        source.replace("\"name\": \"Painted\"", "\"name\": \"material\""),
    )
    .unwrap();
    registry.reload_by_path::<Prefab>(&path).unwrap();

    assert_eq!(
        materials_sub_asset(&registry, "material_1"),
        masked,
        "the unnamed material keeps its name and id"
    );
    let masked = registry.load_by_id::<Material>(masked).unwrap();
    assert_eq!(float_value(&masked.read(), "alpha_cutoff"), 0.3);
    let renamed = registry
        .load_by_id::<Material>(materials_sub_asset(&registry, "material"))
        .unwrap();
    assert_eq!(float_value(&renamed.read(), "alpha_cutoff"), 0.0);
}

#[test]
fn each_primitive_s_mesh_component_takes_its_imported_material() {
    let assets = crate::test_support::TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    let path = assets.fixtures_root().join("gltf/Materials.gltf");
    let prefab = Prefab::from_file(&context, &path).unwrap().asset;

    assert_eq!(
        material_of(&prefab, "Panel"),
        materials_sub_asset(&registry, "Painted")
    );
    assert_eq!(
        material_of(&prefab, "Panel_1"),
        materials_sub_asset(&registry, "material_1"),
        "a further primitive's child object takes its own material"
    );
    assert_eq!(
        material_of(&prefab, "Panel_2"),
        default_material_id(&registry).unwrap(),
        "a primitive without a material keeps the default"
    );
}

#[test]
fn a_material_assigned_to_an_instance_survives_a_reimport() {
    let assets = crate::test_support::TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let registry = context.registries.assets.read();
    let path = assets.fixtures_root().join("gltf/Materials.gltf");
    let prefab_ref = registry.load_by_path::<Prefab>(&path).unwrap();
    let prefab_id = prefab_ref.id();
    let mut level = context.scene();
    let (instance, _) = level
        .instantiate_linked_prefab(prefab_id, &prefab_ref.read(), None)
        .unwrap();
    let object = |level: &crate::scene::Scene, name: &str| {
        level
            .objects()
            .find(|object| level.name(*object) == name)
            .unwrap()
    };
    let material = |level: &crate::scene::Scene, name: &str| {
        level
            .read_component::<ComponentMesh, _, _>(object(level, name), |mesh| mesh.material.id())
            .unwrap()
    };
    let unused = materials_sub_asset(&registry, "Unused");
    let panel = object(&level, "Panel");
    level
        .write_component::<ComponentMesh, _>(panel, |mesh| {
            mesh.material = crate::assets::AssetRef::from_id(unused);
        })
        .unwrap();

    registry.reload_by_path::<Prefab>(&path).unwrap();
    let update = level
        .update_prefab_instances(prefab_id, &prefab_ref.read())
        .unwrap();

    assert_eq!(update.instances, 1);
    assert_eq!(update.kept_overrides, 1);
    assert_eq!(level.prefab_instances(prefab_id), [instance]);
    assert_eq!(material(&level, "Panel"), unused, "the override is kept");
    assert_eq!(
        material(&level, "Panel_1"),
        materials_sub_asset(&registry, "material_1"),
        "a mesh without an override keeps the imported material"
    );
}

#[test]
fn every_material_part_of_an_fbx_node_is_attached() {
    // One cube node whose top face uses a second material.
    let assets = crate::test_support::TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let path = assets.fixtures_root().join("fbx/TwoMaterials.fbx");
    let prefab = Prefab::from_file(&context, &path)
        .unwrap_or_else(|e| panic!("failed to import the two material cube: {e}"))
        .asset;

    assert_eq!(
        mesh_objects(&prefab),
        [("Cube".to_string(), false), ("Cube_1".to_string(), false)]
    );
}
