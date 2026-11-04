# Third-Party Sandbox Assets

Every third-party asset committed to the sandbox is listed here with its
source, author, licence and URL. Only assets licensed CC0 or CC-BY are
committed; CC-BY assets carry their credit here. Unity Asset Store packs and
anything under another licence stay out of git.

Sourced models go through `calyx_model_gen` as `import` models
(`asset_sources/models/*.toml`): each parameter file carries its source's
credit, and the generator refuses a licence other than CC0 or CC-BY and a
source this file does not credit.

## tank-game Tank_002 mesh

- Source repository: https://github.com/cmilatinov/tank-game
- Source file: `Assets/Plugins/Tank_Controller/Art/Objects/Tank/Tank_002.fbx`
- Files:
  - `assets/game/models/Tank_002.fbx`
  - `assets/game/meshes/*.obj` (all but `arena_cube.obj` and `shell.obj`)
  - `assets/game/materials/tank_*.cxmat`
- Notes: exported from the source FBX into OBJ parts for the Calyx mesh loader; the source repository does not include an explicit license file.
- License: not recorded. This entry predates the CC0 / CC-BY rule above; its licence still needs confirming.

## Fox model

- Source: Khronos glTF Sample Assets, https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/Fox
- Files:
  - `assets/showcase/main/models/fox/Fox.glb`
- License and credit:
  - Model by PixelMannen, CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/).
  - Rigging and animation by tomkranis, CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/).
  - Conversion to glTF by @AsoboStudio and @scurest, CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/).
- Notes: used unchanged. The showcase renders it with the material and texture the glTF importer makes from the file.

## Industrial Sunset (Pure Sky) HDRI

- Source: Poly Haven, https://polyhaven.com/a/industrial_sunset_puresky
- Files:
  - `assets/shared/skyboxes/industrial_sunset_puresky_4k.exr`
- License and credit: CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/). Original by Sergej Majboroda, sky edits by Jarod Guest.
