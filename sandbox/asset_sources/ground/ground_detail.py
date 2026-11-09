"""Detail textures of the arena's blended ground: the toon set (ET #576).

Writes two tileable 512 x 512 RGBA PNGs of greyscale layer detail, which the
ground shader (`assets/shaders/mesh/ground_surface.wgsl`) tints with each
palette's ground colour roles, so one set serves every map:

- detail_a.png: R grass, G dirt, B sand, A gravel
- detail_b.png: R field soil, G mud, B dark grass, A light grass

Each layer is cel-shaded clay: two tones a step apart, in patches of mixed
sizes with soft-cornered but crisp edges, and a few crisp accents in a third
tone (leaf clumps on grass, flat stones on gravel, clods in the field soil,
dark pebbles in the dirt, puddles in the mud). The patches mix two noise
scales and stay low in contrast, so wide open ground reads as ground and not
as camouflage. Each channel averages 0.5, which leaves the layer colour as
it is.

Our own generator: no third-party source. Deterministic (fixed seeds).

Usage, from the repository root:
    python sandbox/asset_sources/ground/ground_detail.py sandbox/assets/generated/ground [strip.png]
"""

import sys
from pathlib import Path

import numpy as np
from PIL import Image

N = 512
# Must match `DETAIL_TILE` in `sandbox/examples/remake_arena/ground.rs`.
TILE_METRES = 6.0

YY, XX = np.mgrid[0:N, 0:N].astype(np.float32)


def rng(seed):
    return np.random.default_rng(seed)


def periodic_noise(seed, sigma):
    """Tileable smooth noise in 0..1: white noise low-passed in frequency space."""
    white = rng(seed).standard_normal((N, N))
    f = np.fft.fftfreq(N)
    fx, fy = np.meshgrid(f, f)
    kernel = np.exp(-2 * (np.pi * sigma) ** 2 * (fx**2 + fy**2))
    out = np.real(np.fft.ifft2(np.fft.fft2(white) * kernel))
    lo, hi = np.percentile(out, [1, 99])
    return np.clip((out - lo) / (hi - lo), 0, 1)


def smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


def wrapped_delta(cx, cy):
    dx = (XX - cx + N / 2) % N - N / 2
    dy = (YY - cy + N / 2) % N - N / 2
    return dx, dy


def two_tone(seed, sigmas=(45, 16), cover=0.5, step=0.16, edge=0.025):
    """Two tones `step` apart, in patches mixing a broad and a smaller noise
    scale, so the patches vary in size."""
    n = 0.65 * periodic_noise(seed, sigmas[0]) + 0.35 * periodic_noise(seed + 100, sigmas[1])
    t = np.quantile(n, 1 - cover)
    return 0.5 - step / 2 + step * smoothstep(t - edge, t + edge, n)


def shapes(seed, count, radius, elong=1.0):
    """A 0/1 mask of `count` crisp ellipses, wrapped."""
    r = rng(seed)
    out = np.zeros((N, N), bool)
    for _ in range(count):
        cx, cy = r.uniform(0, N, 2)
        rad = r.uniform(*radius)
        dx, dy = wrapped_delta(cx, cy)
        box = (np.abs(dx) < rad * 2.2) & (np.abs(dy) < rad * 2.2)
        a = r.uniform(0, np.pi)
        u = dx[box] * np.cos(a) + dy[box] * np.sin(a)
        v = -dx[box] * np.sin(a) + dy[box] * np.cos(a)
        e = elong * r.uniform(0.8, 1.25)
        out[box] |= (u / e) ** 2 + v**2 < rad**2
    return out


def leafy(seed, count):
    return np.where(shapes(seed + 7, count, (4, 7), elong=1.8), 0.72, two_tone(seed))


def layers():
    grass = leafy(10, 40)
    dark = leafy(20, 60)
    light = leafy(30, 28)
    dirt = np.where(shapes(41, 45, (3, 6)), 0.3, two_tone(40, sigmas=(35, 12)))
    sand = two_tone(50, sigmas=(70, 25), step=0.12, edge=0.04)
    stones = shapes(60, 150, (8, 14), elong=1.3)
    gravel = np.where(stones, 0.7, 0.36 + 0.08 * periodic_noise(61, 18))
    field = np.where(shapes(71, 140, (3, 6)), 0.68, two_tone(70, sigmas=(30, 10)))
    mud = np.where(shapes(81, 14, (12, 24), elong=1.5), 0.32, two_tone(80, sigmas=(50, 18)))
    return [grass, dirt, sand, gravel], [field, mud, dark, light]


def recentre(a):
    return np.clip(a - a.mean() + 0.5, 0, 1)


def write(path, channels):
    data = np.stack([np.round(recentre(c) * 255).astype(np.uint8) for c in channels], -1)
    Image.fromarray(data, "RGBA").save(path)


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    a, b = layers()
    write(out / "detail_a.png", a)
    write(out / "detail_b.png", b)
    if len(sys.argv) > 2:
        # A greyscale strip of the eight layers, for review.
        strip = np.concatenate([recentre(c) for c in a + b], 1)
        Image.fromarray(np.round(strip * 255).astype(np.uint8), "L").save(sys.argv[2])


if __name__ == "__main__":
    main()
