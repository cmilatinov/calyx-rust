# Final arena kit refinement evidence

Source revision `b53426e` (geometry/material changes in `a56a140`), PR #520.

75 fresh Calyx game-view captures at 2062 x 932, controlled hands-free through the `calyx_ctl` protocol. Every model is recaptured on all three palettes with the requested 2 px inner ink (outer ink stays 2 px, opacity and thresholds unchanged).

Small and medium tree source/mesh files are restored exactly to approved revision `5707b68`. The large tree has a broad unequal overlapping crown over a fused organic trunk with visible tapered branch forks. Sandbags stand 1.27 m high. Hazard paint is confined to both sloped side panels and the roof is plain concrete. Pickup signals are green and blue with stronger colored bloom halos; the green signal retains the legacy `pickup_yellow` name/ID.

Far views use the saved cameras and retained map lighting/bloom with no inspection fill. Moonlit retains its authored moon, sky, grading and bloom; the requested 2 px inner ink is applied to all three map styles.

Close-ups isolate each actual generated prefab and add temporary ambient inspection fill 0.65 to reveal assembly details. This fill and inspection camera/visibility changes were never saved. The cached compatible debug editor in ct266 retained the binary-only 16 MiB stack reserve workaround for #514. The editor reported assemblies loaded and was shut down after capture.

These screenshots supersede v2/v3 model captures. The agreed concept sheet remains at ../v2/concept-sheet.png.

| View | Sunny Battlefield | Sunbaked | Moonlit |
| --- | --- | --- | --- |
| Complete kit at Far | ![](kit-v4-sunny-far.png) | ![](kit-v4-sunbaked-far.png) | ![](kit-v4-moonlit-far.png) |
| Repeated-join pen at Far | ![](kit-v4-sunny-pen-far.png) | ![](kit-v4-sunbaked-pen-far.png) | ![](kit-v4-moonlit-pen-far.png) |

| Model | Sunny Battlefield | Sunbaked | Moonlit |
| --- | --- | --- | --- |
| wall_short | ![](closeups/kit-v4-wall_short-sunny-close.png) | ![](closeups/kit-v4-wall_short-sunbaked-close.png) | ![](closeups/kit-v4-wall_short-moonlit-close.png) |
| wall_long | ![](closeups/kit-v4-wall_long-sunny-close.png) | ![](closeups/kit-v4-wall_long-sunbaked-close.png) | ![](closeups/kit-v4-wall_long-moonlit-close.png) |
| wall_corner | ![](closeups/kit-v4-wall_corner-sunny-close.png) | ![](closeups/kit-v4-wall_corner-sunbaked-close.png) | ![](closeups/kit-v4-wall_corner-moonlit-close.png) |
| wall_t | ![](closeups/kit-v4-wall_t-sunny-close.png) | ![](closeups/kit-v4-wall_t-sunbaked-close.png) | ![](closeups/kit-v4-wall_t-moonlit-close.png) |
| wall_cross | ![](closeups/kit-v4-wall_cross-sunny-close.png) | ![](closeups/kit-v4-wall_cross-sunbaked-close.png) | ![](closeups/kit-v4-wall_cross-moonlit-close.png) |
| wall_end | ![](closeups/kit-v4-wall_end-sunny-close.png) | ![](closeups/kit-v4-wall_end-sunbaked-close.png) | ![](closeups/kit-v4-wall_end-moonlit-close.png) |
| barrier_hazard | ![](closeups/kit-v4-barrier_hazard-sunny-close.png) | ![](closeups/kit-v4-barrier_hazard-sunbaked-close.png) | ![](closeups/kit-v4-barrier_hazard-moonlit-close.png) |
| barrier_plain | ![](closeups/kit-v4-barrier_plain-sunny-close.png) | ![](closeups/kit-v4-barrier_plain-sunbaked-close.png) | ![](closeups/kit-v4-barrier_plain-moonlit-close.png) |
| crate_framed | ![](closeups/kit-v4-crate_framed-sunny-close.png) | ![](closeups/kit-v4-crate_framed-sunbaked-close.png) | ![](closeups/kit-v4-crate_framed-moonlit-close.png) |
| crate_large | ![](closeups/kit-v4-crate_large-sunny-close.png) | ![](closeups/kit-v4-crate_large-sunbaked-close.png) | ![](closeups/kit-v4-crate_large-moonlit-close.png) |
| rock_small | ![](closeups/kit-v4-rock_small-sunny-close.png) | ![](closeups/kit-v4-rock_small-sunbaked-close.png) | ![](closeups/kit-v4-rock_small-moonlit-close.png) |
| rock_medium | ![](closeups/kit-v4-rock_medium-sunny-close.png) | ![](closeups/kit-v4-rock_medium-sunbaked-close.png) | ![](closeups/kit-v4-rock_medium-moonlit-close.png) |
| rock_large | ![](closeups/kit-v4-rock_large-sunny-close.png) | ![](closeups/kit-v4-rock_large-sunbaked-close.png) | ![](closeups/kit-v4-rock_large-moonlit-close.png) |
| tree_small | ![](closeups/kit-v4-tree_small-sunny-close.png) | ![](closeups/kit-v4-tree_small-sunbaked-close.png) | ![](closeups/kit-v4-tree_small-moonlit-close.png) |
| tree | ![](closeups/kit-v4-tree-sunny-close.png) | ![](closeups/kit-v4-tree-sunbaked-close.png) | ![](closeups/kit-v4-tree-moonlit-close.png) |
| tree_large | ![](closeups/kit-v4-tree_large-sunny-close.png) | ![](closeups/kit-v4-tree_large-sunbaked-close.png) | ![](closeups/kit-v4-tree_large-moonlit-close.png) |
| flag | ![](closeups/kit-v4-flag-sunny-close.png) | ![](closeups/kit-v4-flag-sunbaked-close.png) | ![](closeups/kit-v4-flag-moonlit-close.png) |
| lamp | ![](closeups/kit-v4-lamp-sunny-close.png) | ![](closeups/kit-v4-lamp-sunbaked-close.png) | ![](closeups/kit-v4-lamp-moonlit-close.png) |
| berry_bush | ![](closeups/kit-v4-berry_bush-sunny-close.png) | ![](closeups/kit-v4-berry_bush-sunbaked-close.png) | ![](closeups/kit-v4-berry_bush-moonlit-close.png) |
| grass_tuft | ![](closeups/kit-v4-grass_tuft-sunny-close.png) | ![](closeups/kit-v4-grass_tuft-sunbaked-close.png) | ![](closeups/kit-v4-grass_tuft-moonlit-close.png) |
| sandbag_stack | ![](closeups/kit-v4-sandbag_stack-sunny-close.png) | ![](closeups/kit-v4-sandbag_stack-sunbaked-close.png) | ![](closeups/kit-v4-sandbag_stack-moonlit-close.png) |
| pickup_yellow | ![](closeups/kit-v4-pickup_yellow-sunny-close.png) | ![](closeups/kit-v4-pickup_yellow-sunbaked-close.png) | ![](closeups/kit-v4-pickup_yellow-moonlit-close.png) |
| pickup_blue | ![](closeups/kit-v4-pickup_blue-sunny-close.png) | ![](closeups/kit-v4-pickup_blue-sunbaked-close.png) | ![](closeups/kit-v4-pickup_blue-moonlit-close.png) |
