# Icon Atlas

Builds a single PNG texture atlas and JSON manifest from SVG or PNG icon files.

```powershell
cargo run -p icon_atlas -- `
  --input "path/to/phosphor-icons/regular/*.svg" `
  --atlas resources/icons/phosphor_regular.png `
  --manifest resources/icons/phosphor_regular.json `
  --tile-size 128 `
  --padding 2 `
  --color "#ffffff"
```

`--input` accepts either a glob expression or a folder. Folder input is scanned
recursively. Supported file types are `.svg` and `.png`.

Use `--color` to recolor non-transparent pixels while preserving icon alpha.
Accepted formats are `#RGB`, `#RGBA`, `#RRGGBB`, and `#RRGGBBAA`.

The manifest maps each file stem to a 128x128 atlas rect by default:

```json
{
  "atlas": "phosphor_regular.png",
  "width": 660,
  "height": 528,
  "tile_size": 128,
  "padding": 2,
  "columns": 5,
  "rows": 4,
  "color": "#ffffff",
  "icons": {
    "cube": {
      "source": "path/to/cube.svg",
      "x": 2,
      "y": 2,
      "w": 128,
      "h": 128,
      "u0": 0.003,
      "v0": 0.003,
      "u1": 0.197,
      "v1": 0.246
    }
  }
}
```

Use `--columns` to force a specific atlas width.
