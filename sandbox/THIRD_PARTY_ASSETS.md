# Third-Party Sandbox Assets

## tank-game Tank_002 mesh

- Source repository: https://github.com/cmilatinov/tank-game
- Source file: `Assets/Plugins/Tank_Controller/Art/Objects/Tank/Tank_002.fbx`
- Files:
  - `assets/game/models/Tank_002.fbx`
  - `assets/game/meshes/*.obj` (all but `arena_cube.obj`)
  - `assets/game/materials/*.cxmat`
- Notes: exported from the source FBX into OBJ parts for the Calyx mesh loader; the source repository does not include an explicit license file.

## Fox model

- Source: Khronos glTF Sample Assets, https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/Fox
- Files:
  - `assets/showcase/main/models/fox/Fox.glb`
  - `assets/showcase/main/textures/fox.png` (the model's `Texture.png`)
- License and credit:
  - Model by PixelMannen, CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/).
  - Rigging and animation by tomkranis, CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/).
  - Conversion to glTF by @AsoboStudio and @scurest, CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/).
- Notes: used unchanged. The Calyx glTF importer does not import materials, so `assets/showcase/main/materials/fox.cxmat` applies the texture.

## Industrial Sunset (Pure Sky) HDRI

- Source: Poly Haven, https://polyhaven.com/a/industrial_sunset_puresky
- Files:
  - `assets/shared/skyboxes/industrial_sunset_puresky_4k.exr`
- License and credit: CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/). Original by Sergej Majboroda, sky edits by Jarod Guest.
