# Codex Guidance

Use the steering files in this repository as standing project instructions:

- [.steering/gitflow.md](.steering/gitflow.md) - required Gitflow branching model and commit title prefixes.
- [.steering/project.md](.steering/project.md) - Calyx workspace architecture, crate map, and project conventions.
- [.steering/todoist.md](.steering/todoist.md) - Todoist planning, task selection, and PR reference rules.
- [.steering/code-review.md](.steering/code-review.md) - senior-developer review checklist and keep/skip workflow.

When making repository changes, follow the Gitflow rules in `.steering/gitflow.md`. Commit titles must start with exactly one of `refactor:`, `feat:`, `fix:`, `chore:`, or `docs:`.

After a branch has been pushed or a PR has been opened, make review updates as normal follow-up commits on the same branch. Do not amend, rebase, or force-push a published branch unless the user explicitly asks for history rewriting.

When asked what is or is not implemented, or asked to fetch/pick the next highest-priority task, consult the Todoist `Calyx` project first. For implementation work, use the `Codebase Improvements` section unless the user names another section.

When the user says a PR was merged and asks to continue or move on, sync `staging`, create a fresh Gitflow branch, pick the next highest-priority Todoist task plus tightly connected tasks, implement, validate, push, and open a PR.
