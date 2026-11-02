"""Builds the RPG Poly Pack Lite remake's assets and scene.

Reads the Kenney kits (CC0) from the directories given on the command line and
writes everything the scene uses under `demos/assets/rpg_poly/`:

- each Kenney model the scene places, converted to one OBJ per material part
  with its node transforms baked in, since the scene draws each part with one
  of the demo's own materials;
- the town and survival kits' colour atlases, regraded from Kenney's bright
  palette to the original's muted browns and straw;
- the terrain, distant hills, mountains, dirt paths and patches, generated
  here as faceted low-poly meshes;
- the materials and every `.meta` file, with ids derived from the asset
  names so a rebuild keeps them;
- `rpg_poly.cxscene`, laid out like the pack's demo village. The committed
  scene is this output loaded and saved once through the editor
  (`calyx_ctl scene save`), which writes the same scene in its own key order
  and float precision.

Usage (paths to the unzipped kits, as downloaded from kenney.nl):

    python -I demos/tools/rpg_poly/build.py --nature <Nature Kit dir> \
        --town <Fantasy Town Kit dir> --survival <Survival Kit dir> \
        --sky <kloofendal_48d_partly_cloudy_puresky_2k.hdr>

Needs numpy and Pillow. Run it from the repository root.
"""

import argparse
import colorsys
import json
import math
import os
import random
import shutil
import sys
import uuid

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import glb  # noqa: E402

OUT = os.path.join("demos", "assets", "rpg_poly")
ASSET_PREFIX = "rpg_poly"
NAMESPACE = uuid.UUID("5d7c1a3e-4f0b-4b8e-9a61-413413413413")

TYPE_MESH = "792d264b-de6f-4431-b59f-76f18fdb3bfe"
TYPE_TEXTURE = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2"
TYPE_MATERIAL = "f98a7f41-84d4-482d-b7af-a670b07035ae"
TYPE_SKYBOX = "bdb5dd3a-cca4-453d-8260-ff2bdf2a05b2"
TYPE_SCENE = "9946a2e7-e022-447e-8e60-528da548087f"
PBR_SHADER = "869cbd32-a789-f19d-3e24-5207f558d1c2"

C_ID = "02289c92-3412-406e-a7e5-3bbb15d7041e"
C_TRANSFORM = "c5b3b71f-1f14-4b5b-9881-436118684d29"
C_MESH = "93fd32b1-7127-4c88-8e89-893512af58de"
C_CAMERA = "a85867d2-3e68-42b2-b943-ea78c7c6ddb5"
C_SUN = "72b2568a-2ea0-4f58-ae76-e3f655006f0f"
C_SKY = "eb42f81f-45ab-428c-9d07-961696edc5fa"
C_VOLUME = "cdbfef3f-81a5-4ccb-a0f6-098bc5a26979"
C_AO = "01a950bc-d431-4495-8b86-7d51c5f2b4b0"
C_VIGNETTE = "61efa204-ed0d-4d1f-86a4-b226be940b13"
C_TONEMAP = "68ac12ac-fc9a-489e-b305-363eaf593966"
C_BLOOM = "7e09858d-fd71-4729-bb90-8ed76c0bda9a"
C_FXAA = "c19ff2b7-c3ed-4f04-9dfa-e026efdedf82"
C_GRADING = "d2694fb5-4815-471e-9537-4ab502c580f0"
C_CAMERA_PATH = "b6f1c0de-2a4e-4c5b-9d37-8e1f4a6b2c90"

RNG = random.Random(413)
WRITTEN = set()


def asset_id(name):
    return str(uuid.uuid5(NAMESPACE, name))


def write_meta(path_no_ext, name, type_uuid):
    meta = {
        "main": {
            "id": asset_id(name),
            "name": name,
            "display_name": name.rsplit("/", 1)[-1],
            "type_uuid": type_uuid,
        },
        "inner": [],
    }
    WRITTEN.add(path_no_ext + ".meta")
    with open(path_no_ext + ".meta", "w", newline="\n") as f:
        json.dump(meta, f, indent=2)
    return meta["main"]["id"]


# --------------------------------------------------------------------------
# Materials


def srgb(hex_color):
    """Linear RGBA from an sRGB hex colour."""
    rgb = [int(hex_color[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    lin = [c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in rgb]
    return [round(c, 4) for c in lin] + [1.0]


# Flat colours for the untextured parts, picked from the pack's screenshots.
PALETTE = {
    "ground": "#6f9a46",
    "dirt": "#d6bb83",
    "mountain": "#8e9097",
    "snow": "#eef1f5",
    "pine": "#3b6a3a",
    "leaves": "#5f8c38",
    "bark": "#6e4c33",
    "wood": "#a07a52",
    "wood_dark": "#6f523a",
    "plank": "#9e7850",
    "stone": "#a4a4a6",
    "grass": "#5f9440",
    "flower_yellow": "#f2c230",
    "flower_blue": "#4a5ed0",
    "flower_red": "#c8352e",
}

MATERIALS = {}


def material(name, color="#ffffff", texture=None, roughness=0.9):
    """Writes the PBR material `materials/<name>` once and returns its id."""
    asset = f"{ASSET_PREFIX}/materials/{name}"
    if asset in MATERIALS:
        return MATERIALS[asset]
    base_texture = {"Asset": texture} if texture else {"Color": [1.0, 1.0, 1.0, 1.0]}
    data = {
        "shader": PBR_SHADER,
        "variables": [
            {"group": 3, "binding": 0, "offset": None, "name": "base_color_texture",
             "span": None, "value": {"Texture2D": base_texture}},
            {"group": 3, "binding": 2, "offset": 0, "name": "metallic", "span": 4,
             "value": {"Float": 0.0}},
            {"group": 3, "binding": 2, "offset": 4, "name": "roughness", "span": 4,
             "value": {"Float": roughness}},
            {"group": 3, "binding": 2, "offset": 8, "name": "ambient_occlusion", "span": 4,
             "value": {"Float": 1.0}},
            {"group": 3, "binding": 2, "offset": 32, "name": "base_color_factor", "span": 16,
             "value": {"Color": srgb(color)}},
        ],
    }
    path = os.path.join(OUT, "materials", name)
    WRITTEN.add(path + ".cxmat")
    with open(path + ".cxmat", "w", newline="\n") as f:
        json.dump(data, f, indent=2)
    MATERIALS[asset] = write_meta(path, asset, TYPE_MATERIAL)
    return MATERIALS[asset]


# --------------------------------------------------------------------------
# Atlases


def regrade_atlas(src, dst, roof):
    """Regrades a Kenney colour atlas towards the pack's weathered wood.

    Saturated reds and pinks (the town kit's roofs and banners) become `roof`.
    Kenney's orange wood turns a darker brown. The blue-grey stone, white
    plaster and teal glass, which the pack's log houses do not have, become
    dark weathered timber. Any other colour keeps its hue and value but loses
    most of its saturation.
    """
    image = Image.open(src).convert("RGBA")
    pixels = np.array(image).astype(np.float64) / 255
    if roof:
        roof_h, roof_s, roof_v = colorsys.rgb_to_hsv(
            *[int(roof[i:i + 2], 16) / 255 for i in (1, 3, 5)])
    out = pixels.copy()
    for y in range(pixels.shape[0]):
        for x in range(pixels.shape[1]):
            r, g, b, a = pixels[y, x]
            h, s, v = colorsys.rgb_to_hsv(r, g, b)
            if roof and s > 0.45 and (h > 0.9 or h < 0.02):
                h, s, v = roof_h, roof_s, roof_v * (0.75 + 0.5 * v)
            elif 0.02 <= h < 0.12 and s > 0.15:
                h, s, v = 0.075, s * 0.55, v * 0.72
            elif 0.4 <= h < 0.8 or s < 0.15:
                h, s, v = 0.08, 0.22, 0.25 + 0.25 * v
            else:
                s *= 0.45
            out[y, x, :3] = colorsys.hsv_to_rgb(h, min(s, 1.0), min(v, 1.0))
    Image.fromarray((out * 255).round().astype(np.uint8)).save(dst)


# --------------------------------------------------------------------------
# OBJ output


def write_obj(path, positions, normals, uvs, faces):
    """Writes a triangle mesh; `faces` index positions, normals and uvs alike.

    Equal positions, normals and uvs are written once, so a faceted mesh
    shares its corners."""
    tables = {"v": {}, "vn": {}, "vt": {}}

    def index(kind, text):
        table = tables[kind]
        if text not in table:
            table[text] = len(table) + 1
        return table[text]

    lines = []
    for tri in faces:
        corners = []
        for i in tri:
            v = index("v", "%.4f %.4f %.4f" % tuple(positions[i]))
            n = index("vn", "%.4f %.4f %.4f" % tuple(normals[i]))
            if uvs is not None:
                t = index("vt", "%.5f %.5f" % (uvs[i][0], 1.0 - uvs[i][1]))
                corners.append("%d/%d/%d" % (v, t, n))
            else:
                corners.append("%d//%d" % (v, n))
        lines.append("f " + " ".join(corners))
    WRITTEN.add(path)
    with open(path, "w", newline="\n") as f:
        f.write("# Generated by demos/tools/rpg_poly/build.py\n")
        for kind in ("v", "vt", "vn"):
            for text in tables[kind]:
                f.write(f"{kind} {text}\n")
        f.write("\n".join(lines) + "\n")


MESHES = {}


def mesh_asset(rel, positions, normals, uvs, faces):
    """Writes `models/<rel>.obj` and its meta once; returns the mesh id."""
    asset = f"{ASSET_PREFIX}/models/{rel}"
    if asset in MESHES:
        return MESHES[asset]
    path = os.path.join(OUT, "models", *rel.split("/"))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    write_obj(path + ".obj", positions, normals, uvs, faces)
    MESHES[asset] = write_meta(path, asset, TYPE_MESH)
    return MESHES[asset]


def faceted(triangles):
    """Positions, per-face normals and faces for a list of (a, b, c) triangles."""
    positions, normals, faces = [], [], []
    for a, b, c in triangles:
        n = np.cross(np.subtract(b, a), np.subtract(c, a))
        length = np.linalg.norm(n)
        if length < 1e-9:
            continue
        n = n / length
        base = len(positions)
        positions += [a, b, c]
        normals += [n, n, n]
        faces.append((base, base + 1, base + 2))
    return positions, normals, faces


# --------------------------------------------------------------------------
# Kenney models


class Kit:
    def __init__(self, name, directory, atlas_material=None, palette=None):
        self.name = name
        self.directory = directory
        self.atlas_material = atlas_material
        self.palette = palette or {}
        self.parts = {}

    def model(self, model):
        """[(mesh id, material id)] of `model`, converting it on first use."""
        if model in self.parts:
            return self.parts[model]
        groups = {}
        for mat, pos, nrm, uv, idx in glb.primitives(os.path.join(self.directory, model + ".glb")):
            group = "atlas" if self.atlas_material else mat
            groups.setdefault(group, []).append((pos, nrm, uv, idx))
        parts = []
        for group, prims in groups.items():
            positions, normals, uvs, faces = [], [], [], []
            for pos, nrm, uv, idx in prims:
                base = len(positions)
                positions += list(pos)
                normals += list(nrm)
                uvs += list(uv) if uv is not None else [[0, 0]] * len(pos)
                tris = idx.reshape(-1, 3)
                faces += [tuple(t + base) for t in tris]
            suffix = "" if group == "atlas" else "_" + group
            rel = f"{self.name}/{model}{suffix}"
            mesh = mesh_asset(rel, positions, normals, uvs if self.atlas_material else None, faces)
            if self.atlas_material:
                mat = self.atlas_material
            else:
                mat = self.palette.get((model, group), self.palette.get(group))
                if mat is None:
                    raise SystemExit(f"no material for {model} part {group}")
            parts.append((mesh, mat))
        self.parts[model] = parts
        return parts


# --------------------------------------------------------------------------
# Scene graph


def quat_yaw(degrees):
    h = math.radians(degrees) / 2
    return [0.0, math.sin(h), 0.0, math.cos(h)]


def quat_from_matrix(m):
    t = m[0][0] + m[1][1] + m[2][2]
    if t > 0:
        s = math.sqrt(t + 1.0) * 2
        w = 0.25 * s
        x = (m[2][1] - m[1][2]) / s
        y = (m[0][2] - m[2][0]) / s
        z = (m[1][0] - m[0][1]) / s
    elif m[0][0] > m[1][1] and m[0][0] > m[2][2]:
        s = math.sqrt(1.0 + m[0][0] - m[1][1] - m[2][2]) * 2
        w = (m[2][1] - m[1][2]) / s
        x = 0.25 * s
        y = (m[0][1] + m[1][0]) / s
        z = (m[0][2] + m[2][0]) / s
    elif m[1][1] > m[2][2]:
        s = math.sqrt(1.0 + m[1][1] - m[0][0] - m[2][2]) * 2
        w = (m[0][2] - m[2][0]) / s
        x = (m[0][1] + m[1][0]) / s
        y = 0.25 * s
        z = (m[1][2] + m[2][1]) / s
    else:
        s = math.sqrt(1.0 + m[2][2] - m[0][0] - m[1][1]) * 2
        w = (m[1][0] - m[0][1]) / s
        x = (m[0][2] + m[2][0]) / s
        y = (m[1][2] + m[2][1]) / s
        z = 0.25 * s
    return [x, y, z, w]


def mirror_x(position, rotation):
    """Mirrors a layout placement across the X = 0 plane.

    Calyx's view is left-handed, so from the framing camera, which looks
    down -Z, +X is on the left. The layout below was drawn with +X on the
    right; mirroring every placement puts the longhouse on the right, as in
    the original. A placement only
    moves and turns: the models themselves are not mirrored."""
    x, y, z = position
    qx, qy, qz, qw = rotation
    return (-x, y, z), (qx, -qy, -qz, qw)


def quat_facing(direction, up=(0.0, 1.0, 0.0)):
    """The rotation whose local +Z points along `direction`, as nalgebra's
    `face_towards`: cameras look and lights shine down their +Z."""
    zaxis = np.array(direction, dtype=float)
    zaxis /= np.linalg.norm(zaxis)
    xaxis = np.cross(up, zaxis)
    xaxis /= np.linalg.norm(xaxis)
    yaxis = np.cross(zaxis, xaxis)
    m = np.column_stack([xaxis, yaxis, zaxis])
    return quat_from_matrix(m.tolist())


class Scene:
    def __init__(self):
        self.components = {}
        self.hierarchy = {}
        self.roots = []
        self.names = {}
        # Groups whose children are placed in layout coordinates.
        self.layout_groups = set()

    def add(self, name, parent=None, position=(0, 0, 0), rotation=(0, 0, 0, 1),
            scale=(1, 1, 1), **components):
        count = self.names.get(name, 0)
        self.names[name] = count + 1
        key = f"{parent or 'root'}/{name}/{count}"
        object_id = str(uuid.uuid5(NAMESPACE, "object:" + key))
        if isinstance(scale, (int, float)):
            scale = (scale, scale, scale)
        if parent is None or parent in self.layout_groups:
            position, rotation = mirror_x(position, rotation)
        data = {
            C_ID: {"id": object_id, "name": name, "visible": True},
            C_TRANSFORM: {"transform": {
                "position": [round(float(v), 4) for v in position],
                "rotation": [round(float(v), 6) for v in rotation],
                "scale": [round(float(v), 4) for v in scale],
            }},
        }
        data.update(components)
        self.components[object_id] = data
        if parent:
            self.hierarchy.setdefault(parent, []).append(object_id)
        else:
            self.roots.append(object_id)
        return object_id

    def model(self, name, parts, parent, position, yaw=0.0, scale=1.0, shadows=True):
        """Places a model's parts: one object when it has one part, else a
        group holding a child per part."""
        mesh = lambda m, mat: {C_MESH: {"cast_shadows": shadows, "material": mat, "mesh": m}}
        rotation = quat_yaw(yaw)
        if len(parts) == 1:
            return self.add(name, parent, position, rotation, scale, **mesh(*parts[0]))
        group = self.add(name, parent, position, rotation, scale)
        for index, (m, mat) in enumerate(parts):
            self.add(f"{name} part {index}", group, **mesh(m, mat))
        return group

    def save(self, path):
        data = {
            "components": self.components,
            "hierarchy": self.hierarchy,
            "roots": self.roots,
            "prefab_links": {},
        }
        with open(path, "w", newline="\n") as f:
            json.dump(data, f, indent=2)


# --------------------------------------------------------------------------
# Terrain


def hash2(ix, iz, seed):
    h = (ix * 374761393 + iz * 668265263 + seed * 2147483647) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((h ^ (h >> 16)) & 0xFFFF) / 0xFFFF


def value_noise(x, z, seed):
    ix, iz = math.floor(x), math.floor(z)
    fx, fz = x - ix, z - iz
    sx, sz = fx * fx * (3 - 2 * fx), fz * fz * (3 - 2 * fz)
    a = hash2(ix, iz, seed)
    b = hash2(ix + 1, iz, seed)
    c = hash2(ix, iz + 1, seed)
    d = hash2(ix + 1, iz + 1, seed)
    return (a + (b - a) * sx) + ((c + (d - c) * sx) - (a + (b - a) * sx)) * sz


def smoothstep(e0, e1, x):
    t = min(max((x - e0) / (e1 - e0), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def height(x, z):
    """Ground height: the village floor stays flat, rising into hills past it."""
    r = math.hypot(x, z)
    ripple = (value_noise(x / 18, z / 18, 1) - 0.5) * 0.6 * smoothstep(20, 40, r)
    hills = smoothstep(42, 120, r) * (6 + 14 * value_noise(x / 45, z / 45, 2))
    far = smoothstep(130, 260, r) * (12 + 30 * value_noise(x / 70, z / 70, 3))
    return ripple + hills + far


def build_terrain():
    extent, cells = 330.0, 96
    step = 2 * extent / cells
    grid = [[(-extent + i * step, 0.0, -extent + j * step) for j in range(cells + 1)]
            for i in range(cells + 1)]
    for i in range(cells + 1):
        for j in range(cells + 1):
            x, _, z = grid[i][j]
            jitter = 0.3 * step
            if 0 < i < cells and 0 < j < cells and math.hypot(x, z) > 30:
                x += (hash2(i, j, 7) - 0.5) * jitter
                z += (hash2(i, j, 8) - 0.5) * jitter
            # The layout is mirrored in X (see `mirror_x`), so the ground is too.
            grid[i][j] = (x, height(-x, z), z)
    triangles = []
    for i in range(cells):
        for j in range(cells):
            a, b, c, d = grid[i][j], grid[i + 1][j], grid[i + 1][j + 1], grid[i][j + 1]
            if (i + j) % 2:
                triangles += [(a, d, c), (a, c, b)]
            else:
                triangles += [(a, d, b), (b, d, c)]
    return mesh_asset("terrain/ground", *faceted_noflip(triangles))


def faceted_noflip(triangles):
    positions, normals, faces = faceted(triangles)
    # Every terrain face points up.
    for k in range(0, len(faces)):
        n = normals[3 * k]
        if n[1] < 0:
            a, b, c = faces[k]
            faces[k] = (a, c, b)
            for m in range(3):
                normals[3 * k + m] = -n
    return positions, normals, None, faces


def build_mountains():
    """Snow-capped low-poly peaks ringing the valley's far side."""
    rock, snow = [], []
    peaks = [(-170, -330, 120, 95), (-40, -390, 150, 130), (110, -360, 130, 105),
             (250, -300, 110, 85), (-300, -220, 100, 80), (330, -150, 90, 70),
             (-360, -40, 95, 70), (360, 60, 80, 60)]
    for index, (px, pz, radius, peak) in enumerate(peaks):
        rings, segments = 5, 11
        verts = []
        for ring in range(rings + 1):
            t = ring / rings
            row = []
            for s in range(segments):
                angle = 2 * math.pi * (s + 0.5 * ring) / segments
                wobble = 0.75 + 0.5 * hash2(index * 31 + ring, s, 5)
                rr = radius * (1 - t) * wobble
                y = peak * (t ** 0.9) * (0.9 + 0.2 * hash2(index, ring * 17 + s, 6)) - 10
                row.append((px + rr * math.cos(angle), y, pz + rr * math.sin(angle)))
            verts.append(row)
        apex = (px + radius * 0.05, peak + 4, pz)
        tris = []
        for ring in range(rings):
            for s in range(segments):
                a, b = verts[ring][s], verts[ring][(s + 1) % segments]
                if ring == rings - 1:
                    tris.append((a, apex, b))
                else:
                    c, d = verts[ring + 1][s], verts[ring + 1][(s + 1) % segments]
                    tris += [(a, c, b), (b, c, d)]
        for tri in tris:
            mean = sum(p[1] for p in tri) / 3
            (snow if mean > peak * 0.62 else rock).append(tri)
    rock_mesh = mesh_asset("terrain/mountains_rock", *faceted_noflip(rock))
    snow_mesh = mesh_asset("terrain/mountains_snow", *faceted_noflip(snow))
    return rock_mesh, snow_mesh


def ribbon(points, width, lift=0.03):
    """A dirt strip along a polyline, with ragged edges."""
    tris = []
    left, right = [], []
    for k, p in enumerate(points):
        a = points[max(k - 1, 0)]
        b = points[min(k + 1, len(points) - 1)]
        dx, dz = b[0] - a[0], b[1] - a[1]
        length = math.hypot(dx, dz)
        nx, nz = -dz / length, dx / length
        w1 = width * (0.8 + 0.5 * hash2(k, 1, 11))
        w2 = width * (0.8 + 0.5 * hash2(k, 2, 11))
        left.append((p[0] + nx * w1, lift, p[1] + nz * w1))
        right.append((p[0] - nx * w2, lift, p[1] - nz * w2))
    for k in range(len(points) - 1):
        tris += [(left[k], left[k + 1], right[k]), (right[k], left[k + 1], right[k + 1])]
    return tris


def blob(cx, cz, radius, seed, lift=0.025):
    tris = []
    segments = 9
    centre = (cx, lift, cz)
    rim = []
    for s in range(segments):
        angle = 2 * math.pi * s / segments
        r = radius * (0.65 + 0.6 * hash2(seed, s, 12))
        rim.append((cx + r * math.cos(angle), lift, cz + r * math.sin(angle)))
    for s in range(segments):
        tris.append((centre, rim[s], rim[(s + 1) % segments]))
    return tris


def densify(polyline, step):
    out = []
    for (ax, az), (bx, bz) in zip(polyline, polyline[1:]):
        n = max(1, int(math.hypot(bx - ax, bz - az) / step))
        for k in range(n):
            t = k / n
            out.append((ax + (bx - ax) * t, az + (bz - az) * t))
    out.append(polyline[-1])
    return out


# Plank paths through the village, as in the pack's demo: one in from the
# front, forking towards the houses at the back and one off to the left.
PATHS = [
    [(1, 40), (0, 26), (-1, 14), (0, 4)],
    [(0, 4), (-6, -4), (-11, -12), (-14, -21)],
    [(0, 4), (7, -3), (13, -10), (19, -16)],
    [(0, 4), (-9, 6), (-18, 7), (-27, 5)],
    [(0, 4), (2, -8), (2, -18)],
]

# The framing camera, then the stops of the loop it flies in play: (position, look-at).
CAMERA_PATH = [((2.0, 3.2, 34.0), (-0.5, 2.6, 0.0)),
               ((-14.0, 5.0, 22.0), (2.0, 2.0, -4.0)),
               ((-34.0, 13.0, -14.0), (2.0, 1.5, -10.0)),
               ((-6.0, 15.0, -46.0), (6.0, 1.0, -6.0)),
               ((22.0, 10.0, -36.0), (2.0, 2.0, -6.0)),
               ((32.0, 6.0, -2.0), (0.0, 2.0, -6.0)),
               ((20.0, 4.0, 26.0), (0.0, 2.0, -4.0))]

DIRT_PATCHES = [(-10, 12, 4.5), (8, 12, 3.5), (12, 2, 4.0), (-14, -2, 3.5), (3, -20, 5.0),
                (17, -11, 3.5), (-6, 22, 3.0), (-20, 10, 3.0), (6, 30, 3.0)]


def build_dirt():
    tris = []
    for path in PATHS:
        tris += ribbon(densify([(-x, z) for x, z in path], 2.5), 2.2)
    for k, (x, z, r) in enumerate(DIRT_PATCHES):
        tris += blob(-x, z, r, k)
    return mesh_asset("terrain/dirt", *faceted_noflip(tris))


# --------------------------------------------------------------------------
# Village


def house(scene, kits, parent, name, position, yaw, width, depth, material_id, scale=3.0,
          door_side="front"):
    """A log house from the town kit's modular walls under a steep roof.

    Cells are one kit unit; each wall piece covers its cell's +X face, so it
    turns to face out of the house. The roof's two halves meet over the
    centre line along Z; each roof piece closes its own gable end.
    """
    town = kits["town"]
    town_parts = lambda model: [(m, material_id) for m, _ in town.model(model)]
    group = scene.add(name, parent, position, quat_yaw(yaw), scale)
    faces = {"+x": 0, "-x": 180, "-z": 90, "+z": -90}
    for i in range(width):
        for j in range(depth):
            cx, cz = i - (width - 1) / 2, j - (depth - 1) / 2
            sides = []
            if i == width - 1:
                sides.append("+x")
            if i == 0:
                sides.append("-x")
            if j == 0:
                sides.append("-z")
            if j == depth - 1:
                sides.append("+z")
            for side in sides:
                if side == "-z" and i == 0 and door_side == "front":
                    model = "wall-wood-door"
                elif side in ("+x", "-x") and j % 2 == 1:
                    model = "wall-wood-window-shutters"
                elif side == "+z" and i == width - 1:
                    model = "wall-wood-window-small"
                else:
                    model = "wall-wood"
                scene.model(model, town_parts(model), group, (cx, 0, cz), faces[side])
            if width == 2:
                roof_yaw = 0 if i == 0 else 180
                scene.model("roof-high", town_parts("roof-high"), group, (cx, 1, cz), roof_yaw)
    return group


def build_scene(kits, terrain, sky_id):
    nature, town, survival = kits["nature"], kits["town"], kits["survival"]
    scene = Scene()

    # Environment.
    env = scene.add("Environment")
    sun_dir = np.array([0.55, -0.62, -0.56])
    scene.add("Sun", env, rotation=quat_facing(sun_dir), **{C_SUN: {
        "cast_shadows": True, "color": [255, 244, 226, 255], "depth_bias": 1.0,
        "intensity": 2.6, "normal_bias": 1.0, "shadow_distance": 70.0}})
    scene.add("Sky", env, **{C_SKY: {"intensity": 1.0, "skybox": sky_id}})
    scene.add("Post Process", env, **{
        C_VOLUME: {"priority": 0},
        C_TONEMAP: {"exposure_ev": 0.2, "preset": "Aces"},
        C_BLOOM: {"intensity": 0.12, "knee": 0.5, "radius": 6, "threshold": 1.2},
        C_AO: {"bias": 0.025, "intensity": 0.7, "radius": 0.8, "sample_count": 16},
        C_GRADING: {"contrast": 1.04, "saturation": 1.12, "temperature": 0.04, "tint": 0.0},
        C_VIGNETTE: {"color": [0, 0, 0, 255], "intensity": 0.22, "smoothness": 0.5},
        C_FXAA: {"edge_threshold": 0.166, "subpixel": 0.75},
    })

    # Terrain.
    ground = scene.add("Terrain")
    mesh = lambda m, mat, shadows=True: {C_MESH: {"cast_shadows": shadows, "material": mat, "mesh": m}}
    scene.add("Ground", ground, **mesh(terrain["ground"], material("ground", PALETTE["ground"]), False))
    scene.add("Paths and Dirt", ground, **mesh(terrain["dirt"], material("dirt", PALETTE["dirt"]), False))
    scene.add("Mountains", ground, **mesh(terrain["rock"], material("mountain", PALETTE["mountain"]), False))
    scene.add("Mountain Snow", ground, **mesh(terrain["snow"], material("snow", PALETTE["snow"], roughness=0.7), False))

    # Houses: the longhouse on the right, one on the left, one at the back,
    # and the thatched one at the back right.
    village = scene.add("Village")
    scene.layout_groups.add(village)
    wood_roof = kits["town_wood"]
    thatch_roof = kits["town_thatch"]
    houses = scene.add("Houses", village)
    scene.layout_groups.add(houses)
    house(scene, kits, houses, "Longhouse", (12.5, 0, 9), -90, 2, 3, wood_roof, 3.4)
    house(scene, kits, houses, "West House", (-15, 0, -4), 90, 2, 3, wood_roof, 3.0)
    house(scene, kits, houses, "Hall", (1.5, 0, -27), 180, 2, 4, wood_roof, 3.2)
    house(scene, kits, houses, "Thatched House", (22, 0, -20), -130, 2, 3, thatch_roof, 3.0)
    house(scene, kits, houses, "Storehouse", (-19, 0, -22), 145, 2, 2, thatch_roof, 2.6)

    # The well shelter by the path on the left.
    well = scene.add("Well", village, (-8.5, 0, 13), quat_yaw(20), 2.4)
    for dx, dz in ((-0.45, -0.45), (0.45, -0.45), (-0.45, 0.45), (0.45, 0.45)):
        scene.model("Post", [(m, wood_roof) for m, _ in town.model("pillar-wood")], well, (dx, 0, dz))
    scene.model("Roof", [(m, wood_roof) for m, _ in town.model("roof-high-gable")], well, (0, 1, 0), 90)
    scene.model("Bucket", survival.model("bucket"), well, (0.15, 0, 0.1), 0, 2.0)
    scene.model("Well Pit", survival.model("campfire-pit"), well, (0, 0, 0), 0, 2.4)

    # Props around the square.
    props = scene.add("Props", village)
    scene.layout_groups.add(props)
    sv = lambda name, model, pos, yaw=0.0, s=3.0: scene.model(name, survival.model(model), props, pos, yaw, s)
    tw = lambda name, model, pos, yaw=0.0, s=3.0: scene.model(name, [(m, wood_roof) for m, _ in town.model(model)], props, pos, yaw, s)
    nt = lambda name, model, pos, yaw=0.0, s=3.0: scene.model(name, nature.model(model), props, pos, yaw, s)
    tw("Table", "stall-bench", (8.2, 0, 15.5), 0, 3.6)
    tw("Bench", "stall-bench", (7.0, 0, 15.5), 0, 2.4)
    tw("Bench", "stall-bench", (9.4, 0, 15.5), 0, 2.4)
    for k, (x, z) in enumerate([(7.6, 4.0), (8.2, 4.6), (7.2, 5.0), (-10.0, -9.0), (19.0, -14.5)]):
        sv("Barrel", "barrel", (x, 0, z), k * 40)
    for k, (x, z, yaw) in enumerate([(8.0, 1.8, 10), (8.6, 2.6, 40), (-11.2, -8.0, 0), (-4.0, -19.0, 20)]):
        sv("Crate", "box-large", (x, 0, z), yaw)
    sv("Crate", "box", (8.3, 0.75, 2.2), 30)
    sv("Chest", "chest", (17.5, 0, -12.5), -40)
    tw("Cart", "cart", (16.0, 0, -6.0), -60, 3.2)
    tw("Market Stall", "stall", (-6.5, 0, -12.0), 60, 3.2)
    tw("Banner", "banner-red", (-6.0, 0, -15.0), 60, 3.0)
    for k, (x, z) in enumerate([(3.0, 20.0), (-3.0, 8.0), (5.0, -6.0), (-15.0, 8.5)]):
        nt("Sign", "sign", (x, 0, z), k * 30 + 15, 3.0)
    sv("Signpost", "signpost", (-3.0, 0, 3.0), 30, 4.0)
    sv("Workbench", "workbench", (-12.0, 0, 3.5), 80, 3.4)
    sv("Wood Pile", "resource-wood", (-12.5, 0, 5.2), 10, 3.4)
    sv("Campfire", "campfire-pit", (3.5, 0, -14.5), 0, 3.4)
    nt("Log Stack", "log_stack", (19.5, 0, 4.0), 90, 4.0)
    nt("Log Stack", "log_stack", (-21.0, 0, -9.0), 10, 4.0)
    nt("Log", "log", (5.5, 0, 9.0), 70, 4.0)
    nt("Stump", "stump_round", (-6.0, 0, 18.0), 0, 4.0)
    nt("Stump", "stump_round", (14.0, 0, 20.0), 0, 4.0)
    for k, (x, z) in enumerate([(10.4, 16.8), (6.2, 13.8), (-13.5, 1.0), (21.0, -9.0)]):
        nt("Pot", "pot_large", (x, 0, z), k * 50, 1.6)

    # Fences along the village's edges.
    fences = scene.add("Fences", village)
    scene.layout_groups.add(fences)
    fence_runs = [((-30, 18), (-30, -8)), ((26, 22), (30, -4)), ((-12, -34), (16, -36)),
                  ((30, -8), (30, -26))]
    for (ax, az), (bx, bz) in fence_runs:
        length = math.hypot(bx - ax, bz - az)
        yaw = math.degrees(math.atan2(-(bz - az), bx - ax))
        n = int(length / 3.0)
        for k in range(n):
            t = (k + 0.5) / n
            model = "fence_simple" if k % 4 else "fence_planks"
            scene.model("Fence", nature.model(model), fences,
                        (ax + (bx - ax) * t, 0, az + (bz - az) * t + 1.5), yaw, 3.0)

    # Plank walkways over the dirt paths, boards across the path.
    walkways = scene.add("Walkways", village)
    scene.layout_groups.add(walkways)
    for path in PATHS:
        points = densify(path, 2.0)
        for k in range(len(points) - 1):
            (ax, az), (bx, bz) = points[k], points[k + 1]
            if hash2(k, len(path), 21) < 0.18:
                continue  # a missing board here and there, as in the original
            yaw = math.degrees(math.atan2(-(bz - az), bx - ax)) + 90
            yaw += (hash2(k, 3, 22) - 0.5) * 12
            scene.model("Planks", nature.model("path_wood"), walkways,
                        ((ax + bx) / 2, 0.05, (az + bz) / 2), yaw, 4.0, shadows=False)

    # Nature.
    green = scene.add("Nature")
    scene.layout_groups.add(green)
    trees = scene.add("Trees", green)
    scene.layout_groups.add(trees)
    pines = ["tree_pineTallA", "tree_pineTallB", "tree_pineDefaultA", "tree_pineRoundC"]
    leafy = ["tree_default", "tree_oak", "tree_detailed", "tree_fat"]
    occupied = [(12.5, 9, 9), (-15, -4, 8), (1.5, -27, 10), (22, -20, 8), (-19, -22, 7), (-8.5, 13, 3)]

    # The camera's flight in play, sampled along the same Catmull-Rom curve
    # `demos::camera_path::sample_path` flies.
    stops = [np.array(pos) for pos, _ in CAMERA_PATH]
    flight = []
    for k in range(len(stops)):
        p0, p1, p2, p3 = (stops[(k + o) % len(stops)] for o in (-1, 0, 1, 2))
        for u in np.linspace(0, 1, 24, endpoint=False):
            point = 0.5 * (2 * p1 + (p2 - p0) * u + (2 * p0 - 5 * p1 + 4 * p2 - p3) * u * u
                           + (3 * p1 - p0 - 3 * p2 + p3) * u ** 3)
            flight.append((point[0], point[2]))

    def free(x, z, margin=2.5, tall=False):
        for path in PATHS:
            for (ax, az), (bx, bz) in zip(path, path[1:]):
                vx, vz = bx - ax, bz - az
                t = max(0.0, min(1.0, ((x - ax) * vx + (z - az) * vz) / (vx * vx + vz * vz)))
                if math.hypot(x - ax - t * vx, z - az - t * vz) < margin + 2.2:
                    return False
        if not all(math.hypot(x - ox, z - oz) > r + margin for ox, oz, r in occupied):
            return False
        # Nothing tall on the camera's flight line.
        return not tall or all(math.hypot(x - px, z - pz) > 10.0 for px, pz in flight)

    placed, attempts = 0, 0
    while placed < 130 and attempts < 20000:
        attempts += 1
        angle = RNG.uniform(0, 2 * math.pi)
        r = RNG.uniform(45, 130) if placed < 90 else RNG.uniform(18, 55)
        x, z = r * math.cos(angle), r * math.sin(angle)
        if z > 10 and abs(x) < 12 + (z - 10) * 0.6:
            continue  # keep the framing camera's view into the village open
        if not free(x, z, 3.0, tall=True):
            continue
        model = RNG.choice(pines if RNG.random() < 0.65 else leafy)
        size = RNG.uniform(4.5, 7.5) if model in pines else RNG.uniform(4.0, 6.0)
        scene.model("Pine" if model in pines else "Tree", nature.model(model), trees,
                    (x, height(x, z) - 0.1, z), RNG.uniform(0, 360), size)
        placed += 1
        occupied.append((x, z, 1.5))

    rocks = scene.add("Rocks", green)
    scene.layout_groups.add(rocks)
    for x, z, model, size in [(10, -2, "stone_largeB", 3.5), (-22, 14, "stone_largeA", 4.0),
                              (24, 14, "stone_tallA", 3.0), (-4, -24, "stone_largeC", 3.0),
                              (16, 26, "stone_largeD", 4.5), (-26, -14, "stone_largeE", 4.0),
                              (34, -30, "stone_tallB", 5.0), (-36, 26, "stone_largeF", 6.0),
                              (5, 24, "stone_smallA", 3.0), (-3, -9, "stone_smallC", 3.0)]:
        scene.model("Rock", nature.model(model), rocks, (x, height(x, z), z), RNG.uniform(0, 360), size)

    foliage = scene.add("Foliage", green)
    scene.layout_groups.add(foliage)
    tufts = ["grass_large", "grass", "plant_flatTall", "grass_leafsLarge", "plant_bushLarge"]
    flowers = ["flower_yellowA", "flower_purpleA", "flower_redA", "flower_yellowB"]
    count = 0
    while count < 220:
        x, z = RNG.uniform(-40, 40), RNG.uniform(-40, 42)
        if not free(x, z, -0.8):
            continue
        model = RNG.choice(flowers) if count % 6 == 0 else RNG.choice(tufts)
        size = RNG.uniform(3.0, 5.0)
        scene.model("Flowers" if model in flowers else "Grass", nature.model(model), foliage,
                    (x, height(x, z), z), RNG.uniform(0, 360), size, shadows=False)
        count += 1

    # The framed camera, and the path it flies in play.
    stops = scene.add("Camera Path")
    scene.layout_groups.add(stops)
    path = CAMERA_PATH
    for k, (pos, target) in enumerate(path):
        scene.add(f"Stop {k + 1}", stops, pos, quat_facing(np.subtract(target, pos)))
    pos, target = path[0]
    scene.add("Camera", None, pos, quat_facing(np.subtract(target, pos)), **{
        C_CAMERA: {"clear_color": [135, 190, 230, 255], "far_plane": 1200.0, "fov": 1.0,
                   "near_plane": 0.1},
        C_CAMERA_PATH: {"waypoints": stops, "loop_seconds": 70.0},
    })
    return scene


def main():
    global OUT
    parser = argparse.ArgumentParser()
    parser.add_argument("--nature", required=True)
    parser.add_argument("--town", required=True)
    parser.add_argument("--survival", required=True)
    parser.add_argument("--sky", required=True)
    parser.add_argument("--out", default=OUT)
    args = parser.parse_args()
    OUT = args.out

    subdirs = ("models", "materials", "textures", "sky")
    before = {os.path.join(root, f) for sub in subdirs
              for root, _, files in os.walk(os.path.join(OUT, sub)) for f in files}
    for sub in subdirs:
        os.makedirs(os.path.join(OUT, sub), exist_ok=True)

    textures = {}
    for name, src, roof in [
        ("town_wood", os.path.join(args.town, "Textures", "colormap.png"), "#6e5644"),
        ("town_thatch", os.path.join(args.town, "Textures", "colormap.png"), "#c9a868"),
        ("survival", os.path.join(args.survival, "Textures", "colormap.png"), None),
    ]:
        path = os.path.join(OUT, "textures", name)
        regrade_atlas(src, path + ".png", roof)
        WRITTEN.add(path + ".png")
        textures[name] = write_meta(path, f"{ASSET_PREFIX}/textures/{name}", TYPE_TEXTURE)

    sky_name = os.path.splitext(os.path.basename(args.sky))[0]
    shutil.copyfile(args.sky, os.path.join(OUT, "sky", os.path.basename(args.sky)))
    WRITTEN.add(os.path.join(OUT, "sky", os.path.basename(args.sky)))
    sky_id = write_meta(os.path.join(OUT, "sky", sky_name), f"{ASSET_PREFIX}/sky/{sky_name}", TYPE_SKYBOX)

    p = lambda key, roughness=0.9: material(key, PALETTE[key], roughness=roughness)
    nature_palette = {
        "leafsDark": p("pine"), "leafsGreen": p("leaves"), "woodBarkDark": p("bark"),
        "woodBark": p("bark"), "woodInner": p("wood"), "wood": p("wood"),
        "woodDark": p("wood_dark"), "grass": p("grass"), "stone": p("stone"),
        "_defaultMat": p("stone"), "dirt": p("dirt"), "colorYellow": p("flower_yellow"),
        "colorPurple": p("flower_blue"), "colorRed": p("flower_red"),
        ("path_wood", "dirt"): p("plank"), ("path_wood", "dirtDark"): p("wood_dark"),
        ("tree_detailed", "_defaultMat"): p("bark"), ("sign", "_defaultMat"): p("wood"),
    }
    kits = {
        "nature": Kit("nature", args.nature, palette=nature_palette),
        "town": Kit("town", args.town, atlas_material="placeholder"),
        "survival": Kit("survival", args.survival,
                        atlas_material=material("survival_atlas", texture=textures["survival"])),
        "town_wood": material("town_wood", texture=textures["town_wood"]),
        "town_thatch": material("town_thatch", texture=textures["town_thatch"]),
    }
    kits["town"].atlas_material = kits["town_wood"]

    terrain = {"ground": build_terrain(), "dirt": build_dirt()}
    terrain["rock"], terrain["snow"] = build_mountains()

    scene = build_scene(kits, terrain, sky_id)
    scene_path = os.path.join(OUT, "rpg_poly")
    scene.save(scene_path + ".cxscene")
    write_meta(scene_path, f"{ASSET_PREFIX}/rpg_poly", TYPE_SCENE)
    # Files are rewritten in place, so an open editor sees changes, not
    # deletions; only what this run no longer writes goes.
    after = {os.path.join(root, f) for sub in subdirs
             for root, _, files in os.walk(os.path.join(OUT, sub)) for f in files}
    written = {os.path.normpath(p) for p in WRITTEN}
    for stale in sorted(before & after):
        if os.path.normpath(stale) not in written:
            os.remove(stale)
    print(f"{len(scene.components)} objects, {len(MESHES)} meshes, {len(MATERIALS)} materials")


if __name__ == "__main__":
    main()
