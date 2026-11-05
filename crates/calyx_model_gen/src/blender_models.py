"""Blender side of `calyx_model_gen`: builds each model of a job and writes
it, and its ink outline hull, as OBJ.

Embedded in the generator, which writes it beside its job and runs
`blender --background --factory-startup --python blender_models.py -- job.json`.
The job is `{"models": [{"name", "obj", "outline_obj", "outline", "shading",
"shape"}, ...]}`; `shape` is the parameter file's kind and keys, sizes in
metres in Calyx's axes (x across, y up, z forward).

Each OBJ is in Calyx's space: left-handed, Y up, +Z forward. Blender is
right-handed and Z up, so a Blender point (x, y, z) is written (x, z, y).
Swapping two axes mirrors the mesh, so each triangle's winding is reversed
to keep its front face, as `calyx_unity_import` does for its own mirror.
Sourced models are imported the way Blender reads their format (glTF and
OBJ files are converted from Y up on import), then go through the same
writer.

Normals: flat shading gives each triangle its face normal; smooth shading
averages, by area, the normals of the triangles around each corner's
position whose angle to the corner's own triangle is within the smoothing
angle, so bevels read soft and creases between faces stay sharp. Corners
are matched by position, so seams in the source (UV or material splits)
do not show.

The outline hull is the model pushed out along its fully smoothed normals
by the outline thickness, with every triangle's winding flipped and flat
normals: drawn in the ink material with back-face culling, only its rim
shows round the model.

Each model built prints `generated <name>`; the generator counts those.
"""

import json
import math
import os
import random
import sys

import bmesh
import bpy


def reset():
    # A kit has hundreds of named parts. Reloading factory settings for
    # every small mesh repeatedly reloads Blender's whole scene/UI state.
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for mesh in list(bpy.data.meshes):
        if mesh.users == 0:
            bpy.data.meshes.remove(mesh)


def bevel(obj, width, segments, angle=None):
    """Adds a bevel modifier rounding `obj`'s edges (only those sharper than
    `angle` degrees, when given)."""
    if width <= 0:
        return
    modifier = obj.modifiers.new("bevel", "BEVEL")
    modifier.width = width
    modifier.segments = segments
    modifier.limit_method = "NONE" if angle is None else "ANGLE"
    if angle is not None:
        modifier.angle_limit = math.radians(angle)
    modifier.harden_normals = False


def box(size, centre_y, bevel_width, segments):
    """A box of Calyx `size` (x, y, z) whose centre is at height `centre_y`."""
    bpy.ops.mesh.primitive_cube_add(size=1, location=(0, 0, centre_y))
    obj = bpy.context.active_object
    obj.scale = (size[0], size[2], size[1])
    bpy.ops.object.transform_apply(scale=True)
    bevel(obj, bevel_width, segments)
    return obj


def cylinder(radius, height, vertices, bevel_width, segments):
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=vertices, radius=radius, depth=height, location=(0, 0, height / 2)
    )
    obj = bpy.context.active_object
    bevel(obj, bevel_width, segments)
    return obj


def ico(radius, location, subdivisions):
    bpy.ops.mesh.primitive_ico_sphere_add(
        subdivisions=subdivisions, radius=radius, location=location
    )
    return bpy.context.active_object


def build_box(shape):
    size = shape["size"]
    parts = [box(size, size[1] / 2, shape["bevel"], shape["segments"])]
    for key, centre in (("band", size[1] / 2), ("foot", None)):
        band = shape.get(key)
        if not band:
            continue
        height = band["height"]
        overhang = band["overhang"]
        width = (size[0] + 2 * overhang, height, size[2] + 2 * overhang)
        y = centre if centre is not None else height / 2
        # Round the band about as much as the block, within its own height.
        rounding = min(shape["bevel"], height * 0.45)
        parts.append(box(width, y, rounding, shape["segments"]))
    return parts


def build_rock(shape):
    size = shape["size"]
    obj = ico(1.0, (0, 0, 0), shape["subdivisions"])
    rnd = random.Random(shape["seed"])
    for vertex in obj.data.vertices:
        direction = vertex.co.normalized()
        vertex.co = direction * (1.0 + rnd.uniform(-shape["jitter"], shape["jitter"]))
    # Squash to the size, then stand it on the ground.
    for vertex in obj.data.vertices:
        vertex.co.x *= size[0] / 2
        vertex.co.y *= size[2] / 2
        vertex.co.z *= size[1] / 2
    # The jitter moves the extremes; rescale so the rock spans its size.
    xs = [v.co.x for v in obj.data.vertices]
    ys = [v.co.y for v in obj.data.vertices]
    zs = [v.co.z for v in obj.data.vertices]
    spans = (max(xs) - min(xs), max(ys) - min(ys), max(zs) - min(zs))
    targets = (size[0], size[2], size[1])
    lows = (min(xs), min(ys), min(zs))
    highs = (max(xs), max(ys), max(zs))
    for vertex in obj.data.vertices:
        for k in range(3):
            centre = (lows[k] + highs[k]) / 2
            vertex.co[k] = (vertex.co[k] - centre) * targets[k] / spans[k]
        vertex.co.z += size[1] / 2
    return [obj]


def build_cylinder(shape):
    parts = [
        cylinder(
            shape["radius"], shape["height"], shape["vertices"], shape["bevel"], shape["segments"]
        )
    ]
    if shape.get("top_radius") is not None:
        ratio = shape["top_radius"] / shape["radius"]
        for vertex in parts[0].data.vertices:
            factor = 1 + (ratio - 1) * (vertex.co.z / shape["height"] + 0.5)
            vertex.co.x *= factor
            vertex.co.y *= factor
    flare = shape.get("flare")
    if flare:
        rounding = min(shape["bevel"] * 1.5, flare["height"] * 0.45, flare["radius"] * 0.45)
        parts.append(
            cylinder(flare["radius"], flare["height"], shape["vertices"], rounding, shape["segments"])
        )
    return parts


def build_blob(shape):
    parts = [
        ico(lobe["radius"], (lobe["center"][0], lobe["center"][2], lobe["center"][1]),
            shape["subdivisions"])
        for lobe in shape["lobes"]
    ]
    if shape.get("fuse", 0) > 0:
        bpy.ops.object.select_all(action="DESELECT")
        for obj in parts:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = parts[0]
        bpy.ops.object.join()
        obj = bpy.context.active_object
        remesh = obj.modifiers.new("join canopy lobes", "REMESH")
        remesh.mode = "VOXEL"
        remesh.voxel_size = shape["fuse"]
        smooth = obj.modifiers.new("soft clay surface", "SMOOTH")
        smooth.factor = 0.8
        smooth.iterations = 4
        decimate = obj.modifiers.new("low poly silhouette", "DECIMATE")
        decimate.ratio = 0.3
        return [obj]
    return parts


def build_prism(shape):
    points = shape["points"]
    area = sum(a[0]*b[1]-b[0]*a[1] for a,b in zip(points,points[1:]+points[:1]))
    if area < 0:
        points = list(reversed(points))
    count = len(points)
    vertices = [(x,z,y) for y in (0,shape["height"]) for x,z in points]
    faces = [list(reversed(range(count))), list(range(count,2*count))]
    faces += [(i,(i+1)%count,(i+1)%count+count,i+count) for i in range(count)]
    mesh = bpy.data.meshes.new("extruded footprint")
    mesh.from_pydata(vertices,[],faces)
    mesh.update()
    obj = bpy.data.objects.new("Prism",mesh)
    bpy.context.collection.objects.link(obj)
    bevel(obj,shape["bevel"],shape["segments"])
    return [obj]


def build_ring(shape):
    count = shape["vertices"]
    vertices = [
        (radius * math.cos(2 * math.pi * i / count),
         radius * math.sin(2 * math.pi * i / count), height)
        for height in (0, shape["height"])
        for radius in (shape["radius"], shape["inner_radius"])
        for i in range(count)
    ]
    faces = []
    for i in range(count):
        j = (i + 1) % count
        faces += [
            (i, j, j + 2 * count, i + 2 * count),
            (i + count, i + 3 * count, j + 3 * count, j + count),
            (i + 2 * count, j + 2 * count, j + 3 * count, i + 3 * count),
            (i, i + count, j + count, j),
        ]
    mesh = bpy.data.meshes.new("annular band")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new("Ring", mesh)
    bpy.context.collection.objects.link(obj)
    return [obj]


def build_import(shape):
    source = shape["source"]
    extension = os.path.splitext(source)[1].lower()
    if extension in (".glb", ".gltf"):
        bpy.ops.import_scene.gltf(filepath=source)
    elif extension == ".obj":
        bpy.ops.wm.obj_import(filepath=source)
    elif extension == ".fbx":
        bpy.ops.import_scene.fbx(filepath=source)
    else:
        raise SystemExit(f"cannot import {source}")
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        raise SystemExit(f"{source} has no mesh")
    parts = []
    for obj in meshes:
        # Bake the object's transform, parents included, into its vertices.
        # A mirroring transform (negative determinant) turns the faces
        # inside out, so their winding is reversed with it.
        mirrored = obj.matrix_world.determinant() < 0
        obj.data = obj.data.copy()
        obj.data.transform(obj.matrix_world)
        obj.parent = None
        obj.matrix_world.identity()
        obj.data.materials.clear()
        # Sources split vertices at UV and material seams; weld them so a
        # bevel rounds the real edges.
        mesh = bmesh.new()
        mesh.from_mesh(obj.data)
        if mirrored:
            bmesh.ops.reverse_faces(mesh, faces=mesh.faces)
        bmesh.ops.remove_doubles(mesh, verts=mesh.verts, dist=1e-4)
        mesh.to_mesh(obj.data)
        mesh.free()
        bevel(obj, shape["bevel"], shape["segments"], angle=30)
        parts.append(obj)
    return parts


BUILDERS = {
    "ring": build_ring,
    "prism": build_prism,
    "box": build_box,
    "rock": build_rock,
    "cylinder": build_cylinder,
    "blob": build_blob,
    "import": build_import,
}


def triangles(parts):
    """Every triangle of `parts`, modifiers applied, in Calyx space with
    front faces kept."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    out = []
    for obj in parts:
        evaluated = obj.evaluated_get(depsgraph)
        mesh = evaluated.to_mesh()
        mesh.transform(obj.matrix_world)
        mesh.calc_loop_triangles()
        for tri in mesh.loop_triangles:
            corners = [mesh.vertices[mesh.loops[l].vertex_index].co for l in tri.loops]
            out.append([(c.x, c.z, c.y) for c in reversed(corners)])
        evaluated.to_mesh_clear()
    return out


def place_import(tris, shape):
    """Scales and turns a sourced model about the vertical axis, as an
    object rotation of `rotate_y` degrees about Y would in Calyx, then
    centres it on x and z and stands it on the ground."""
    scale = shape["scale"]
    angle = math.radians(shape["rotate_y"])
    cos, sin = math.cos(angle), math.sin(angle)
    turned = [
        [((x * cos + z * sin) * scale, y * scale, (-x * sin + z * cos) * scale)
         for x, y, z in tri]
        for tri in tris
    ]
    points = [p for tri in turned for p in tri]
    low = [min(p[k] for p in points) for k in range(3)]
    high = [max(p[k] for p in points) for k in range(3)]
    offset = (-(low[0] + high[0]) / 2, -low[1], -(low[2] + high[2]) / 2)
    return [[tuple(p[k] + offset[k] for k in range(3)) for p in tri] for tri in turned]


def rounded(tris):
    """Positions as written, so each face normal is the written triangle's
    and corners at one position match."""
    return [[tuple(round(c, 5) + 0.0 for c in p) for p in tri] for tri in tris]


def face_normal(a, b, c):
    u = [b[i] - a[i] for i in range(3)]
    v = [c[i] - a[i] for i in range(3)]
    n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    length = math.sqrt(sum(x * x for x in n))
    return ([x / length for x in n] if length > 0 else [0.0, 1.0, 0.0]), length / 2


def corner_normals(tris, shading):
    faces = [face_normal(*tri) for tri in tris]
    if shading["mode"] == "flat":
        return [[face] * 3 for face, _ in faces]
    limit = math.cos(math.radians(shading["angle"])) - 1e-6
    around = {}
    for index, tri in enumerate(tris):
        for corner in tri:
            around.setdefault(corner, []).append(index)
    normals = []
    for index, tri in enumerate(tris):
        own = faces[index][0]
        corners = []
        for corner in tri:
            total = [0.0, 0.0, 0.0]
            for other in around[corner]:
                normal, area = faces[other]
                if sum(normal[k] * own[k] for k in range(3)) >= limit:
                    total = [total[k] + normal[k] * area for k in range(3)]
            length = math.sqrt(sum(x * x for x in total))
            corners.append([x / length for x in total] if length > 0 else own)
        normals.append(corners)
    return normals


def hull(tris, thickness):
    """The inverted hull: each position pushed out along its area-weighted
    normal over every triangle around it, winding flipped."""
    sums = {}
    for tri in tris:
        normal, area = face_normal(*tri)
        for corner in tri:
            total = sums.setdefault(corner, [0.0, 0.0, 0.0])
            for k in range(3):
                total[k] += normal[k] * area
    pushed = {}
    for corner, total in sums.items():
        length = math.sqrt(sum(x * x for x in total)) or 1.0
        pushed[corner] = tuple(corner[k] + total[k] / length * thickness for k in range(3))
    return rounded([[pushed[c] for c in reversed(tri)] for tri in tris])


def write_obj(path, name, tris, normals):
    positions, directions, faces = {}, {}, []

    def index(table, key):
        if key not in table:
            table[key] = len(table) + 1
        return table[key]

    for tri, tri_normals in zip(tris, normals):
        # Skip triangles that rounding collapsed.
        if len(set(tri)) < 3:
            continue
        corners = []
        for corner, normal in zip(tri, tri_normals):
            v = index(positions, "%.5f %.5f %.5f" % corner)
            n = index(directions, "%.4f %.4f %.4f" % tuple(x + 0.0 for x in normal))
            corners.append("%d//%d" % (v, n))
        faces.append("f " + " ".join(corners))
    with open(path, "w", newline="\n") as f:
        f.write(f"# Generated by calyx_model_gen from {name}.toml; regenerate rather than edit\n")
        for text in positions:
            f.write(f"v {text}\n")
        for text in directions:
            f.write(f"vn {text}\n")
        f.write("\n".join(faces) + "\n")


def build(model):
    reset()
    shape = model["shape"]
    tris = triangles(BUILDERS[shape["kind"]](shape))
    if shape["kind"] == "import":
        tris = place_import(tris, shape)
    tris = rounded(tris)
    write_obj(model["obj"], model["name"], tris, corner_normals(tris, model["shading"]))
    if model["outline_obj"]:
        shell = hull(tris, model["outline"])
        write_obj(model["outline_obj"], model["name"], shell,
                  corner_normals(shell, {"mode": "flat"}))
    print(f"generated {model['name']}", flush=True)


def main():
    with open(sys.argv[sys.argv.index("--") + 1]) as f:
        job = json.load(f)
    for model in job["models"]:
        build(model)


main()
