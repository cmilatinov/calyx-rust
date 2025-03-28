use egui_wgpu::wgpu;

use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::skybox::Skybox;
use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::core::Ref;
use crate::render::asset_map::AssetMap;
use crate::render::Shader;
use std::collections::HashMap;
use std::sync::RwLockReadGuard;

#[derive(Default)]
pub(crate) struct AssetRenderState {
    pub meshes: AssetMap<Mesh>,
    pub materials: AssetMap<Material>,
    pub textures: AssetMap<Texture>,
    pub shaders: AssetMap<Shader>,
    pub skyboxes: AssetMap<Skybox>,
}

#[allow(unused)]
impl AssetRenderState {
    pub fn lock(&self, device: &wgpu::Device) -> LockedAssetRenderState {
        let meshes = self.meshes.lock_read();
        LockedAssetRenderState {
            mesh_instance_groups: meshes
                .iter()
                .map(|(id, m)| (*id, m.instance_bind_group(device)))
                .collect(),
            meshes,
            materials: self.materials.lock_read(),
            textures: self.textures.lock_read(),
            shaders: self.shaders.lock_read(),
            skyboxes: self.skyboxes.lock_read(),
        }
    }
    pub fn mesh(&self, id: AssetId) -> &Ref<Mesh> {
        self.meshes.get(id)
    }
    pub fn material(&self, id: AssetId) -> &Ref<Material> {
        self.materials.get(id)
    }
    pub fn shader(&self, id: AssetId) -> &Ref<Shader> {
        self.shaders.get(id)
    }
    pub fn texture(&self, id: AssetId) -> &Ref<Texture> {
        self.textures.get(id)
    }
    pub fn skybox(&self, id: AssetId) -> &Ref<Skybox> {
        self.skyboxes.get(id)
    }
}

pub(crate) struct LockedAssetRenderState<'a> {
    pub meshes: HashMap<AssetId, RwLockReadGuard<'a, Mesh>>,
    pub mesh_instance_groups: HashMap<AssetId, wgpu::BindGroup>,
    pub materials: HashMap<AssetId, RwLockReadGuard<'a, Material>>,
    pub textures: HashMap<AssetId, RwLockReadGuard<'a, Texture>>,
    pub shaders: HashMap<AssetId, RwLockReadGuard<'a, Shader>>,
    pub skyboxes: HashMap<AssetId, RwLockReadGuard<'a, Skybox>>,
}

#[allow(unused)]
impl LockedAssetRenderState<'_> {
    pub fn mesh(&self, id: AssetId) -> &RwLockReadGuard<Mesh> {
        self.meshes.get(&id).unwrap()
    }
    pub fn mesh_instance_group(&self, id: AssetId) -> &wgpu::BindGroup {
        self.mesh_instance_groups.get(&id).unwrap()
    }
    pub fn material(&self, id: AssetId) -> &RwLockReadGuard<Material> {
        self.materials.get(&id).unwrap()
    }
    pub fn texture(&self, id: AssetId) -> &RwLockReadGuard<Texture> {
        self.textures.get(&id).unwrap()
    }
    pub fn shader(&self, id: AssetId) -> &RwLockReadGuard<Shader> {
        self.shaders.get(&id).unwrap()
    }
    pub fn skybox(&self, id: AssetId) -> &RwLockReadGuard<Skybox> {
        self.skyboxes.get(&id).unwrap()
    }
}
