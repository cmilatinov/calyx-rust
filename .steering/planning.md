---
inclusion: auto
---

# Calyx Planning

## Source Of Truth

- Repository Engineering Tasks are GitHub Issues in `cmilatinov/calyx-rust`.
- Roadmap priority and status live in the [Calyx Roadmap](https://github.com/users/cmilatinov/projects/1).
- Project priority runs from `P1` (highest) through `P4` (lowest). Use the live project field options if they change.
- Imported Todoist links and IDs are historical context only. Do not use Todoist as the execution source.

Use `github-issues-workflow` to select, refine, create, or close Engineering Tasks. Use `github-projects-workflow` for roadmap Epic and project-field changes.

## Current Product Direction

Prioritize the first-playable single-player tank loop:

1. Tank entity and scene wiring.
2. Forward/reverse movement, turning, and predictable hull behavior.
3. Top-down camera, aiming, and independent turret rotation.
4. Projectile firing, hit detection, lifetime, ammo, and reload gating.
5. Health, damage, death, and respawn.
6. Minimal health and ammo/reload HUD.
7. End-to-end first-playable validation.

Treat multiplayer, Steam integration, powerups, destruction, and polish as later work unless the user selects them or they block the playable loop.

## Task Selection

- Follow a user-selected issue even when another issue has higher roadmap priority.
- For a general next-task request, choose the highest-priority unblocked issue that advances the current product direction.
- For maintenance or refactor requests, choose the highest-priority unblocked maintenance issue instead.
- Break priority ties by milestone impact, dependency order, and whether the outcome fits one coherent PR.
- Do not combine unrelated gameplay, editor, engine, and maintenance outcomes.
