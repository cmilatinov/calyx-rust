"""Imports RPG Poly Pack - Lite into the demos project.

The pack's licence (the Standard Unity Asset Store EULA) does not allow its
files in a public repository, so none of them are committed. This script
builds them, from your own copy of the pack, into the gitignored
`demos/assets/rpg_poly/vendor/`:

- every model, converted from FBX to OBJ by headless Blender
  (`blender_convert.py` says how the axes are converted);
- the textures, converted from TGA to PNG;
- the three materials, converted from Unity's to Calyx's PBR material;
- a `.meta` file for each, with ids derived from the asset names, so the
  committed scene finds them again after every import.

With `--scene` it also regenerates the committed scene,
`demos/assets/rpg_poly/rpg_poly.cxscene`, from the pack's demo scene
(`rpgpp_lt_scene_1.0.unity`): every prefab instance and group in the
original hierarchy, with its name and transform, plus the sun, the camera
and the camera's flythrough. Unity and Calyx are both left-handed and Y-up
with +Z forward, so positions and rotations carry over unchanged.

Usage, from the repository root (the pack downloads through the Unity
editor's Package Manager into the Asset Store cache, the default path):

    python demos/tools/rpg_poly/import_pack.py [--package <.unitypackage>]
        [--blender <blender.exe>]

Needs Python 3 and Blender 4.x, nothing else.
"""

import argparse
import json
import math
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import unity_yaml  # noqa: E402

DEFAULT_PACKAGE = os.path.join(
    os.environ.get("APPDATA", ""), "Unity", "Asset Store-5.x", "Gigel3d",
    "3D ModelsEnvironmentsLandscapes", "RPG Poly Pack - Lite.unitypackage")
DEFAULT_BLENDER = r"C:\Program Files\Blender Foundation\Blender 4.0\blender.exe"

ROOT = os.path.join("demos", "assets", "rpg_poly")
VENDOR = os.path.join(ROOT, "vendor")
SCENE = os.path.join(ROOT, "rpg_poly.cxscene")
ASSET_PREFIX = "rpg_poly"
NAMESPACE = uuid.UUID("5d7c1a3e-4f0b-4b8e-9a61-413413413413")
DEMO_SCENE = "Assets/RPGPP_LT/Scene/rpgpp_lt_scene_1.0.unity"
# The material a model file's own renderer uses: the pack's one atlas material.
DEFAULT_MATERIAL = "58d53420af384224ea4cfee8973d18cb"

TYPE_MESH = "792d264b-de6f-4431-b59f-76f18fdb3bfe"
TYPE_TEXTURE = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2"
TYPE_MATERIAL = "f98a7f41-84d4-482d-b7af-a670b07035ae"
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

# The committed sky: Poly Haven's Kloofendal 48d Partly Cloudy (CC0) lights
# the scene's ambient, standing in for the procedural sky Unity's ambient
# comes from. The pack's own sky dome and clouds are drawn over it.
SKYBOX_ID = "b54b855c-1db7-5cdc-9293-e19639776142"  # sky/kloofendal_48d_partly_cloudy_puresky_2k.meta


def asset_id(name):
    return str(uuid.uuid5(NAMESPACE, name))


def write_meta(path_no_ext, name, type_uuid):
    meta = {"main": {"id": asset_id(name), "name": name,
                     "display_name": name.rsplit("/", 1)[-1], "type_uuid": type_uuid},
            "inner": []}
    with open(path_no_ext + ".meta", "w", newline="\n") as f:
        json.dump(meta, f, indent=2)
    return meta["main"]["id"]


def stem(path):
    return os.path.splitext(os.path.basename(path))[0]


def mesh_name(fbx_path):
    return f"{ASSET_PREFIX}/vendor/models/{stem(fbx_path)}"


def material_name(mat_path):
    return f"{ASSET_PREFIX}/vendor/materials/{stem(mat_path)}"


def texture_name(tex_path):
    return f"{ASSET_PREFIX}/vendor/textures/{stem(tex_path)}"


# --------------------------------------------------------------------------
# Package


class Package:
    """The `.unitypackage`: a gzipped tar of `<guid>/pathname`, `<guid>/asset`
    and `<guid>/asset.meta`."""

    def __init__(self, path):
        self.paths, self.assets, self.metas = {}, {}, {}
        with tarfile.open(path, "r:gz") as tar:
            for member in tar.getmembers():
                if not member.isfile():
                    continue
                guid, _, leaf = member.name.partition("/")
                data = tar.extractfile(member).read()
                if leaf == "pathname":
                    self.paths[guid] = data.decode("utf-8").splitlines()[0].strip()
                elif leaf == "asset":
                    self.assets[guid] = data
                elif leaf == "asset.meta":
                    self.metas[guid] = data.decode("utf-8")
        self.guids = {path: guid for guid, path in self.paths.items()}

    def with_extension(self, extension):
        return sorted((path, guid) for guid, path in self.paths.items()
                      if path.lower().endswith(extension) and guid in self.assets)

    def documents(self, guid):
        return unity_yaml.parse_documents(self.assets[guid].decode("utf-8"))


# --------------------------------------------------------------------------
# Materials


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def write_material(path_no_ext, name, *, texture=None, color=(1, 1, 1), roughness=0.85,
                   metallic=0.0, emissive_texture=None, emission=None, unlit=False):
    """A Calyx PBR material. `unlit` keeps a Unity Unlit material's look: no
    lit base colour, its colour or texture emitted as is."""
    base = [srgb_to_linear(c) for c in color] + [1.0]
    texture_value = {"Asset": texture} if texture else {"Color": [1.0, 1.0, 1.0, 1.0]}
    emissive_value = {"Asset": emissive_texture} if emissive_texture else {"Color": [1.0, 1.0, 1.0, 1.0]}
    emission_color = [1.0, 1.0, 1.0, 1.0]
    strength = 0.0
    if unlit:
        emission_color = base
        strength = 1.0
        base = [0.0, 0.0, 0.0, 1.0]
    elif emission:
        emission_color = [srgb_to_linear(c) for c in emission] + [1.0]
        strength = 1.0
    variables = [
        ("base_color_texture", 0, None, None, {"Texture2D": texture_value}),
        ("metallic", 2, 0, 4, {"Float": metallic}),
        ("roughness", 2, 4, 4, {"Float": roughness}),
        ("ambient_occlusion", 2, 8, 4, {"Float": 1.0}),
        ("emission_strength", 2, 12, 4, {"Float": strength}),
        ("emission_color", 2, 16, 16, {"Color": [round(c, 5) for c in emission_color]}),
        ("base_color_factor", 2, 32, 16, {"Color": [round(c, 5) for c in base]}),
        ("emissive_texture", 7, None, None, {"Texture2D": emissive_value}),
    ]
    data = {"shader": PBR_SHADER, "variables": [
        {"group": 3, "binding": binding, "offset": offset, "name": var, "span": span, "value": value}
        for var, binding, offset, span, value in variables]}
    with open(path_no_ext + ".cxmat", "w", newline="\n") as f:
        json.dump(data, f, indent=2)
    return write_meta(path_no_ext, name, TYPE_MATERIAL)


def unity_material(package, guid):
    """(texture guid, colour, smoothness, metallic, emission colour, shader file id)."""
    body = package.documents(guid)[0][4]
    props = body["m_SavedProperties"]
    textures = {k: v for entry in props.get("m_TexEnvs") or [] for k, v in entry.items()}
    floats = {k: float(v) for entry in props.get("m_Floats") or [] for k, v in entry.items()}
    colors = {k: v for entry in props.get("m_Colors") or [] for k, v in entry.items()}
    main = (textures.get("_MainTex") or {}).get("m_Texture") or {}
    color = colors.get("_Color", {"r": 1, "g": 1, "b": 1})
    emission = colors.get("_EmissionColor", {"r": 0, "g": 0, "b": 0})
    keywords = body.get("m_ShaderKeywords") or ""
    return {
        "texture": main.get("guid"),
        "color": tuple(float(color[c]) for c in "rgb"),
        "smoothness": floats.get("_Glossiness", 0.5),
        "metallic": floats.get("_Metallic", 0.0),
        "emission": tuple(float(emission[c]) for c in "rgb") if "_EMISSION" in keywords else None,
        "shader": int(body["m_Shader"]["fileID"]),
    }


# Unity's built-in Unlit shaders (Unlit/Texture, Unlit/Color and the rest).
UNLIT_SHADERS = range(10750, 10760)


# --------------------------------------------------------------------------
# Fog

# The sky dome's texture, and the dome's radius in the demo scene: its mesh
# spans about 49 m, and the scene scales it by 10.
SKY_TEXTURE = "5c4fa0e44769f8f46a06d96031e56a4e"
SKY_DOME_RADIUS = 490.0


def sky_fog(package):
    """The demo scene's linear fog where it meets the sky dome, as [r, g, b,
    fraction]. Unity draws the dome through the scene's fog, which washes its
    saturated teal out to the pale blue of the store screenshots. Calyx has no
    fog, and the dome sits at a near-constant distance from the village, so
    the fog's effect on it is baked into its texture instead."""
    settings = next(body for _, _, _, name, body in package.documents(package.guids[DEMO_SCENE])
                    if name == "RenderSettings")
    if settings.get("m_Fog") != "1":
        return None
    color = settings["m_FogColor"]
    start, end = float(settings["m_LinearFogStart"]), float(settings["m_LinearFogEnd"])
    fraction = min(max((SKY_DOME_RADIUS - start) / (end - start), 0.0), 1.0)
    return [float(color["r"]), float(color["g"]), float(color["b"]), fraction]


# --------------------------------------------------------------------------
# Normals

# Unity's ModelImporterNormals values, as `normalImportMode` stores them.
NORMALS_IMPORT, NORMALS_CALCULATE, NORMALS_NONE = 0, 1, 2


def normal_settings(meta_text):
    """(mode, smoothing angle) of a model's normals from its `.meta`. None
    becomes Calculate, as Unity does for a lit shader; a meta without the
    settings gets Unity's defaults, Import and 60 degrees."""
    importer = unity_yaml.parse_meta(meta_text).get("ModelImporter") or {}
    tangent_space = importer.get("tangentSpace") or {}
    mode = int(tangent_space.get("normalImportMode", NORMALS_IMPORT))
    angle = float(tangent_space.get("normalSmoothAngle", 60))
    if mode == NORMALS_NONE:
        mode = NORMALS_CALCULATE
    return mode, angle


def check_faceted(obj_path):
    """Fails the import unless the OBJ at `obj_path` is faceted the way a
    model whose normals Unity calculates with a smoothing angle of 0 is:
    every triangle's corners carry the triangle's own face normal, facing the
    way its (counter-clockwise) winding does. A smooth-shaded mesh, or one
    whose normals point against its winding, fails."""
    positions, normals = [], []
    with open(obj_path) as f:
        for line in f:
            parts = line.split()
            if not parts:
                continue
            if parts[0] == "v":
                positions.append([float(c) for c in parts[1:4]])
            elif parts[0] == "vn":
                normals.append([float(c) for c in parts[1:4]])
            elif parts[0] == "f":
                corners = [c.split("/") for c in parts[1:]]
                a, b, c = (positions[int(k[0]) - 1] for k in corners)
                u = [b[i] - a[i] for i in range(3)]
                v = [c[i] - a[i] for i in range(3)]
                face = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
                length = math.sqrt(sum(x * x for x in face))
                if length < 1e-9:
                    continue  # a degenerate triangle has no face normal to compare
                for k in corners:
                    n = normals[int(k[2]) - 1]
                    if sum(face[i] * n[i] for i in range(3)) / length < 0.999:
                        raise SystemExit(f"{obj_path}: a corner normal is not its triangle's face normal")


# --------------------------------------------------------------------------
# Import


def import_assets(package, blender):
    for sub in ("models", "textures", "materials"):
        shutil.rmtree(os.path.join(VENDOR, sub), ignore_errors=True)
        os.makedirs(os.path.join(VENDOR, sub), exist_ok=True)
    with open(os.path.join(VENDOR, "README.md"), "w", newline="\n") as f:
        f.write("Built by `demos/tools/rpg_poly/import_pack.py` from RPG Poly Pack - Lite.\n"
                "The pack's licence does not allow these files in git; this folder is ignored.\n")

    work = tempfile.mkdtemp(prefix=".import-", dir=VENDOR)
    try:
        job = {"meshes": [], "textures": []}
        # Each meta goes beside its file once the file exists, so an open
        # editor never sees a meta without its asset.
        metas = []
        for path, guid in package.with_extension(".fbx"):
            fbx = os.path.join(work, guid + ".fbx")
            with open(fbx, "wb") as f:
                f.write(package.assets[guid])
            obj = os.path.join(VENDOR, "models", stem(path))
            mode, angle = normal_settings(package.metas.get(guid, ""))
            job["meshes"].append([os.path.abspath(fbx), os.path.abspath(obj + ".obj"), mode, angle])
            metas.append((obj, mesh_name(path), TYPE_MESH))
        for path, guid in package.with_extension(".tga"):
            tga = os.path.join(work, guid + ".tga")
            with open(tga, "wb") as f:
                f.write(package.assets[guid])
            png = os.path.join(VENDOR, "textures", stem(path))
            job["textures"].append([os.path.abspath(tga), os.path.abspath(png + ".png"),
                                    sky_fog(package) if guid == SKY_TEXTURE else None])
            metas.append((png, texture_name(path), TYPE_TEXTURE))
        job_path = os.path.join(work, "job.json")
        with open(job_path, "w") as f:
            json.dump(job, f)
        script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "blender_convert.py")
        result = subprocess.run([blender, "--background", "--factory-startup", "--python", script,
                                 "--", os.path.abspath(job_path)], capture_output=True, text=True)
        converted = sum(1 for line in result.stdout.splitlines() if line.startswith("converted "))
        if result.returncode != 0 or converted != len(job["meshes"]) + len(job["textures"]):
            sys.stderr.write(result.stdout[-4000:] + result.stderr[-4000:])
            raise SystemExit("Blender conversion failed")
        for fbx, obj, mode, angle in job["meshes"]:
            if mode != NORMALS_IMPORT and angle == 0:
                check_faceted(obj)
        for args in metas:
            write_meta(*args)
    finally:
        shutil.rmtree(work, ignore_errors=True)

    for path, guid in package.with_extension(".mat"):
        mat = unity_material(package, guid)
        texture = asset_id(texture_name(package.paths[mat["texture"]])) if mat["texture"] else None
        target = os.path.join(VENDOR, "materials", stem(path))
        if mat["shader"] in UNLIT_SHADERS:
            # Unlit/Texture shows the texture as is; Unlit/Color its colour.
            if texture:
                write_material(target, material_name(path), color=(0, 0, 0), roughness=1.0,
                               emissive_texture=texture, emission=(1, 1, 1))
            else:
                write_material(target, material_name(path), color=mat["color"], roughness=1.0,
                               unlit=True)
        else:
            write_material(target, material_name(path), texture=texture, color=mat["color"],
                           roughness=1.0 - mat["smoothness"], metallic=mat["metallic"],
                           emission=mat["emission"])
    print(f"imported {len(job['meshes'])} models, {len(job['textures'])} textures and "
          f"{len(package.with_extension('.mat'))} materials into {VENDOR}")


# --------------------------------------------------------------------------
# Scene


def q(d):
    return [float(d["x"]), float(d["y"]), float(d["z"]), float(d["w"])]


def v3(d):
    return [float(d["x"]), float(d["y"]), float(d["z"])]


class Placed:
    """One of the pack's objects as a prefab or prefab instance leaves it:
    every prefab in the pack is a single object with one mesh."""

    def __init__(self, name, mesh, material, position, rotation, scale, active, shadows):
        self.name, self.mesh, self.material = name, mesh, material
        self.position, self.rotation, self.scale = position, rotation, scale
        self.active, self.shadows = active, shadows

    def copy(self):
        return Placed(self.name, self.mesh, self.material, list(self.position),
                      list(self.rotation), list(self.scale), self.active, self.shadows)

    def apply(self, modifications):
        """Applies a prefab instance's modifications. Each prefab holds one
        object, so every transform modification is its root's."""
        for mod in modifications or []:
            prop, value = mod["propertyPath"], mod.get("value")
            field, _, axis = prop.partition(".")
            target = {"m_LocalPosition": self.position, "m_LocalRotation": self.rotation,
                      "m_LocalScale": self.scale}.get(field)
            if target is not None and axis in ("x", "y", "z", "w"):
                target["xyzw".index(axis)] = float(value)
            elif prop == "m_Name":
                self.name = value
            elif prop == "m_IsActive":
                self.active = value != "0"
            elif prop == "m_CastShadows":
                self.shadows = value != "0"
            elif prop == "m_Materials.Array.data[0]":
                self.material = (mod.get("objectReference") or {}).get("guid")


def resolve_prefab(package, guid, cache):
    """The object a prefab, prefab variant or model file (FBX) places."""
    if guid in cache:
        return cache[guid].copy()
    path = package.paths[guid]
    if path.lower().endswith(".fbx"):
        placed = Placed(stem(path), guid, None, [0, 0, 0], [0, 0, 0, 1], [1, 1, 1], True, True)
    else:
        docs = {name: body for _, _, _, name, body in package.documents(guid)}
        if "PrefabInstance" in docs:
            instance = docs["PrefabInstance"]
            placed = resolve_prefab(package, instance["m_SourcePrefab"]["guid"], cache)
            placed.apply(instance["m_Modification"].get("m_Modifications"))
        else:
            transform, renderer = docs["Transform"], docs.get("MeshRenderer") or {}
            materials = renderer.get("m_Materials") or []
            placed = Placed(docs["GameObject"]["m_Name"], docs["MeshFilter"]["m_Mesh"].get("guid"),
                            materials[0].get("guid") if materials else None,
                            v3(transform["m_LocalPosition"]), q(transform["m_LocalRotation"]),
                            v3(transform["m_LocalScale"]), docs["GameObject"].get("m_IsActive") != "0",
                            renderer.get("m_CastShadows", "1") != "0")
    cache[guid] = placed
    return placed.copy()


class SceneWriter:
    def __init__(self):
        self.components, self.hierarchy, self.roots = {}, {}, []

    def add(self, key, name, parent, position, rotation, scale, components=None, visible=True):
        object_id = str(uuid.uuid5(NAMESPACE, "unity:" + key))
        data = {C_ID: {"id": object_id, "name": name, "visible": visible},
                C_TRANSFORM: {"transform": {"position": position, "rotation": rotation, "scale": scale}}}
        data.update(components or {})
        self.components[object_id] = data
        (self.hierarchy.setdefault(parent, []) if parent else self.roots).append(object_id)
        return object_id


def build_scene(package):
    scene_guid = package.guids[DEMO_SCENE]
    docs = package.documents(scene_guid)
    by_id = {file_id: (name, body, stripped) for _, file_id, stripped, name, body in docs}
    prefabs = {}
    mesh_ids = {guid: asset_id(mesh_name(path)) for path, guid in package.with_extension(".fbx")}
    material_ids = {guid: asset_id(material_name(path)) for path, guid in package.with_extension(".mat")}
    writer = SceneWriter()
    counts = {"prefab instances": 0, "meshes": 0, "groups": 0}

    def transform_values(body):
        return v3(body["m_LocalPosition"]), q(body["m_LocalRotation"]), v3(body["m_LocalScale"])

    # The stripped transform each prefab instance's root shows the scene as.
    instance_of_stripped = {}
    for file_id, (name, body, stripped) in by_id.items():
        if name == "Transform" and stripped:
            instance_of_stripped[file_id] = int(body["m_PrefabInstance"]["fileID"])
    children_of = {}
    for file_id, (name, body, stripped) in by_id.items():
        if name == "PrefabInstance":
            parent = int(body["m_Modification"]["m_TransformParent"]["fileID"])
            children_of.setdefault(parent, []).append(("instance", file_id))
    for file_id, (name, body, stripped) in by_id.items():
        if name == "Transform" and not stripped:
            father = int(body["m_Father"]["fileID"])
            if father in instance_of_stripped:
                children_of.setdefault(father, []).append(("transform", file_id))
            for child in body.get("m_Children") or []:
                cid = int(child["fileID"])
                if cid in instance_of_stripped:
                    continue  # listed through its PrefabInstance
                children_of.setdefault(file_id, []).append(("transform", cid))

    cameras, sun = [], None

    def emit_instance(instance_id, parent):
        body = by_id[instance_id][1]
        placed = resolve_prefab(package, body["m_SourcePrefab"]["guid"], prefabs)
        placed.apply(body["m_Modification"].get("m_Modifications"))
        components = {}
        if placed.mesh:
            components[C_MESH] = {"cast_shadows": placed.shadows,
                                  "material": material_ids.get(placed.material or DEFAULT_MATERIAL),
                                  "mesh": mesh_ids[placed.mesh]}
            counts["meshes"] += 1
        counts["prefab instances"] += 1
        object_id = writer.add(str(instance_id), placed.name, parent, placed.position,
                               placed.rotation, placed.scale, components, visible=placed.active)
        for stripped_id, instance in instance_of_stripped.items():
            if instance == instance_id:
                for kind, child in children_of.get(stripped_id, []):
                    emit_child(kind, child, object_id)

    def emit_transform(transform_id, parent):
        nonlocal sun
        body = by_id[transform_id][1]
        go_id = int(body["m_GameObject"]["fileID"])
        go = by_id[go_id][1]
        name = go["m_Name"]
        position, rotation, scale = transform_values(body)
        kinds = {by_id[int(c["component"]["fileID"])][0]: by_id[int(c["component"]["fileID"])][1]
                 for c in go.get("m_Component") or []}
        if "Camera" in kinds:
            cameras.append((name, position, rotation, kinds["Camera"], go.get("m_IsActive") != "0"))
            return
        if "Light" in kinds:
            sun = (name, position, rotation, kinds["Light"])
            return
        counts["groups"] += 1
        object_id = writer.add(str(transform_id), name, parent, position, rotation, scale,
                               visible=go.get("m_IsActive") != "0")
        for kind, child in children_of.get(transform_id, []):
            emit_child(kind, child, object_id)

    def emit_child(kind, child, parent):
        if kind == "instance":
            emit_instance(child, parent)
        else:
            emit_transform(child, parent)

    roots = next(body for name, body, _ in by_id.values() if name == "SceneRoots")
    for entry in roots["m_Roots"]:
        rid = int(entry["fileID"])
        if rid in instance_of_stripped:
            emit_instance(instance_of_stripped[rid], None)
        else:
            emit_transform(rid, None)

    add_lighting(writer, sun, by_id)
    add_camera(writer, cameras)
    with open(SCENE, "w", newline="\n") as f:
        json.dump({"components": writer.components, "hierarchy": writer.hierarchy,
                   "roots": writer.roots}, f, indent=2)
    write_meta(os.path.splitext(SCENE)[0], f"{ASSET_PREFIX}/rpg_poly", TYPE_SCENE)
    print(f"wrote {SCENE}: {counts['prefab instances']} prefab instances, {counts['meshes']} meshes, "
          f"{counts['groups']} groups, {len(cameras)} cameras; the camera path has {ORBIT_STOPS + 1} stops")


def add_lighting(writer, sun, by_id):
    env = writer.add("environment", "Environment", None, [0, 0, 0], [0, 0, 0, 1], [1, 1, 1])
    name, position, rotation, light = sun
    color = [round(float(light["m_Color"][c]) * 255) for c in "rgb"] + [255]
    writer.add("sun", name, env, position, rotation, [1, 1, 1], {C_SUN: {
        "cast_shadows": light["m_Shadows"]["m_Type"] != "0", "color": color, "depth_bias": 1.0,
        "intensity": SUN_INTENSITY * float(light["m_Intensity"]), "normal_bias": 1.0,
        "shadow_distance": 120.0}})
    writer.add("sky", "Sky Light", env, [0, 0, 0], [0, 0, 0, 1], [1, 1, 1],
               {C_SKY: {"intensity": SKY_INTENSITY, "skybox": SKYBOX_ID}})
    writer.add("post", "Post Process", env, [0, 0, 0], [0, 0, 0, 1], [1, 1, 1], POST_PROCESS)


# Unity's directional light of intensity 1 under its default ambient, in
# Calyx's units; tuned by eye against the store screenshots.
SUN_INTENSITY = 3.0
SKY_INTENSITY = 0.9
POST_PROCESS = {
    C_VOLUME: {"priority": 0},
    C_TONEMAP: {"exposure_ev": 0.6, "preset": "Aces"},
    C_BLOOM: {"intensity": 0.08, "knee": 0.5, "radius": 6, "threshold": 1.5},
    C_AO: {"bias": 0.025, "intensity": 0.6, "radius": 0.8, "sample_count": 16},
    C_GRADING: {"contrast": 1.05, "saturation": 1.0, "temperature": -0.06, "tint": 0.0},
    C_VIGNETTE: {"color": [0, 0, 0, 255], "intensity": 0.15, "smoothness": 0.5},
    C_FXAA: {"edge_threshold": 0.166, "subpixel": 0.75},
}

# The flythrough: from the key art's camera it climbs onto a circle above the
# rooftops and treetops, circles the village looking at its centre, and comes
# back down. A loop through the pack's own screenshot cameras would cut
# through roofs and trees, since they stand among them.
ORBIT_CENTRE = (76.0, 16.0, 40.0)
ORBIT_RADIUS = 46.0
ORBIT_HEIGHT = 36.0
ORBIT_STOPS = 8
FRAMING_CAMERA = "Camera(KeyImage)"


def facing(position, target):
    """The rotation whose +Z looks from `position` at `target`, upright."""
    z = [t - p for t, p in zip(target, position)]
    length = math.sqrt(sum(c * c for c in z))
    z = [c / length for c in z]
    x = [z[2], 0.0, -z[0]]  # up x z
    length = math.sqrt(x[0] ** 2 + x[2] ** 2)
    x = [c / length for c in x]
    y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]]
    m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]]
    w = math.sqrt(max(0.0, 1 + m[0][0] + m[1][1] + m[2][2])) / 2
    if w > 1e-4:
        return [(m[2][1] - m[1][2]) / (4 * w), (m[0][2] - m[2][0]) / (4 * w),
                (m[1][0] - m[0][1]) / (4 * w), w]
    # A half turn: the axis is the column with the largest diagonal entry.
    i = max(range(3), key=lambda k: m[k][k])
    axis = [0.0, 0.0, 0.0]
    axis[i] = math.sqrt(max(0.0, (m[i][i] + 1) / 2))
    for k in range(3):
        if k != i:
            axis[k] = m[i][k] / (2 * axis[i])
    return axis + [0.0]


def add_camera(writer, cameras):
    by_name = {c[0]: c for c in cameras}
    path = writer.add("camera path", "Camera Path", None, [0, 0, 0], [0, 0, 0, 1], [1, 1, 1])
    name, position, rotation, camera, _ = by_name[FRAMING_CAMERA]
    writer.add("stop:" + name, name, path, position, rotation, [1, 1, 1])
    cx, cy, cz = ORBIT_CENTRE
    start = math.atan2(position[2] - cz, position[0] - cx)
    for k in range(ORBIT_STOPS):
        angle = start + 2 * math.pi * (k + 0.5) / ORBIT_STOPS
        stop = [cx + ORBIT_RADIUS * math.cos(angle), ORBIT_HEIGHT, cz + ORBIT_RADIUS * math.sin(angle)]
        writer.add(f"stop:orbit {k}", f"Orbit {k + 1}", path, [round(c, 3) for c in stop],
                   [round(c, 6) for c in facing(stop, ORBIT_CENTRE)], [1, 1, 1])
    fov_y = math.radians(float(camera["field of view"]))
    # Calyx's camera `fov` sets the horizontal angle; the store shots are 16:9.
    fov_x = 2 * math.atan(math.tan(fov_y / 2) * 16 / 9)
    writer.add("camera", "Camera", None, position, rotation, [1, 1, 1], {
        C_CAMERA: {"clear_color": [135, 190, 230, 255], "far_plane": 1500.0,
                   "fov": round(fov_x, 5), "near_plane": float(camera["near clip plane"])},
        C_CAMERA_PATH: {"path": path, "duration": 90.0, "looping": True},
    })


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--package", default=DEFAULT_PACKAGE)
    parser.add_argument("--blender", default=DEFAULT_BLENDER)
    parser.add_argument("--scene", action="store_true",
                        help="also regenerate the committed scene from the pack's demo scene")
    parser.add_argument("--skip-assets", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not os.path.isfile(args.package):
        raise SystemExit(f"RPG Poly Pack - Lite not found at {args.package}; download it through "
                         "the Unity editor's Package Manager (My Assets) or pass --package")
    if not os.path.isdir(ROOT):
        raise SystemExit("run this from the repository root")
    package = Package(args.package)
    if not args.skip_assets:
        if not os.path.isfile(args.blender):
            raise SystemExit(f"Blender not found at {args.blender}; pass --blender")
        import_assets(package, args.blender)
    if args.scene:
        build_scene(package)


if __name__ == "__main__":
    main()
