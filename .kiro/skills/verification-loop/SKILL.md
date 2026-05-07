---
name: verification-loop
description: "A comprehensive Rust verification system (cargo build, clippy, test, coverage, audit) for code quality assurance."
origin: harness
---

# Verification Loop Skill (Rust)

A comprehensive verification system for Rust code quality assurance.

## When to Use

Invoke this skill:
- After completing a feature or significant code change
- Before creating a PR
- When you want to ensure quality gates pass
- After refactoring

## Verification Phases

### Phase 1: Build Verification

```bash
cargo build --all-targets 2>&1 | tail -30
```

If build fails, STOP and fix before continuing.

### Phase 2: Type & Borrow Check

```bash
cargo check --all-targets 2>&1 | tail -30
```

Faster than `cargo build`; catches type and borrow-checker errors without codegen. Fix every error before the next phase.

### Phase 3: Lint (Clippy)

```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -40
```

Treat warnings as errors. Justify any `#[allow(...)]` with a comment.

### Phase 4: Format Check

```bash
cargo fmt --all --check 2>&1 | head -30
```

If the check fails, run `cargo fmt --all` to fix.

### Phase 5: Test Suite

```bash
cargo test --all-features 2>&1 | tail -50
```

Report:
- Total tests: X
- Passed: X
- Failed: X
- Ignored: X

Run doc tests too:

```bash
cargo test --doc 2>&1 | tail -20
```

### Phase 6: Coverage

```bash
# requires: cargo install cargo-llvm-cov
cargo llvm-cov --all-features --fail-under-lines 70 2>&1 | tail -30
```

Workspace target (per `steering/testing.md`): 70% on critical paths.

### Phase 7: Security & Hygiene Scan

```bash
# Dependency vulnerabilities
cargo audit 2>&1 | tail -20

# Leftover debug output in production code (exclude tests and main.rs CLI output)
grep -rn --include='*.rs' -E 'dbg!|println!|eprintln!' src/ \
  | grep -v '#\[cfg(test)\]' \
  | grep -v 'src/main.rs'

# Hardcoded secrets
grep -rn --include='*.rs' -E '(api_key|secret|password|token)\s*=\s*"' src/ | head -10

# unwrap/expect on non-test code
grep -rn --include='*.rs' -E '\.(unwrap|expect)\(' src/ \
  | grep -v '#\[cfg(test)\]' | head -20

# unsafe blocks without SAFETY comment
grep -rn --include='*.rs' -B1 'unsafe ' src/ | grep -v 'SAFETY' | head -10
```

### Phase 8: Diff Review

```bash
git diff --stat
git diff HEAD~1 --name-only
```

Review each changed file for:
- Unintended changes
- Missing error handling (`?` vs `unwrap`)
- Edge cases (empty input, NULL, multi-byte Unicode, `usize::MAX`)
- Public API changes without doc comments

## Output Format

After running all phases, produce a verification report:

```
VERIFICATION REPORT
==================

Build:     [PASS/FAIL]
Check:     [PASS/FAIL] (X errors)
Clippy:    [PASS/FAIL] (X warnings)
Fmt:       [PASS/FAIL]
Tests:     [PASS/FAIL] (X/Y passed)
Coverage:  [PASS/FAIL] (Z% — target 70%)
Audit:     [PASS/FAIL] (X vulns)
Hygiene:   [PASS/FAIL] (dbg!/unwrap/secrets)
Diff:      [X files changed]

Overall:   [READY/NOT READY] for PR

Issues to Fix:
1. ...
2. ...
```

## Quick Command Bundle

One-shot verification for a Rust crate:

```bash
cargo fmt --all --check \
  && cargo clippy --all-targets --all-features -- -D warnings \
  && cargo test --all-features \
  && cargo test --doc \
  && cargo audit
```

## Continuous Mode

For long sessions, run verification every 15 minutes or after major changes:

- After completing each function — `cargo check`
- After finishing a module — `cargo test --lib <module>`
- Before moving to next task — full loop above

## Integration with Hooks

This skill complements event-driven hooks (`diagnostics-on-save`, `review-on-stop`) but provides deeper verification. Hooks catch issues immediately; this skill provides comprehensive pre-PR review.
