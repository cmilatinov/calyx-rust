"""Blender side of `import_pack.py`: converts the pack's FBX meshes to OBJ and
its TGA textures to PNG.

Run by `import_pack.py` as
`blender --background --factory-startup --python blender_convert.py -- job.json`.
The job lists `{"meshes": [[fbx, obj], ...], "textures": [[tga, png, fog], ...]}`,
where `fog` is null or `[r, g, b, fraction]` to blend the texture towards.

Each OBJ holds the mesh as Unity stores it, so the scene can reuse the
pack's transforms unchanged:

- Vertices are in the mesh's own node space (the FBX node's transform, which
  Blender turns into an object rotation, is not applied), scaled by the
  file's unit scale as Unity's `useFileScale` does.
- X is negated. FBX is right-handed and Unity left-handed, and Unity's FBX
  importer converts by negating X; Calyx's view is left-handed too, so the
  converted mesh renders the same way round as in Unity. Negating one axis
  mirrors the triangles, so each triangle's winding is reversed to keep its
  front face.
- Normals are the mesh's split (per-corner) normals, so the pack's flat
  low-poly shading survives.
"""

import json
import sys

import bpy


def write_obj(path, mesh, scale):
    mesh.calc_loop_triangles()
    if hasattr(mesh, "calc_normals_split"):
        mesh.calc_normals_split()
    uv_layer = mesh.uv_layers.active
    tables = {"v": {}, "vt": {}, "vn": {}}

    def index(kind, text):
        table = tables[kind]
        if text not in table:
            table[text] = len(table) + 1
        return table[text]

    faces = []
    for tri in mesh.loop_triangles:
        corners = []
        for loop_index in reversed(tri.loops):
            loop = mesh.loops[loop_index]
            co = mesh.vertices[loop.vertex_index].co
            normal = loop.normal
            v = index("v", "%.5f %.5f %.5f" % (-co.x * scale[0], co.y * scale[1], co.z * scale[2]))
            n = index("vn", "%.4f %.4f %.4f" % (-normal.x, normal.y, normal.z))
            if uv_layer:
                uv = uv_layer.data[loop_index].uv
                t = index("vt", "%.5f %.5f" % (uv.x, uv.y))
                corners.append("%d/%d/%d" % (v, t, n))
            else:
                corners.append("%d//%d" % (v, n))
        faces.append("f " + " ".join(corners))
    with open(path, "w", newline="\n") as f:
        f.write("# Converted from RPG Poly Pack - Lite by demos/tools/rpg_poly/import_pack.py\n")
        for kind in ("v", "vt", "vn"):
            for text in tables[kind]:
                f.write(f"{kind} {text}\n")
        f.write("\n".join(faces) + "\n")


def convert_mesh(fbx, obj):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=fbx)
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if len(meshes) != 1:
        raise SystemExit(f"{fbx}: expected one mesh, found {len(meshes)}")
    obj_data = meshes[0]
    write_obj(obj, obj_data.data, tuple(obj_data.scale))
    print(f"converted {obj}", flush=True)


def convert_texture(tga, png, fog=None):
    image = bpy.data.images.load(tga)
    if not image.has_data:
        image.pixels[0]  # loads the pixels; saving needs them
    if fog:
        # Blends the texture towards the fog colour by the fog fraction,
        # [r, g, b, fraction], as Unity's fog does at the object's distance.
        r, g, b, amount = fog
        pixels = list(image.pixels)
        for i in range(0, len(pixels), 4):
            pixels[i] += (r - pixels[i]) * amount
            pixels[i + 1] += (g - pixels[i + 1]) * amount
            pixels[i + 2] += (b - pixels[i + 2]) * amount
        image.pixels[:] = pixels
    image.file_format = "PNG"
    image.filepath_raw = png
    image.save()
    print(f"converted {png}", flush=True)


def main():
    job = json.load(open(sys.argv[sys.argv.index("--") + 1]))
    for fbx, obj in job["meshes"]:
        convert_mesh(fbx, obj)
    for tga, png, fog in job["textures"]:
        convert_texture(tga, png, fog)


main()
