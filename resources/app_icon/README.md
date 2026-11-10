# Calyx app icon

A flame-shaped C with an inner flame tongue, in a yellow-to-orange-to-red
gradient on a dark rounded-square tile.

The mark is original, drawn for Calyx. It uses no third-party logo, artwork
or font. `calyx.svg` is the master; the other files are rendered from it:

- `calyx.png`: 256 px, the window icon the editor and the player load
  (`calyx_engine::runtime::calyx_icon`).
- `calyx.ico`: 16, 24, 32, 48, 64 and 256 px, the icon resource embedded in
  the Windows `editor.exe` and the sandbox `game.exe` by their `build.rs`.

After editing the SVG, regenerate both files:

```
pip install resvg_py==0.5.0 pillow==13.0.0
python resources/app_icon/generate.py
```
