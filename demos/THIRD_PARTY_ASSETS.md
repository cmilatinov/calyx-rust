# Third-Party Demo Assets

## RPG Poly Pack - Lite (not in this repository)

The first demo, `assets/rpg_poly/rpg_poly.cxscene`, is the demo village of
[RPG Poly Pack - Lite](https://assetstore.unity.com/packages/3d/environments/landscapes/rpg-poly-pack-lite-148410)
by Gigel3d, a free Unity Asset Store package, rebuilt in Calyx from the
pack's own models, textures, materials and demo scene.

### Licence check

Checked on 2026-11-02:

- **Licence.** The store page lists the pack under the
  [Standard Unity Asset Store EULA](https://unity.com/legal/as-terms) as an
  Extension Asset. The pack has no licence of its own.
- **Redistribution in a public repository: not allowed.** The EULA licenses
  an asset for use "as incorporated and embedded in" a Licensed Product,
  where it must not be a substantial portion of that product (Appendix 1,
  section 2.2.1), and it allows no copies except "incidental transient or
  temporary copies" (Appendix 1, section 5.1). Committing the files, raw or
  converted, would distribute them as standalone, extractable files.
- **Use outside Unity: allowed, within the EULA.** The EULA has no clause that
  ties assets to the Unity engine, and Unity's support article
  [Can I use assets from the Asset Store with other engines?](https://support.unity.com/hc/en-us/articles/34387186019988-Can-I-use-assets-from-the-Asset-Store-with-other-engines)
  says assets may be used with other engines under the EULA, but not
  redistributed as standalone items or in a form others can extract.

So the pack stays out of git. Everyone who runs the demo downloads the pack
themselves and imports it locally.

### Import step

1. Get the pack (free) from its store page with a Unity account, and download
   it through the Unity editor's Package Manager (My Assets). Unity saves it
   to `%APPDATA%\Unity\Asset Store-5.x\Gigel3d\3D ModelsEnvironmentsLandscapes\RPG Poly Pack - Lite.unitypackage`.
2. With the Calyx editor closed, run from the repository root:

   ```
   python demos/tools/rpg_poly/import_pack.py
   ```

   It needs Python 3 and Blender 4.x (`--blender <blender.exe>` if Blender
   is not in its default place, `--package <path>` for another copy of the
   pack). It writes the converted models, textures and materials to
   `assets/rpg_poly/vendor/`, which `.gitignore` keeps out of git.

Without the import the scene still opens and plays, showing only the sky,
and the editor log names each missing asset once.

### What is committed

- `assets/rpg_poly/rpg_poly.cxscene`: the demo scene's layout, generated
  from the pack's `rpgpp_lt_scene_1.0.unity` by
  `import_pack.py --scene`. It holds object names, transforms and references
  to the vendor assets by id, and no geometry, textures or materials.
- `tools/rpg_poly/`: the importer.

## Kloofendal 48d Partly Cloudy (Pure Sky) HDRI

- Source: Poly Haven, https://polyhaven.com/a/kloofendal_48d_partly_cloudy_puresky
- Licence: CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/). Original by Greg Zaal, sky edits by Jarod Guest.
- Files: `assets/rpg_poly/sky/kloofendal_48d_partly_cloudy_puresky_2k.hdr`
- Used unchanged, at 2K, for the scene's ambient and reflected light; the
  pack's own sky dome and clouds are drawn in front of it.
