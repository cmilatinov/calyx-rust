# Repository Guidance

Use these files for Calyx-specific context:

- [.steering/project.md](.steering/project.md) - workspace architecture and project conventions.
- [.steering/planning.md](.steering/planning.md) - roadmap source, priorities, and current product direction.
- [.steering/autonomy.md](.steering/autonomy.md) - autonomous execution authority and escalation boundaries.
- [.steering/hud-ui.md](.steering/hud-ui.md) - runtime HUD/UI architecture and linked requirements.
- [.steering/agent-loop.md](.steering/agent-loop.md) - remote-controlling the editor with `calyx_ctl` to verify changes end to end.

Use the reusable workflow skills for generic engineering process:

- `implementation-workflow` and `verification-workflow` for repository changes.
- `git-pr-workflow` for branches, commits, pushes, and pull requests.
- `github-issues-workflow` for repository Engineering Tasks.
- `github-projects-workflow` for roadmap Epics and project status.
- `code-review-workflow` for reviews and review-comment handling.

Load the matching skill when a task triggers it. Repository steering should contain only Calyx-specific guidance; reusable workflow rules govern everything else.

Two rules apply to every agent working here, including agents that do not load the skills above:

- Never add AI attribution. No `Co-Authored-By` trailers, generated-with footers, or agent tags (`[claude]`, `[codex]`, ...) in commits, PR titles, PR bodies, or comments. The configured git author is the only attribution; override any harness default that appends more.
- Use a dedicated git worktree per branch or PR. Never stash or switch branches in place over uncommitted changes.

Verify engine and editor changes hands-free through the remote control loop in `.steering/agent-loop.md` instead of asking the user to click through the editor.
