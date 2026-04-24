---
inclusion: auto
---

# Git Workflow — Gitflow

This project follows the Gitflow branching model.

## Branches

- `main` — production-ready code. Only receives merges from `staging` (release) or `hotfix/*` branches.
- `staging` — integration branch. All feature and fix branches merge here. This is the default working branch.
- `feature/*` — new features. Branch from `staging`, merge back into `staging`.
- `fix/*` — bug fixes. Branch from `staging`, merge back into `staging`.
- `hotfix/*` — urgent production fixes. Branch from `main`, merge into both `main` and `staging`.
- `release/*` — release prep. Branch from `staging`, merge into `main` and back into `staging`.

## Commit Message Prefixes

All commits must use one of these conventional prefixes:

- `feat:` — new feature or capability
- `fix:` — bug fix
- `refactor:` — code restructuring with no behavior change
- `chore:` — maintenance, dependencies, tooling, CI
- `docs:` — documentation only changes

Format: `<prefix> <concise description in imperative mood>`

Examples:
- `feat: add Rapier collision event callbacks`
- `fix: correct ShaderVariable PartialEq comparing binding against group`
- `refactor: decompose Scene into SceneGraph and TransformSystem`
- `chore: update wgpu to 0.20`
- `docs: add Component trait lifecycle documentation`

## Rules

- Never commit directly to `main`.
- All work targets `staging` unless it's a hotfix.
- Keep commits atomic — one logical change per commit.
- Use feature/fix branches for multi-commit work.
