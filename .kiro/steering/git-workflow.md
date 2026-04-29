# Git Workflow

## Commit Messages

Write clear, descriptive commit messages:
- Use imperative mood: "Add feature" not "Added feature"
- First line: concise summary (50 chars or less preferred)
- Body (optional): explain why, not what — the diff shows what changed
- Reference issue/ticket IDs when applicable

```
feat: add user email validation

Validates email format on registration to prevent invalid accounts.
Closes #142
```

## Conventional Commits

Use conventional commit prefixes when the project adopts them:

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`

## Branch Naming

Use a consistent pattern: `<type>/<short-description>`

```
feat/user-auth
fix/email-validation
refactor/order-service
```

Keep branch names lowercase with hyphens. Include ticket IDs if the team convention requires it: `feat/PROJ-123-user-auth`.

## Merge Strategy

Choose one strategy per project and stick with it:

- **Squash merge** (recommended for feature branches): Collapses all commits into one clean commit on main. Keeps history linear and readable.
- **Rebase and merge**: Replays commits on top of main. Preserves individual commits but requires clean history.
- **Merge commit**: Creates a merge commit. Preserves full branch history. Best for long-lived branches.

Before merging:
1. Rebase on latest main to resolve conflicts locally
2. Ensure CI passes
3. Get required reviews

## Conflict Resolution

1. Pull latest main: `git fetch origin main`
2. Rebase your branch: `git rebase origin/main`
3. Resolve conflicts file by file — understand both sides before choosing
4. Run tests after resolving to verify nothing broke
5. Never resolve conflicts by blindly accepting one side

## What Not to Commit

- Generated files (build output, compiled assets) — use `.gitignore`
- Secrets, API keys, credentials — use environment variables
- Large binary files — use Git LFS if necessary
- Editor/IDE config — add to global gitignore unless team-shared

## Pull Requests

- Keep PRs focused: one logical change per PR
- Write a clear description of what and why
- Link related issues
- Self-review the diff before requesting review

> For the full development process (planning, TDD, code review) before git operations,
> see [development-workflow.md](./development-workflow.md).

