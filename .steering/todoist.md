---
inclusion: auto
---

# Todoist Planning

Use the Todoist `Calyx` project as the source of truth for planned work, scope selection, and PR references.

## Board Structure

The board is organized by product area:

- `Engine Foundations` - engine systems needed by gameplay and editor work.
- `Core Gameplay` - tank movement, aiming, shooting, health, spawn, camera, arena, and scoring work.
- `Destruction System` - destructible objects, explosions, debris, and destruction feedback.
- `Powerups` - gameplay modifiers after the core loop is playable.
- `Multiplayer & Steam` - Steamworks, lobby, transport, server authority, prediction, and reconciliation.
- `Polish & Juice` - audio, VFX, camera shake, and game feel after the loop works.
- `Codebase Improvements` - maintenance and refactors that reduce risk or unblock future work.
- `Editor` - editor workflow, inspector, asset UI, hot reload, terminal, and authoring tools.
- `MVP (First Playable)`, `V1 Playable Game`, and `First Playable Plan` - milestone planning for playable slices.

## Priority Interpretation

Todoist priority maps to implementation priority as follows:

- `P1` - highest priority; select first unless the user directs otherwise or a dependency is missing.
- `P2` - important near-term work.
- `P3` - useful but normally below active milestone work.
- `P4` - backlog, polish, or later-stage work unless directly connected to the current task.

When choosing work autonomously, inspect the relevant section and prefer the highest Todoist priority item. If multiple items share the same priority, choose the one that best advances the current milestone and can be completed cleanly in one PR.

## Current Product Direction

The board currently emphasizes a first-playable single-player tank loop. Prefer work that advances this sequence:

1. Tank entity and scene wiring.
2. Tank control: forward/reverse movement, turning, and predictable hull behavior.
3. Top-down camera and aiming, including independent turret rotation.
4. Shooting: projectile prefab/component, barrel spawn, hit detection, lifetime, and fire-rate gating.
5. Health, damage, death, and respawn.
6. Minimal HUD for health and ammo/reload.
7. Validation of the first playable slice.

Treat multiplayer, Steam integration, powerups, destruction, and polish as later work unless the user explicitly selects those areas or they are needed to support the playable loop.

## Implementation Selection Rules

- For user-selected work, use the named Todoist task even if it is not the highest priority.
- For general "next task" requests, start with `Codebase Improvements` only when the user is asking for maintenance/refactor work; otherwise prefer the first-playable gameplay path.
- Group tasks only when they share the same files or system boundary and can be reviewed as one coherent change.
- A PR should cover at most three Todoist items. If more than three tasks are involved, split the work into separate PRs unless the user explicitly approves a larger scope.
- Do not mix unrelated gameplay, editor, engine, and cleanup tasks in the same PR.
- If a task is too broad, implement a clearly reviewable slice and leave the Todoist item open unless the completed slice satisfies its acceptance criteria.
- Use task descriptions and acceptance notes as requirements; do not silently narrow scope below the task's stated goal.

## PR Reference Rules

- Every PR must reference at least one Todoist task from the `Calyx` project.
- Every PR should reference at most three Todoist tasks, and those tasks must be related by feature, system boundary, or implementation scope.
- Reference tasks by Todoist task title and, when available, task id in the PR body.
- If no existing Todoist item matches the PR changes, create a new task in the most appropriate `Calyx` section before opening the PR.
- For maintenance-only changes, use `Codebase Improvements` unless another section is clearly more specific.
- For repo/process/steering changes, create or use a `Codebase Improvements` task.
- Mention created tasks explicitly in the PR body so the planning history remains connected to the code change.
