"""Blender side of `import_pack.py`: converts the pack's FBX meshes to OBJ and
its TGA textures to PNG.

Run by `import_pack.py` as
`blender --background --factory-startup --python blender_convert.py -- job.json`.
The job lists `{"meshes": [[fbx, obj, normal mode, smoothing angle], ...], "textures": [[tga, png, fog], ...]}`,
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
- Normals follow the model's Unity import settings (`normalImportMode` in
  its `.meta`). The pack's models all use Calculate with a smoothing angle
  of 0, so Unity throws the FBX's (smooth) normals away and gives every
  triangle its own face normal: the faceted low-poly look. Calculate mode
  is reproduced here: a corner's normal averages the normals of the faces
  around its vertex that lie within the smoothing angle of its own face.
  Import mode keeps the FBX's split normals; None falls back to Calculate,
  as Unity does for a lit shader.
"""

import math

import json
import sys

import bpy

# Unity's ModelImporterNormals: Import, Calculate, None.
NORMALS_IMPORT, NORMALS_CALCULATE, NORMALS_NONE = 0, 1, 2


def face_normal(a, b, c):
    u = [b[i] - a[i] for i in range(3)]
    v = [c[i] - a[i] for i in range(3)]
    n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    length = math.sqrt(sum(x * x for x in n))
    return [x / length for x in n] if length > 0 else [0.0, 0.0, 0.0], length / 2


def calculated_normals(triangles, smooth_angle):
    """Unity's calculated normals for each triangle corner. `triangles` are
    corner positions as written, in their front-face (counter-clockwise)
    order. A corner's normal averages, weighted by area, the normals of the
    triangles around its position whose angle to its own triangle is within
    `smooth_angle` degrees, so 0 gives each triangle its face normal."""
    faces = [face_normal(*tri) for tri in triangles]
    limit = math.cos(math.radians(smooth_angle)) - 1e-6
    around = {}
    if smooth_angle > 0:
        for index, tri in enumerate(triangles):
            for corner in tri:
                around.setdefault(tuple(round(c, 5) for c in corner), []).append(index)
    normals = []
    for index, tri in enumerate(triangles):
        own, _ = faces[index]
        corners = []
        for corner in tri:
            total = list(own)
            if smooth_angle > 0:
                total = [0.0, 0.0, 0.0]
                for other in around[tuple(round(c, 5) for c in corner)]:
                    normal, area = faces[other]
                    if sum(normal[i] * own[i] for i in range(3)) >= limit:
                        total = [total[i] + normal[i] * area for i in range(3)]
                length = math.sqrt(sum(x * x for x in total))
                total = [x / length for x in total] if length > 0 else list(own)
            corners.append(total)
        normals.append(corners)
    return normals


def write_obj(path, mesh, scale, normal_mode=NORMALS_CALCULATE, smooth_angle=0.0):
    mesh.calc_loop_triangles()
    # Negating X mirrors the mesh; reversing each triangle's winding keeps its
    # front face facing out.
    def position(loop_index):
        co = mesh.vertices[mesh.loops[loop_index].vertex_index].co
        # Rounded as written, so each face normal is the written triangle's.
        return (round(-co.x * scale[0], 5), round(co.y * scale[1], 5), round(co.z * scale[2], 5))

    corner_loops = [list(reversed(tri.loops)) for tri in mesh.loop_triangles]
    triangles = [[position(loop) for loop in loops] for loops in corner_loops]
    if normal_mode == NORMALS_IMPORT:
        if hasattr(mesh, "calc_normals_split"):
            mesh.calc_normals_split()
        normals = [[(-mesh.loops[l].normal.x, mesh.loops[l].normal.y, mesh.loops[l].normal.z)
                    for l in loops] for loops in corner_loops]
    else:
        normals = calculated_normals(triangles, smooth_angle)
    uv_layer = mesh.uv_layers.active
    tables = {"v": {}, "vt": {}, "vn": {}}

    def index(kind, text):
        table = tables[kind]
        if text not in table:
            table[text] = len(table) + 1
        return table[text]

    faces = []
    for loops, tri, tri_normals in zip(corner_loops, triangles, normals):
        corners = []
        for loop_index, co, normal in zip(loops, tri, tri_normals):
            v = index("v", "%.5f %.5f %.5f" % tuple(co))
            n = index("vn", "%.4f %.4f %.4f" % tuple(normal))
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


def convert_mesh(fbx, obj, normal_mode=NORMALS_CALCULATE, smooth_angle=0.0):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=fbx)
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if len(meshes) != 1:
        raise SystemExit(f"{fbx}: expected one mesh, found {len(meshes)}")
    obj_data = meshes[0]
    write_obj(obj, obj_data.data, tuple(obj_data.scale), normal_mode, smooth_angle)
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
    for fbx, obj, normal_mode, smooth_angle in job["meshes"]:
        convert_mesh(fbx, obj, normal_mode, smooth_angle)
    for tga, png, fog in job["textures"]:
        convert_texture(tga, png, fog)


main()
