"""Regenerate the original eight-sided shell mesh; run with Python 3."""

import math
from pathlib import Path


def main():
    vertices = []
    for z, radius in [(-0.5, 0.35), (-0.4, 0.5), (0.25, 0.5), (0.5, 0.24)]:
        for i in range(8):
            angle = 2 * math.pi * i / 8
            vertices.append((radius * math.cos(angle), radius * math.sin(angle), z))
    faces = [list(range(8, 0, -1))]
    for ring in range(3):
        for i in range(8):
            a, b = ring * 8 + i + 1, ring * 8 + (i + 1) % 8 + 1
            faces.append([a, b, b + 8, a + 8])
    faces.append(list(range(25, 33)))
    lines = ["# Original shell geometry, +Z forward. Diameter 1, length 1.", "o Shell"]
    lines += ["v %.7f %.7f %.7f" % p for p in vertices]
    for face in faces:
        a, b, c = [vertices[i - 1] for i in face[:3]]
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        length = math.sqrt(sum(x * x for x in n))
        lines.append("vn %.7f %.7f %.7f" % tuple(x / length for x in n))
    for normal, face in enumerate(faces, 1):
        lines.append("f " + " ".join(f"{i}//{normal}" for i in face))
    output = Path(__file__).resolve().parents[2] / "assets/game/meshes/shell.obj"
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
