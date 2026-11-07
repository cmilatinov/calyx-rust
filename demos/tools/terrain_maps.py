"""Generates the terrain demo's heightmap and color map.

Run from the repository root: python demos/tools/terrain_maps.py
Writes demos/assets/terrain/heightmap.png (512 x 512, 16-bit grayscale) and
demos/assets/terrain/color.png (512 x 512 sRGB), both deterministic.
"""

from pathlib import Path

import numpy as np
from PIL import Image

SIZE = 512
OUT = Path(__file__).resolve().parent.parent / "assets" / "terrain"


def value_noise(rng: np.random.Generator, cells: int) -> np.ndarray:
    """Smoothly interpolated random values on a cells x cells lattice."""
    lattice = rng.random((cells + 1, cells + 1))
    t = np.linspace(0.0, cells, SIZE, endpoint=False)
    i = t.astype(int)
    f = t - i
    f = f * f * (3.0 - 2.0 * f)
    rows = lattice[i][:, i] * (1 - f)[None, :] + lattice[i][:, i + 1] * f[None, :]
    rows_next = lattice[i + 1][:, i] * (1 - f)[None, :] + lattice[i + 1][:, i + 1] * f[None, :]
    return rows * (1 - f)[:, None] + rows_next * f[:, None]


def heights() -> np.ndarray:
    rng = np.random.default_rng(472)
    field = sum(value_noise(rng, 4 * 2**octave) * 0.5**octave for octave in range(6))
    field = (field - field.min()) / (field.max() - field.min())
    # A valley: low in the middle, rising to ridges toward the edges.
    y, x = np.mgrid[-1:1:SIZE * 1j, -1:1:SIZE * 1j]
    radius = np.clip(np.sqrt(x * x + y * y) / 1.2, 0.0, 1.0)
    valley = radius**2 * (3.0 - 2.0 * radius)
    h = 0.15 * field + 0.85 * valley * (0.4 + 0.6 * field)
    return (h - h.min()) / (h.max() - h.min())


def colors(h: np.ndarray) -> np.ndarray:
    gy, gx = np.gradient(h * 60.0, 200.0 / SIZE)
    slope = np.clip(np.sqrt(gx * gx + gy * gy), 0.0, 2.0) / 2.0
    grass = np.array([0.33, 0.52, 0.22])
    meadow = np.array([0.55, 0.62, 0.30])
    rock = np.array([0.45, 0.42, 0.38])
    snow = np.array([0.92, 0.93, 0.95])
    c = grass + (meadow - grass) * np.clip(h * 2.5, 0, 1)[..., None]
    c = c + (rock - c) * np.clip((slope - 0.25) * 4.0, 0, 1)[..., None]
    c = c + (snow - c) * np.clip((h - 0.8) * 8.0, 0, 1)[..., None]
    return (np.clip(c, 0, 1) * 255).astype(np.uint8)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    h = heights()
    Image.fromarray((h * 65535).round().astype(np.uint16)).save(OUT / "heightmap.png")
    Image.fromarray(colors(h), "RGB").save(OUT / "color.png")


if __name__ == "__main__":
    main()
