# Model import fixtures

Test-only models loaded by the import snapshot tests through `TestAssets::models`.

## RiggedFigure

`glb/RiggedFigure.glb` is the Rigged Figure sample model from the
[Khronos glTF Sample Assets](https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/RiggedFigure),
© 2017 Cesium, licensed under
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/legalcode).

`fbx/RiggedFigure.fbx` is the same model converted to FBX with Blender 4.0: imported from the GLB
with default settings, the scene frame range set to the animation's own range (frames 0 to 30 at
24 fps), and exported with leaf bones off and the animation baked. It carries the same
attribution and licence.

`fbx/RiggedFigureOffset.fbx` is the same conversion with every animation key moved 30 frames later,
the frame range set to frames 30 to 60, and the current frame moved by the same amount, so its
static pose matches. It checks that a clip whose playback range starts after frame 0 plays from
its first key.

## TwoObjects

`obj/TwoObjects.obj` is hand-written: a textured quad in object `Left`, and an untextured quad in
object `Right` whose two triangles use different materials. It checks that a mesh asset keeps
every object and material part of an OBJ file.

## TwoMaterials

`fbx/TwoMaterials.fbx` was made with Blender 4.0: the default cube, named `Cube`, with materials
`Red` and `Blue`, its top face using `Blue`, exported with default settings. It checks that a
prefab keeps every material part of one FBX node.

## TwoSkins

`gltf/TwoSkins.gltf` is a hand-built model written by a script: two meshes skinned by one
Hip, Spine and Arm joint tree, whose skins list the joints in different orders and counts (the
body's `[Hip, Spine]`, the arm's `[Arm, Spine, Hip]`), and a one-second clip that moves the hip
and turns the spine but has no channel for the arm. It checks that each skinned mesh follows its
own bones.
