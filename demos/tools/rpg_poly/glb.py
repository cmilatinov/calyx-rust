"""Minimal GLB reader: node world matrices and primitive geometry."""
import json, struct
import numpy as np

CT = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
NC = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}

def load(path):
    b = open(path, 'rb').read()
    jl = struct.unpack('<I', b[12:16])[0]
    j = json.loads(b[20:20 + jl])
    off = 20 + jl
    bl = struct.unpack('<I', b[off:off + 4])[0]
    binc = b[off + 8: off + 8 + bl]
    return j, binc

def accessor(j, binc, i):
    a = j['accessors'][i]
    bv = j['bufferViews'][a['bufferView']]
    dt = np.dtype(CT[a['componentType']])
    n = NC[a['type']]
    start = bv.get('byteOffset', 0) + a.get('byteOffset', 0)
    stride = bv.get('byteStride', 0) or dt.itemsize * n
    count = a['count']
    raw = np.frombuffer(binc, dtype=np.uint8, count=stride * (count - 1) + dt.itemsize * n, offset=start)
    out = np.empty((count, n), dtype=dt)
    for k in range(count):
        out[k] = np.frombuffer(raw[k * stride: k * stride + dt.itemsize * n].tobytes(), dtype=dt)
    return out.astype(np.float64) if dt == np.float32 else out.astype(np.int64)

def quat_mat(q):
    x, y, z, w = q
    return np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])

def local(node):
    if 'matrix' in node:
        return np.array(node['matrix']).reshape(4, 4).T
    m = np.eye(4)
    s = np.diag(node.get('scale', [1, 1, 1]))
    r = quat_mat(node.get('rotation', [0, 0, 0, 1]))
    m[:3, :3] = r @ s
    m[:3, 3] = node.get('translation', [0, 0, 0])
    return m

def primitives(path):
    """Yields (material_name, positions, normals, uvs, indices) in model space, node transforms baked."""
    j, binc = load(path)
    scene = j['scenes'][j.get('scene', 0)]
    mats = j.get('materials', [])
    def walk(ni, parent):
        node = j['nodes'][ni]
        m = parent @ local(node)
        if 'mesh' in node:
            for p in j['meshes'][node['mesh']]['primitives']:
                at = p['attributes']
                pos = accessor(j, binc, at['POSITION'])
                nrm = accessor(j, binc, at['NORMAL']) if 'NORMAL' in at else None
                uv = accessor(j, binc, at['TEXCOORD_0']) if 'TEXCOORD_0' in at else None
                idx = accessor(j, binc, p['indices']).reshape(-1) if 'indices' in p else np.arange(len(pos))
                pos = (np.c_[pos, np.ones(len(pos))] @ m.T)[:, :3]
                if nrm is not None:
                    nm = np.linalg.inv(m[:3, :3]).T
                    nrm = nrm @ nm.T
                    nrm /= np.linalg.norm(nrm, axis=1, keepdims=True) + 1e-12
                mat = mats[p['material']]['name'] if 'material' in p else 'default'
                yield mat, pos, nrm, uv, idx
        for c in node.get('children', []):
            yield from walk(c, m)
    for r in scene['nodes']:
        yield from walk(r, np.eye(4))
