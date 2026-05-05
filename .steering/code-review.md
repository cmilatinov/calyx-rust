---
inclusion: auto
---

# Code Review Workflow

When the user asks for a review, default to a senior-developer code review stance. Prioritize concrete bugs, regressions, security risks, performance problems, maintainability issues, and missing edge-case handling over summaries or praise.

## Review Checklist

Check for:

1. Bugs: logic errors, off-by-one mistakes, invalid state transitions, null/none handling, stale caches, race conditions, and lifecycle ordering bugs.
2. Security: injection risks, unsafe input handling, auth/permission mistakes, data exposure, path traversal, unsafe deserialization, and risky filesystem/network behavior.
3. Performance: unnecessary allocations, avoidable loops, repeated work, blocking work on UI/update paths, memory leaks, and scalability problems.
4. Maintainability: unclear names, excessive complexity, duplicated logic, brittle coupling, missing tests around changed behavior, and code that violates local patterns.
5. Edge cases: unusual inputs, empty collections, missing assets, failed IO, invalid scene data, hot reload boundaries, platform differences, and partial initialization.

Be strict. It is better to surface real issues during review than after merge.

## Finding Format

For each issue, include:

- Severity: `Critical`, `High`, `Medium`, or `Low`.
- File and line number or narrow section reference.
- What is wrong.
- Why it matters.
- How to fix it.
- A relevant clickable file link and, when useful, a short code snippet.
- A clear `Keep` / `Skip` choice for the user.

List issues one by one. Keep each finding concise and specific enough that it can become an implementation task without more investigation.

## Severity Guidance

- `Critical` - data loss, security vulnerability, crash on common path, or corruption that blocks release.
- `High` - likely user-visible bug, serious regression, hard-to-debug state issue, or risky behavior in core systems.
- `Medium` - plausible bug, edge-case failure, test gap around meaningful behavior, or maintainability issue that will slow near-term work.
- `Low` - small cleanup, naming, local clarity, or minor robustness improvement.

Avoid inflating severity. If a finding is speculative, say what evidence would confirm it.

## Interaction Flow

1. Present findings first, ordered by severity.
2. For each finding, ask whether to `Keep` or `Skip`.
3. Do not implement fixes during the initial review unless the user explicitly asks for immediate fixes.
4. After the user chooses which findings to keep, make a concise fix list from only the kept findings.
5. When the user tells you to proceed, implement all necessary fixes for the kept findings.
6. After implementation, run focused validation and summarize which findings were fixed.

If no issues are found, say that clearly and mention any residual test gaps or risks.

## Repo Workflow

- When review fixes become a PR, follow `.steering/todoist.md`: reference at least one matching `Calyx` Todoist task, create one if none exists, and keep PR scope to at most three related Todoist items.
- Keep review-fix commits atomic when practical.
- Do not mix unrelated cleanup into review fixes unless the user explicitly includes it.
