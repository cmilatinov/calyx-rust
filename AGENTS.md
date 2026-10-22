# Repository Guidance

Use these files for Calyx-specific context:

- [docs/architecture/overview.md](docs/architecture/overview.md) - workspace architecture and project conventions.
- [docs/process/planning.md](docs/process/planning.md) - roadmap source, priorities, and current product direction.
- [docs/process/autonomy.md](docs/process/autonomy.md) - autonomous execution authority and escalation boundaries.
- [docs/design/hud-ui/architecture.md](docs/design/hud-ui/architecture.md) - runtime HUD/UI architecture and linked requirements.
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
