# Lights

`LightManager` gathers every enabled light of the scene into one storage
buffer each frame, and every lit pass reads it through
`assets/shaders/common/lights.wgsl`. Shadows are in
[Shadow Mapping](shadows.md).

Code: `crates/calyx_engine/src/render/light_manager.rs`,
`assets/shaders/common/lights.wgsl`.

## Light buffer

`LightManager` packs every enabled directional, point and spot light into
`LightStorage` at group 2 binding 0: a 16-byte header of per-kind counts, then
one 64-byte `Light` per light, grouped by kind (directional, then point, then
spot).

- The range a light sits in is its kind, so the PBR shader runs one
  branch-free loop per kind and a light carries no kind field.
- Each kind reads only its own fields.
- `shadow_index` indexes the light's shadow.
- A disabled light is never gathered; the lights carry no `active` field of
  their own (see [Component Enabled State](../scene/component-enabled-state.md)).

## Falloff

`light_distance_attenuation` in `lights.wgsl`, shared by point and spot lights
and by every lit pass that includes the file, is

```
saturate(1 - (d/r)^4)^2 / (d^2 + 1)
```

for a point `d` world units from the light and the light's range `r`
(`ComponentPointLight::radius`, `ComponentSpotLight::range`).

- It is inverse-square with a `+1` that keeps it finite at the light, windowed
  so it reaches exactly 0 at `r` and stays 0 beyond.
- `intensity` is therefore the light's radiance at distance 0 before the
  color: a surface 1 unit away receives half of it and one 3 units away a
  tenth, while well inside the range. The window only dims the outer part of
  the range.
- A spot light multiplies the falloff by its cone factor.
- Directional light `intensity` is the radiance everywhere.
