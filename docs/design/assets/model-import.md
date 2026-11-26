# Model Import

A model file imports as a prefab with mesh, animation, material and texture
sub-assets. `assets/import` turns a file into a library-independent imported
scene (meshes, node tree, baked animations), using ufbx for FBX and OBJ and
the gltf crate for glTF and GLB. Collada (`.dae`) is not supported.

Code: `crates/calyx_engine/src/assets/import/` (`gltf.rs`, `ufbx.rs`,
`tangents.rs`), `Material::from_imported`, `Texture::from_imported`, and the
prefab importer in `crates/calyx_engine/src/scene/prefab.rs`.

## What a model becomes

- A `.cxprefab` is authored; an FBX, glTF or GLB model imports into mesh and
  animation sub-assets, and glTF and GLB also into material and texture
  sub-assets.
- Every sub-asset of a model shares one namespace (meshes, then animations,
  materials and textures, each made unique with a `_<n>` suffix), and its id
  is the hash of `<model>/<name>`, so it stays the same across reimports
  while the file's names do.
- Imported meshes without tangents get them from `assets/import/tangents.rs`;
  see [PBR materials](../rendering/pbr-materials.md#tangents).

## glTF materials

- Each glTF material becomes a material sub-asset on `shaders/mesh/pbr`,
  named after it or `material_<index>`, with base color factor and texture,
  metallic and roughness factors and texture, normal texture and scale,
  emissive factor (as `emission_color` with `emission_strength` 1) and
  texture, and occlusion texture and strength; `ambient_occlusion` is 1.
- `MASK` sets `alpha_cutoff`, and `OPAQUE` and `BLEND` leave it at 0.
- Each pair of an image and a sampler becomes a texture sub-asset, named after
  the image, its file name or `image_<index>`, so an image several slots use
  uploads once with both its stored and sRGB views. The sampler's wrap modes
  and filters are kept, and a sampler without a mip filter samples only the
  full-size image.
- An image read from a buffer view, a data URI or an external file that is
  missing or does not decode is logged, and its slots keep their default.
- Each mesh component of the prefab takes its primitive's material, or
  `materials/default` for a primitive with none. A material set by hand on an
  instance is a [prefab-link](../scene/prefab-links.md) override and survives
  a reimport.

## Not imported

FBX materials, `BLEND` transparency, `doubleSided`, vertex colors, texture
coordinate sets other than `TEXCOORD_0`, the `KHR_materials_*`,
`KHR_texture_transform` and `KHR_texture_basisu` extensions, cameras, lights
and morph targets. Imported materials have no editable on-disk copy.

## Testing

Import snapshot tests compare each model against references captured from the
former assimp importer, and the GLB fixture's animation against the FBX
fixture's; see `crates/calyx_engine/tests/fixtures/`.
