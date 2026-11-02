# Calyx Demos

A Calyx project of standalone demo scenes, each one showing off what the
engine renders. It sits next to `sandbox/`, the tank game, and leaves it
alone.

Open it in the editor with

```
calyx_ctl launch --release --project ./demos
```

or `cargo run -p calyx_editor --release -- ./demos`. Play starts the
project's startup scene, the first demo below. To switch demos, open another
scene from the content browser, or with `calyx_ctl scene load <path>`, and
press play.

## Demos

| Demo | Scene | |
| --- | --- | --- |
| ![RPG Poly village](docs/rpg_poly.png) | [`assets/rpg_poly/rpg_poly.cxscene`](assets/rpg_poly/rpg_poly.cxscene) | **RPG Poly village.** The demo village of Unity's [RPG Poly Pack - Lite](https://assetstore.unity.com/packages/3d/environments/landscapes/rpg-poly-pack-lite-148410), object for object from the pack's own scene, with sun shadows, image-based ambient light, SSAO and a post-processing volume. In play the camera flies a loop above the village. **Needs the pack:** see below. |

## Importing RPG Poly Pack - Lite

The pack's licence keeps its files out of this public repository, so the
RPG Poly village needs a one-time local import:

1. Download the free pack through the Unity editor's Package Manager
   (My Assets) after adding it to your Unity account from its store page.
2. Close the Calyx editor, then run from the repository root:

   ```
   python demos/tools/rpg_poly/import_pack.py
   ```

   It needs Python 3 and Blender 4.x, and builds the models, textures and
   materials into the gitignored `assets/rpg_poly/vendor/`.

Without it the scene opens with only its sky, and the editor log names the
missing assets. [`THIRD_PARTY_ASSETS.md`](THIRD_PARTY_ASSETS.md) has the
licence details and the full import options.

The flythrough uses the engine's `ComponentCameraPath`: the `Camera` follows the children of the `Camera Path` object. Components a demo needs that the engine does not ship go in `src/`.
