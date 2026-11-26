use super::*;
use crate::assets::material::Material;
use crate::assets::Asset;
use crate::render::AssetRenderState;
use crate::test_support::TestAssets;
use uuid::Uuid;

#[test]
fn prunes_material_bind_group_cache_to_live_materials() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let asset_registry = read_only_context.registries.assets.read();
    let shader = asset_registry
        .load::<Shader>("shaders/mesh/pbr")
        .expect("missing pbr shader");
    let default_texture = asset_registry
        .white_texture()
        .expect("missing default material texture");
    drop(asset_registry);

    let material = Material::from_shader(&read_only_context, shader);
    let key = material.bind_group_cache_key(&read_only_context, default_texture);
    let live_id = Uuid::new_v4();
    let stale_id = Uuid::new_v4();
    let mut renderer = MeshRenderer::new(&read_only_context).expect("mesh renderer should load");
    renderer.material_bind_group_cache.insert(
        live_id,
        CachedMaterialBindGroups {
            key: key.clone(),
            groups: Default::default(),
        },
    );
    renderer.material_bind_group_cache.insert(
        stale_id,
        CachedMaterialBindGroups {
            key,
            groups: Default::default(),
        },
    );

    renderer.prune_material_bind_group_cache(&HashSet::from([live_id]));

    assert!(renderer.material_bind_group_cache.contains_key(&live_id));
    assert!(!renderer.material_bind_group_cache.contains_key(&stale_id));
}

/// The bind groups `renderer` builds for `material` this frame.
fn material_bind_groups(
    renderer: &mut MeshRenderer,
    context: &ReadOnlyAssetContext,
    material: &Ref<Material>,
    default_texture: &Ref<Texture>,
) -> HashMap<u32, wgpu::BindGroup> {
    let mut state = AssetRenderState::default();
    state.materials.insert(material.id(), material.clone());
    let shader = material.read().shader.get_ref(&context.registries);
    let shader = shader.expect("the material's shader is loaded");
    state.shaders.insert(shader.id(), shader);
    material
        .read()
        .collect_textures(context, &mut state.textures, default_texture.clone());
    let locked = state.lock();
    let mut groups = renderer.build_material_bind_groups(
        context.render_context.device(),
        context,
        default_texture.clone(),
        &locked,
    );
    groups
        .remove(&material.id())
        .expect("the material has bind groups")
}

#[test]
fn material_bind_groups_are_rebuilt_only_when_the_material_buffers_are_replaced() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let asset_registry = read_only_context.registries.assets.read();
    let shader = asset_registry
        .load::<Shader>("shaders/mesh/pbr")
        .expect("missing pbr shader");
    let default_texture = asset_registry
        .white_texture()
        .expect("missing default material texture");
    drop(asset_registry);
    let material = Ref::new(Material::from_shader(&read_only_context, shader.clone()));
    assert!(
        !material.read().buffers.is_empty(),
        "the pbr material binds a uniform buffer"
    );
    let mut renderer = MeshRenderer::new(&read_only_context).expect("mesh renderer should load");

    let first = material_bind_groups(
        &mut renderer,
        &read_only_context,
        &material,
        &default_texture,
    );
    assert!(!first.is_empty(), "the material binds at least one group");
    let unchanged = material_bind_groups(
        &mut renderer,
        &read_only_context,
        &material,
        &default_texture,
    );
    assert_eq!(
        unchanged, first,
        "an unchanged material reuses its cached bind groups"
    );

    // A hot reload replaces the material in place under the same id: the
    // same shader, so buffers of the same sizes, but new buffers.
    *material.write() = Material::from_shader(&read_only_context, shader);
    let reloaded = material_bind_groups(
        &mut renderer,
        &read_only_context,
        &material,
        &default_texture,
    );
    for (group, bind_group) in &first {
        assert_ne!(
            reloaded.get(group),
            Some(bind_group),
            "group {group} still binds the buffers the reload replaced"
        );
    }
}

#[test]
fn object_id_shader_builds_pipeline_with_skinning_path() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let shader = Shader::from_file(
        &read_only_context,
        &assets.engine_root().join("shaders/mesh/object_id.wgsl"),
    )
    .expect("object id shader should load")
    .asset;
    let options = PipelineOptions::builder()
        .samples(1)
        .fragment_targets(vec![Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::R32Uint,
            blend: None,
            write_mask: wgpu::ColorWrites::RED,
        })])
        .build();

    let pipeline = shader.pipeline(&options);

    assert_eq!(shader.pipeline(&options), pipeline);
    assert_eq!(shader.pipeline_count(), 1);
}
