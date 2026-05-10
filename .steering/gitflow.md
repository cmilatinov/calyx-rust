---
inclusion: auto
---

# Git Workflow - Gitflow

Always use the Gitflow branching model for this project.

## Branches

- `main` - production-ready code. Only receives merges from `staging` (release) or `hotfix/*` branches.
- `staging` - integration branch. All non-hotfix branches merge here. This is the default working branch.
- `feature/*` - new features. Branch from `staging`, merge back into `staging`.
- `fix/*` - bug fixes. Branch from `staging`, merge back into `staging`.
- `chore/*` - maintenance, dependencies, tooling, and cleanup. Branch from `staging`, merge back into `staging`.
- `hotfix/*` - urgent production fixes. Branch from `main`, merge into both `main` and `staging`.
- `release/*` - release prep. Branch from `staging`, merge into `main` and back into `staging`.

## Commit Title Prefixes

All commit titles must start with exactly one of these prefixes:

- `refactor:` - code restructuring with no behavior change
- `feat:` - new feature or capability
- `fix:` - bug fix
- `perf:` - performance improvement
- `chore:` - maintenance, dependencies, tooling, CI
- `docs:` - documentation only changes

Format: `<prefix> <concise description in imperative mood>`

Examples:
- `refactor: decompose Scene into SceneGraph and TransformSystem`
- `feat: add Rapier collision event callbacks`
- `fix: correct ShaderVariable PartialEq comparing binding against group`
- `perf: speed up object ID lookup`
- `chore: update wgpu to 0.20`
- `docs: add Component trait lifecycle documentation`

## Rules

- Never commit directly to `main`.
- Always create or use a Gitflow branch for changes: `feature/*`, `fix/*`, `chore/*`, `hotfix/*`, or `release/*`.
- Always use a dedicated git worktree for repository changes. Create the worktree from the intended base branch and keep each worktree scoped to one branch/PR.
- If there are uncommitted changes in another worktree, leave them untouched and do the new work in a separate worktree instead of stashing or switching branches in place.
- All non-hotfix work targets `staging`.
- Keep commits atomic - one logical change per commit.
- Use feature/fix/chore branches for multi-commit work.
- After a branch has been pushed or a PR has been opened, make review updates as normal follow-up commits on the same branch.
- Do not amend, rebase, or force-push a published branch unless the user explicitly asks for history rewriting.

## Pull Requests

- Every PR must reference at least one Todoist task from the `Calyx` project.
- Every PR should reference at most three Todoist tasks, and the referenced tasks must be related by feature, system boundary, or implementation scope.
- Include a concise "Architectural Decisions" section in the PR body for relevant design choices made in the code. Omit it when the change has no meaningful architecture impact.
- If no existing Todoist task matches the changes, create a new task in the appropriate `Calyx` section before opening the PR, then reference that new task in the PR body.
- For maintenance-only, repo process, or steering changes, use or create a task in `Codebase Improvements` unless another section is clearly more specific.
- When relevant PRs are merged, update or close the corresponding Todoist tasks. Close tasks only when the merged PR satisfies their stated goal or acceptance criteria.
