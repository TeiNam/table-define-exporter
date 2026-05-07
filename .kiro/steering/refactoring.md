# Refactoring Rules

## When to Refactor

Refactor **during** regular work, not as a separate project:

- Before adding to code you don't understand — refactor until you do
- When a change touches the same area for the third time (rule of three)
- When tests pass but the code is hard to read or extend
- Right after green tests, before opening a PR

Do NOT refactor:

- During incidents or urgent fixes
- Right before a release freeze
- Code that works and nobody touches
- To impose a preferred style with no functional benefit

## The Golden Rule

**Never refactor without tests.**

If tests don't exist, add characterization tests first (capture current behavior), then refactor. Without tests, you're not refactoring — you're rewriting and hoping.

## Small Steps

Each refactor should be:

- Under 100 lines of diff
- One logical change
- Verifiable — tests still green, behavior unchanged
- Commitable on its own

If you can't commit it independently, it's too big. Break it down.

## Refactor vs Rewrite

| | Refactor | Rewrite |
|---|---|---|
| Behavior | Unchanged | May change |
| Size | Small, incremental | Large, from scratch |
| Risk | Low | High |
| Default choice | Yes | No — last resort |

Rewrite only when:
- The current architecture blocks all forward progress
- Cost of rewrite is lower than cost of maintenance
- There's a test suite to validate equivalence
- Leadership and stakeholders are aligned

## Common Refactors

### Extract Function
- A function does more than one thing — split it
- A code block needs a comment to explain — it wants to be a function

### Rename
- Names lie — variable says `count` but holds a list
- Names are vague — `data`, `info`, `result` with no context
- Rename immediately when you notice. Don't let bad names spread

### Remove Duplication
- Three or more copies — extract
- Two copies that might diverge — leave them alone until they clearly shouldn't
- Premature abstraction is worse than duplication

### Split Large Files
- Files over 400 lines are suspect
- Files over 800 lines almost always want to be split
- Split by responsibility, not by length

### Flatten Nesting
- More than 4 levels of nesting — extract or invert
- Use early returns to handle edge cases first
- Replace nested conditionals with guard clauses

### Simplify Conditions
- `if (x == true)` → `if (x)`
- Double negatives — rewrite positively
- Complex boolean expressions — name them (`const isEligible = ...`)

## Anti-patterns

### Rewriting Instead of Refactoring
Starting from scratch "because it's faster" almost never is. You'll hit the same edge cases and lose the institutional knowledge baked into the original.

### Refactoring Without Purpose
"I cleaned this up" is not a goal. "I extracted X to make adding Y easier" is.

### Big-Bang Refactors
Multi-week refactor branches that touch everything are a recipe for merge hell and hidden regressions. Land small changes continuously.

### Refactoring Other People's Active Code
Coordinate. Don't force others to rebase daily because you're reshuffling their area.

### Gold-Plating
Making things "cleaner" beyond what's needed. The goal is to make today's change easier, not to achieve perfection.

## Process

1. **Have a reason** — specific, written down: "Extracting validation makes the next feature 3 files instead of 7"
2. **Confirm tests pass** — before you start
3. **Make one change** — rename, extract, move, inline
4. **Run tests** — must still pass
5. **Commit** — small, descriptive message
6. **Repeat** — or stop if the reason is satisfied

If tests break, revert. Don't debug your refactor.

## Tooling

Use tools that preserve behavior automatically:

- IDE rename refactor (not find-and-replace)
- Extract method / extract variable shortcuts
- AST-based codemods for cross-repo changes (jscodeshift, rope, libcst)
- `git bisect` when a refactor introduces a subtle bug

## Code Smells to Watch For

| Smell | Refactor |
|---|---|
| Long function | Extract function |
| Long parameter list | Introduce parameter object |
| Duplicated code | Extract function / module |
| Large class | Extract class |
| Shotgun surgery (one change touches many files) | Move related logic together |
| Feature envy (method uses another class more than its own) | Move method |
| Primitive obsession (strings and ints where types should be) | Introduce type |
| Switch statements on type | Replace with polymorphism |
| Magic numbers | Replace with named constant |
| Comments explaining unclear code | Rename or extract until the comment is unnecessary |

## Review Checklist

Before merging a refactor:

- [ ] Behavior unchanged (tests prove it)
- [ ] Diff is small and focused
- [ ] Commit message explains the reason
- [ ] No new features bundled in
- [ ] No style-only changes mixed with logic changes
- [ ] Public API unchanged (or change documented)
