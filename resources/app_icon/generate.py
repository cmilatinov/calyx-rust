"""Renders calyx.svg into the app icon files beside it.

Writes calyx.png (256 px, the window icon the editor and player load) and
calyx.ico (16, 24, 32, 48, 64 and 256 px, the Windows executable resource).
Each size is rendered from the SVG at that size rather than scaled down from
the 256 px image, so the small sizes stay sharp.

    pip install resvg_py==0.5.0 pillow==13.0.0
    python resources/app_icon/generate.py
"""

import io
import os

import resvg_py
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
SVG = os.path.join(HERE, "calyx.svg")
ICO_SIZES = (16, 24, 32, 48, 64, 256)


def render(size):
    png = resvg_py.svg_to_bytes(svg_path=SVG, width=size, height=size)
    return Image.open(io.BytesIO(bytes(png))).convert("RGBA")


def main():
    images = {size: render(size) for size in ICO_SIZES}
    images[256].save(os.path.join(HERE, "calyx.png"), optimize=True)
    largest, *rest = (images[size] for size in reversed(ICO_SIZES))
    largest.save(
        os.path.join(HERE, "calyx.ico"),
        sizes=[(size, size) for size in ICO_SIZES],
        append_images=rest,
    )


if __name__ == "__main__":
    main()
