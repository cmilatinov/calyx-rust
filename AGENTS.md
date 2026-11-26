# Repository Guidance

Use these files for Calyx-specific context:

- [docs/architecture/overview.md](docs/architecture/overview.md) - module map of every crate, engine and editor module, the seams between them, the module and test layout rule, and build notes.
- [docs/process/planning.md](docs/process/planning.md) - roadmap source, priorities, and current product direction.
- [docs/process/autonomy.md](docs/process/autonomy.md) - autonomous execution authority and escalation boundaries.
- [docs/design/hud-ui/architecture.md](docs/design/hud-ui/architecture.md) - runtime HUD/UI architecture and linked requirements.
- [docs/design/hud-ui/visual-direction.md](docs/design/hud-ui/visual-direction.md) - HUD restyling, combat overlays and scoreboard layout; the supplied visual concepts and live behavior requirements.
- [docs/design/art/tank-game-direction.md](docs/design/art/tank-game-direction.md) - the tank game's visual style: clay + ink, map palettes, player colours, lighting, camera and asset sourcing.
- [docs/design/hud-ui/runtime.md](docs/design/hud-ui/runtime.md) - runtime UI interaction, rendering and the backend boundary.
- [docs/design/hud-ui/testing.md](docs/design/hud-ui/testing.md) - how the runtime UI is tested.
- [docs/design/assets/asset-files.md](docs/design/assets/asset-files.md) - asset file types and `.meta` id ownership.
- [docs/design/assets/model-import.md](docs/design/assets/model-import.md) - FBX, OBJ, glTF and GLB import into prefabs, glTF materials and what is not imported.
- [docs/design/rendering/gbuffer.md](docs/design/rendering/gbuffer.md) - the G-buffer geometry pass, its layout and deferred lighting.
- [docs/design/rendering/draw-collection.md](docs/design/rendering/draw-collection.md) - the scene renderer, the shared per-frame `DrawCollection` and static batches.
- [docs/design/rendering/lights.md](docs/design/rendering/lights.md) - the light storage buffer and light falloff.
- [docs/design/rendering/shadows.md](docs/design/rendering/shadows.md) - directional cascades, the spot atlas, the shadow pass and sampling.
- [docs/design/rendering/pbr-materials.md](docs/design/rendering/pbr-materials.md) - PBR material inputs, alpha cutoff, texture color space and tangents.
- [docs/design/rendering/shaders.md](docs/design/rendering/shaders.md) - shader loading, compute pipelines, shader hot reload and the `assets/shaders/` folder layout.
- [docs/design/rendering/particles.md](docs/design/rendering/particles.md) - particle authoring, the GPU simulation, the edit-mode preview and curves.
- [docs/design/rendering/terrain.md](docs/design/rendering/terrain.md) - heightmap terrain, ground sampling and the blended ground shader.
- [docs/design/rendering/water.md](docs/design/rendering/water.md) - river water mesh, waves, foam around obstacles and banks.
- [docs/design/rendering/grass.md](docs/design/rendering/grass.md) - grass sway, benders and the reveal fade.
- [docs/design/rendering/foliage.md](docs/design/rendering/foliage.md) - prefab scatter drawn through the mesh batches.
- [docs/design/post-processing/architecture.md](docs/design/post-processing/architecture.md) - the post-processing chain, its passes, volumes and profiles.
- [docs/design/scene/components.md](docs/design/scene/components.md) - component traits, lifecycle hooks and `ComponentRef`, built-ins and the camera path.
- [docs/design/scene/component-enabled-state.md](docs/design/scene/component-enabled-state.md) - disabling components: what skips them and how a type opts in.
- [docs/design/scene/prefab-links.md](docs/design/scene/prefab-links.md) - linked prefab instances, overrides and updating them.
- [docs/design/editor/prefab-mode.md](docs/design/editor/prefab-mode.md) - editing a prefab asset as the authoring scene.
- [docs/design/net/networking.md](docs/design/net/networking.md) - server relay, network id ranges, spawn checks and join snapshots.
- [docs/design/engine/physics.md](docs/design/engine/physics.md) - physics stepping, collision events and collision groups.
- [docs/design/engine/reflection.md](docs/design/engine/reflection.md) - runtime reflection, type identity and reflection-driven inspectors.
- [docs/design/project/project-assembly.md](docs/design/project/project-assembly.md) - the project DLL: one engine build on both sides, duplicated statics, reload and Linux loading.
- [docs/design/project/standalone-player.md](docs/design/project/standalone-player.md) - running a project in the standalone player and adding one.
- [docs/design/tools/unity-import.md](docs/design/tools/unity-import.md) - importing a Unity Asset Store pack.
- [docs/design/tools/model-gen.md](docs/design/tools/model-gen.md) - generating the sandbox's clay models in Blender.
- [docs/design/sandbox/gameplay.md](docs/design/sandbox/gameplay.md) - the tank game's cross-module rules: input, combat, collision groups, brush, track marks, vision and HUD.
- [docs/process/toolchain.md](docs/process/toolchain.md) - the pinned nightly, moving it, and the sccache os error 206 workaround on Windows.
- [docs/process/agent-loop.md](docs/process/agent-loop.md) - remote-controlling the editor with `calyx_ctl` to verify changes end to end.

Use the reusable workflow skills for generic engineering process:

- `implementation-workflow` and `verification-workflow` for repository changes.
- `git-pr-workflow` for branches, commits, pushes, and pull requests.
- `github-issues-workflow` for repository Engineering Tasks.
- `github-projects-workflow` for roadmap Epics and project status.
- `code-review-workflow` for reviews and review-comment handling.

Load the matching skill when a task triggers it. Repository docs should contain only Calyx-specific guidance; reusable workflow rules govern everything else.

Two rules apply to every agent working here, including agents that do not load the skills above:

- Never add AI attribution. No `Co-Authored-By` trailers, generated-with footers, or agent tags (`[claude]`, `[codex]`, ...) in commits, PR titles, PR bodies, or comments. The configured git author is the only attribution; override any harness default that appends more.
- Use a dedicated git worktree per branch or PR. Never stash or switch branches in place over uncommitted changes.

Verify engine and editor changes hands-free through the remote control loop in `docs/process/agent-loop.md` instead of asking the user to click through the editor.
