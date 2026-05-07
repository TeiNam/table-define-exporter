# Git Workflow Rules

## Commit Message Format

```
<type>: <description>

<optional body>
```

Types: feat, fix, refactor, docs, test, chore, perf, ci

## PR Workflow

When creating a PR:
1. Analyze the full commit history (not just the latest commit)
2. Review all changes with `git diff [base-branch]...HEAD`
3. Write a comprehensive PR summary
4. Include a test plan with TODOs
5. Push with `-u` flag for new branches
