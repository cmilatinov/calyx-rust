use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture2D;
use crate::core::Ref;
use crate::render::asset_map::AssetMap;
use crate::render::Shader;
use std::collections::HashMap;
use std::sync::RwLockReadGuard;

#[derive(Default)]
pub(crate) struct AssetRenderState {
    pub meshes: AssetMap<Mesh>,
    pub materials: AssetMap<Material>,
    pub textures: AssetMap<Texture2D>,
    pub shaders: AssetMap<Shader>,
}

#[allow(unused)]
impl AssetRenderState {
    pub fn lock(&self) -> LockedAssetRenderState {
        LockedAssetRenderState {
            meshes: self.meshes.lock_read(),
            materials: self.materials.lock_read(),
            textures: self.textures.lock_read(),
            shaders: self.shaders.lock_read(),
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
    pub fn texture(&self, id: usize) -> &Ref<Texture2D> {
        self.textures.get(id)
    }
}

pub(crate) struct LockedAssetRenderState<'a> {
    pub meshes: HashMap<usize, RwLockReadGuard<'a, Mesh>>,
    pub materials: HashMap<usize, RwLockReadGuard<'a, Material>>,
    pub textures: HashMap<usize, RwLockReadGuard<'a, Texture2D>>,
    pub shaders: HashMap<usize, RwLockReadGuard<'a, Shader>>,
}

#[allow(unused)]
impl LockedAssetRenderState<'_> {
    pub fn mesh(&self, id: usize) -> &RwLockReadGuard<Mesh> {
        self.meshes.get(&id).unwrap()
    }
    pub fn material(&self, id: usize) -> &RwLockReadGuard<Material> {
        self.materials.get(&id).unwrap()
    }
    pub fn texture(&self, id: usize) -> &RwLockReadGuard<Texture2D> {
        self.textures.get(&id).unwrap()
    }
    pub fn shader(&self, id: usize) -> &RwLockReadGuard<Shader> {
        self.shaders.get(&id).unwrap()
    }
}
