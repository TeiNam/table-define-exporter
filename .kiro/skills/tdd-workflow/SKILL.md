---
name: tdd-workflow
description: Test-driven development workflow for writing features, fixing bugs, or refactoring Rust code with comprehensive test coverage.
origin: harness
---

# Test-Driven Development Workflow (Rust)

## When to Activate

- Writing new functions, methods, or traits
- Fixing bugs (write a failing test first, then fix)
- Refactoring existing code (ensure tests exist before changing)
- Adding public API surface or service logic

## TDD Cycle

### 1. Write a Failing Test

Define expected behavior before writing implementation. Use `todo!()` as a placeholder so the test fails loudly instead of panicking with a type error.

```rust
// src/cart.rs
pub struct Item { pub price: i64 }

pub fn calculate_total(items: &[Item]) -> i64 {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_item_prices() {
        let items = [Item { price: 10 }, Item { price: 20 }];
        assert_eq!(calculate_total(&items), 30);
    }

    #[test]
    fn returns_zero_for_empty_list() {
        assert_eq!(calculate_total(&[]), 0);
    }

    #[test]
    fn ignores_items_with_negative_prices() {
        let items = [Item { price: 10 }, Item { price: -5 }];
        assert_eq!(calculate_total(&items), 10);
    }
}
```

### 2. Run Tests (They Should Fail)

```bash
cargo test --lib cart
```

Verify the test fails for the right reason — a missing implementation (panic from `todo!()`), not a compile error or a type mismatch.

### 3. Write Minimal Code to Pass

```rust
pub fn calculate_total(items: &[Item]) -> i64 {
    items.iter()
        .filter(|item| item.price > 0)
        .map(|item| item.price)
        .sum()
}
```

### 4. Refactor

With tests green, improve the code:
- Remove duplication
- Tighten names
- Replace loops with iterator chains
- Extract helpers

Run `cargo test` after each change to ensure green stays green.

### 5. Verify Coverage

```bash
cargo llvm-cov --lib           # summary
cargo llvm-cov --html          # HTML report
```

Focus on business logic, not boilerplate or `derive` macros.

## When TDD Helps Most

- **Well-defined contracts** — input/output clear
- **Bug fixes** — reproduce the bug as a failing test, then fix
- **Pure functions** — deterministic mapping
- **Parsers and validators** — explicit edge cases

## When Test-After Is Fine

- Exploratory prototypes
- CLI glue code in `main.rs`
- Rapidly evolving APIs still being shaped

## Test Types

### Unit Tests (`#[cfg(test)] mod tests` in each file)
- Individual functions, methods, and trait impls
- Fast, isolated, no external services
- Mock dependencies with `mockall` or trait substitution

### Integration Tests (`tests/` directory)
- Public API of the crate
- Real file I/O, real sqlx against a test database
- Each file in `tests/` is its own binary

### Doc Tests
- Examples inside `///` doc comments
- Double as user-facing documentation
- Run with `cargo test --doc`

## Testing Patterns

### Arrange-Act-Assert

```rust
#[test]
fn rejects_expired_tokens() {
    // Arrange
    let token = create_token(TokenOptions { expires_at: past_date() });

    // Act
    let result = validate_token(&token);

    // Assert
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), TokenError::Expired));
}
```

### Test Isolation

Each test builds its own fixtures. Tests must not depend on execution order.

```rust
#[test]
fn creates_user() {
    let user = make_test_user("alice");
    assert!(!user.id.is_empty());
}

#[test]
fn updates_user() {
    let user = make_test_user("alice"); // own setup, not reusing previous test
    let updated = update_user(&user.id, "Alice Smith").unwrap();
    assert_eq!(updated.name, "Alice Smith");
}
```

### Testing `Result`

```rust
#[test]
fn parse_succeeds_for_valid_input() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_config(r#"port = 8080"#)?;
    assert_eq!(config.port, 8080);
    Ok(())
}
```

### Property-Based Tests (`proptest`)

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn encode_decode_roundtrip(input in ".*") {
        let encoded = encode(&input);
        let decoded = decode(&encoded).unwrap();
        prop_assert_eq!(input, decoded);
    }
}
```

### Mocking Dependencies

Mock at trait boundaries, not inside implementations:

```rust
#[cfg_attr(test, mockall::automock)]
pub trait UserRepository {
    fn find_by_id(&self, id: u64) -> Option<User>;
}
```

## Common Mistakes

- **Testing implementation details** — assert on observable behavior, not private state
- **Shared mutable state across tests** — each test owns its setup
- **Unwrap-heavy tests** — use `?` in tests returning `Result` for clearer failure messages
- **Ignoring edge cases** — empty input, `usize::MAX`, multi-byte Unicode, error paths
- **`#[should_panic]` when `Result::is_err()` works** — prefer explicit error assertions

## Coverage Guidance

- Target 70%+ on critical paths (per workspace `testing.md`)
- Meaningful coverage beats a number — 70% of core logic > 95% of boilerplate
- Use `cargo llvm-cov` reports to find blind spots

## Commands Cheat Sheet

```bash
cargo test                          # all tests
cargo test test_name                # single test or pattern
cargo test --lib                    # unit tests only
cargo test --test integration       # one integration file
cargo test -- --nocapture           # show println output
cargo test --doc                    # doc tests only
cargo llvm-cov --fail-under-lines 70
```

For deeper patterns (rstest, async, criterion, CI setup), see the companion skill `rust-testing`.
