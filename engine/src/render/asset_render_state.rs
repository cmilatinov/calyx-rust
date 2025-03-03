use egui_wgpu::wgpu;

use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::skybox::Skybox;
use crate::assets::texture::Texture;
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
    pub fn mesh(&self, id: usize) -> &Ref<Mesh> {
        self.meshes.get(id)
    }
    pub fn material(&self, id: usize) -> &Ref<Material> {
        self.materials.get(id)
    }
    pub fn shader(&self, id: usize) -> &Ref<Shader> {
        self.shaders.get(id)
    }
    pub fn texture(&self, id: usize) -> &Ref<Texture> {
        self.textures.get(id)
    }
    pub fn skybox(&self, id: usize) -> &Ref<Skybox> {
        self.skyboxes.get(id)
    }
}

pub(crate) struct LockedAssetRenderState<'a> {
    pub meshes: HashMap<usize, RwLockReadGuard<'a, Mesh>>,
    pub mesh_instance_groups: HashMap<usize, wgpu::BindGroup>,
    pub materials: HashMap<usize, RwLockReadGuard<'a, Material>>,
    pub textures: HashMap<usize, RwLockReadGuard<'a, Texture>>,
    pub shaders: HashMap<usize, RwLockReadGuard<'a, Shader>>,
    pub skyboxes: HashMap<usize, RwLockReadGuard<'a, Skybox>>,
}

#[allow(unused)]
impl LockedAssetRenderState<'_> {
    pub fn mesh(&self, id: usize) -> &RwLockReadGuard<Mesh> {
        self.meshes.get(&id).unwrap()
    }
    pub fn mesh_instance_group(&self, id: usize) -> &wgpu::BindGroup {
        self.mesh_instance_groups.get(&id).unwrap()
    }
    pub fn material(&self, id: usize) -> &RwLockReadGuard<Material> {
        self.materials.get(&id).unwrap()
    }
    pub fn texture(&self, id: usize) -> &RwLockReadGuard<Texture> {
        self.textures.get(&id).unwrap()
    }
    pub fn shader(&self, id: usize) -> &RwLockReadGuard<Shader> {
        self.shaders.get(&id).unwrap()
    }
    pub fn skybox(&self, id: usize) -> &RwLockReadGuard<Skybox> {
        self.skyboxes.get(&id).unwrap()
    }
}
