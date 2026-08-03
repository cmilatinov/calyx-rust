---
inclusion: auto
---

# Autonomous Execution

The user is the product validator and merge authority. The agent owns routine engineering execution from task selection through pull-request maintenance.

## Authority

For the active Calyx product direction, the agent may autonomously:

- Select the highest-priority unblocked Engineering Task from the Calyx Roadmap when the user has not selected one.
- Move the selected issue through Backlog, Ready, In progress, In review, and Done as its state changes.
- Sync `main`, create a scoped branch, implement the task, validate it, self-review it, commit, push, and open a ready-for-review PR.
- Update the issue, project fields, and PR description to reflect completed work and validation.
- Address valid automated or human review comments with follow-up commits, then reply to and resolve the corresponding threads.
- Close a completed issue after its PR is merged and begin the next eligible task when the user signals that merge.

Use `git-pr-workflow`, `github-issues-workflow`, `github-projects-workflow`, `implementation-workflow`, and `verification-workflow` for the applicable mechanics.

## Escalation

Ask the user before proceeding only when:

- Product behavior, visual direction, or acceptance criteria are materially ambiguous.
- The work would expand beyond the selected issue or combine unrelated outcomes.
- User-owned changes conflict with the required work.
- The action is destructive, security-sensitive, hardware-affecting, or would rewrite published history.

When blocked, state the decision required and provide the smallest practical options.

## User Validation

The user validates gameplay feel, visual output, and product acceptance, then merges approved PRs. The agent never merges its own PRs or performs releases.

The agent cannot monitor repository activity between user messages. After a merge or external review event, the user should send a short trigger such as `merged`, `continue`, or `check reviews`; the agent then resumes this workflow end to end.
