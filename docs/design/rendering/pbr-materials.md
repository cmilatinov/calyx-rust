# PBR Materials

The material inputs of `shaders/mesh/pbr` follow glTF's metallic-roughness
model. A `Material` binds values to the variables its shader declares, and
the deferred pass lights the result ([G-Buffer](gbuffer.md)).

Code: `assets/shaders/mesh/pbr.wgsl`, `crates/calyx_engine/src/assets/material.rs`,
`crates/calyx_engine/src/assets/texture.rs`,
`crates/calyx_engine/src/assets/import/tangents.rs`.

## Group 3

| Binding | Input |
| --- | --- |
| 0 | Base color texture, multiplied by `base_color_factor` |
| 2 | The `MaterialProperties` uniform |
| 3 | Metallic-roughness texture, whose B and G scale the `metallic` and `roughness` factors |
| 5 | Tangent-space normal texture (+Y up the image), scaled in X and Y by `normal_scale` |
| 7 | Emissive texture, multiplied by `emission_color * emission_strength` |
| 9 | Occlusion texture, whose R applies to the ambient light by `occlusion_strength`, on top of `ambient_occlusion` |

Each texture's sampler is bound right after it. Point the occlusion and
metallic-roughness slots at the same image for glTF's packed
occlusion-roughness-metallic texture.

## Alpha cutoff

`alpha_cutoff` discards fragments whose base color alpha is below it (glTF
`MASK`) in the scene pass and in the shadow pass
([Shadow Mapping](shadows.md#the-shadow-pass)). The object-id pick, outline
and thumbnail passes draw the whole mesh. `BLEND` is not supported.

## Saved materials

Uniform fields are only appended, so a saved material's offsets stay valid. A
`.cxmat` lacking a variable its shader declares gets the reflected default
when it loads: white textures and colors, the flat normal for a normal
texture, one for `normal_scale` and `occlusion_strength`, and zero otherwise,
which together render as the material did before the input existed.

## Color space

Texture color space is chosen per material slot, not per image.

- An 8-bit image texture also gets a copy in the sRGB form of its format
  (`Texture::srgb_texture`), whose mips are filtered after decoding so a
  minified color image keeps its brightness.
- A slot whose shader variable names a color (base color, albedo, diffuse,
  emissive) binds that copy's view, so it decodes sRGB.
- Every other slot reads the stored texture, whose mips average the channels
  as stored.
- A solid `Color` source holds linear values in any slot.

## Tangents

Mesh vertices carry a tangent (`Vertex` location 8): `xyz` along increasing U,
`w` the handedness of the bitangent `cross(normal, tangent) * w`, which points
up the image.

- The glTF importer reads `TANGENT`.
- Any imported mesh without tangents gets them from
  `assets/import/tangents.rs`, a MikkTSpace-style generator (angle-weighted
  per-triangle frames projected onto each normal, with vertices split where UV
  handedness flips).
- The PBR vertex shader skins tangents with the same bone matrices as normals.
- A mesh built in code without tangents gets `Mesh::DEFAULT_TANGENT`.
