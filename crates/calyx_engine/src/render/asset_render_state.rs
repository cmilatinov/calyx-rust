use crate::assets::material::Material;
use crate::assets::mesh::Mesh;
use crate::assets::skybox::Skybox;
use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::core::Ref;
use crate::render::Shader;
use std::collections::HashMap;
use std::sync::RwLockReadGuard;

/// The assets one scene renderer draws in the current frame.
///
/// It is rebuilt every frame: [`Self::clear`] runs before the frame collects
/// its assets, so an asset no longer drawn, or deleted from the registry, is
/// no longer held.
#[derive(Default)]
pub(crate) struct AssetRenderState {
    pub meshes: HashMap<AssetId, Ref<Mesh>>,
    pub materials: HashMap<AssetId, Ref<Material>>,
    pub textures: HashMap<AssetId, Ref<Texture>>,
    pub shaders: HashMap<AssetId, Ref<Shader>>,
    pub skyboxes: HashMap<AssetId, Ref<Skybox>>,
}

fn lock_read<T>(refs: &HashMap<AssetId, Ref<T>>) -> HashMap<AssetId, RwLockReadGuard<'_, T>> {
    refs.iter().map(|(id, r)| (*id, r.read())).collect()
}

#[allow(unused)]
impl AssetRenderState {
    /// Forgets every asset of the previous frame.
    pub fn clear(&mut self) {
        self.meshes.clear();
        self.materials.clear();
        self.textures.clear();
        self.shaders.clear();
        self.skyboxes.clear();
    }

    /// Whether the state holds the asset `id` of any type.
    pub fn contains(&self, id: AssetId) -> bool {
        self.meshes.contains_key(&id)
            || self.materials.contains_key(&id)
            || self.textures.contains_key(&id)
            || self.shaders.contains_key(&id)
            || self.skyboxes.contains_key(&id)
    }

    pub fn lock(&self) -> LockedAssetRenderState<'_> {
        LockedAssetRenderState {
            meshes: lock_read(&self.meshes),
            materials: lock_read(&self.materials),
            textures: lock_read(&self.textures),
            shaders: lock_read(&self.shaders),
            skyboxes: lock_read(&self.skyboxes),
        }
    }
    pub fn mesh(&self, id: AssetId) -> Option<&Ref<Mesh>> {
        self.meshes.get(&id)
    }
    pub fn material(&self, id: AssetId) -> Option<&Ref<Material>> {
        self.materials.get(&id)
    }
    pub fn shader(&self, id: AssetId) -> Option<&Ref<Shader>> {
        self.shaders.get(&id)
    }
    pub fn texture(&self, id: AssetId) -> Option<&Ref<Texture>> {
        self.textures.get(&id)
    }
    pub fn skybox(&self, id: AssetId) -> Option<&Ref<Skybox>> {
        self.skyboxes.get(&id)
    }
}

pub(crate) struct LockedAssetRenderState<'a> {
    pub meshes: HashMap<AssetId, RwLockReadGuard<'a, Mesh>>,
    pub materials: HashMap<AssetId, RwLockReadGuard<'a, Material>>,
    pub textures: HashMap<AssetId, RwLockReadGuard<'a, Texture>>,
    pub shaders: HashMap<AssetId, RwLockReadGuard<'a, Shader>>,
    pub skyboxes: HashMap<AssetId, RwLockReadGuard<'a, Skybox>>,
}

#[allow(unused)]
impl LockedAssetRenderState<'_> {
    pub fn mesh(&self, id: AssetId) -> Option<&RwLockReadGuard<'_, Mesh>> {
        self.meshes.get(&id)
    }
    pub fn material(&self, id: AssetId) -> Option<&RwLockReadGuard<'_, Material>> {
        self.materials.get(&id)
    }
    pub fn texture(&self, id: AssetId) -> Option<&RwLockReadGuard<'_, Texture>> {
        self.textures.get(&id)
    }
    pub fn shader(&self, id: AssetId) -> Option<&RwLockReadGuard<'_, Shader>> {
        self.shaders.get(&id)
    }
    pub fn skybox(&self, id: AssetId) -> Option<&RwLockReadGuard<'_, Skybox>> {
        self.skyboxes.get(&id)
    }
}
