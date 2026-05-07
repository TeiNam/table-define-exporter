# Documentation Rules

## Write for Readers, Not Authors

- Assume the reader is smart but unfamiliar with the context
- Start with what the thing does, not how it was built
- Prefer concrete examples over abstract descriptions
- Show the happy path first, edge cases after

## README Structure

Every project README follows this order:

1. **Title + one-line description** — what this does, who it's for
2. **Badges** (optional) — CI, license, version
3. **Overview** — 2-5 sentences, no hype
4. **Quick start** — shortest runnable example
5. **Installation** — exact commands per platform
6. **Usage** — common tasks with code
7. **Configuration** — env vars and defaults
8. **Development** — how to build, test, contribute
9. **License**

Keep the first screen (~30 lines) informative enough that a reader can decide whether to continue.

## Overview Section

- Answer: "What does this do? Who would use it? What problem does it solve?"
- Maximum 5 sentences
- No marketing language ("game-changer", "revolutionary", "cutting-edge")
- No roadmap, no history, no author bios

## Code Examples in Docs

- Every example must be runnable as-is
- Include imports, setup, and cleanup
- Show input and expected output
- Use realistic values, not `foo` / `bar` unless generic is intentional
- Match the language the project actually uses

Bad:

```python
result = process(data)
```

Good:

```python
from mylib import parse_invoice

invoice = parse_invoice(open("sample.pdf", "rb"))
print(invoice.total)
# 142.50
```

## Inline Comments

Comment only what the code cannot say:

- **Why** a non-obvious decision was made
- **Constraints** that force unusual approaches
- **Links** to issues, RFCs, or specs that explain context
- **Warnings** about subtle bugs or gotchas

Do not comment what the code already says:

```python
# BAD: obvious
# increment counter by 1
counter += 1

# GOOD: explains why
# Offset by 1 because the API returns 0-indexed but users expect 1-indexed
counter += 1
```

## Function and Class Documentation

Document the **contract**, not the implementation:

- What it does (one line)
- Parameters (type, meaning, constraints)
- Return value (type, meaning, when null/empty)
- Exceptions or errors raised
- Side effects (writes, network calls, state changes)

Skip docstrings on trivial getters, setters, and internal helpers.

## API Documentation

For REST/RPC APIs:

- One page per endpoint
- Include: method, path, auth required, request shape, response shape, error codes, curl example
- Keep examples working (CI tests the examples if possible)
- Link to related endpoints

## Changelog

Follow [Keep a Changelog](https://keepachangelog.com):

- Group by: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`
- Most recent version at the top
- Each entry is one line, past-tense verb first
- Link to PRs or issues
- Never rewrite history; add new entries

## Architecture Documentation

For systems larger than one file:

- **Decision records** — one per significant choice, document context + decision + consequences
- **Diagrams** for data flow and service boundaries (prefer Mermaid or simple ASCII)
- **Runbook** for operational tasks (deploy, rollback, rotate secrets)

Keep diagrams in sync with code or delete them. Stale diagrams mislead.

## When to Skip Documentation

- Single-use scripts where the code is shorter than the docs would be
- Experiments or prototypes clearly marked as such
- Private helpers used in one place

## When to Update Documentation

- Any change to public API, CLI flags, or config
- Any change to install or setup steps
- Breaking changes (always, with migration guide)
- Removing or renaming anything users touch

## Review Checklist

Before shipping docs, verify:

- [ ] Examples actually run
- [ ] Links are not broken
- [ ] Commands match the current version
- [ ] No placeholder text (`TODO`, `your-value-here`)
- [ ] Terminology is consistent across the doc
- [ ] First paragraph answers "what is this?"

## Anti-patterns

- Writing docs that simply restate the code
- "See the code for details" (the whole point of docs is to not read the code)
- Generated API docs with no examples or context
- README that reads like a changelog
- Hype language that delays useful information
