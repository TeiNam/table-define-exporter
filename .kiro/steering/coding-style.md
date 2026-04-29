# Coding Style

## Immutability

Prefer creating new objects over mutating existing ones:
- Prevents hidden side effects and makes data flow easier to trace
- Enables safe concurrency and simpler debugging
- Language note: some languages (Go, Rust) have idiomatic mutation patterns — language-specific rules take precedence

## File Organization

- Organize by feature or domain, not by file type
- Keep files focused on a single responsibility — 200-400 lines typical, 800 max
- Extract reusable utilities when the same logic appears in 3+ places

## Error Handling

- Handle errors explicitly — never silently swallow errors that affect correctness
- Provide user-friendly messages in UI-facing code; log detailed context server-side
- It's acceptable to ignore errors on best-effort operations (optional telemetry, non-critical retries) with a brief comment explaining why

## Input Validation

- Validate at system boundaries (API endpoints, user input, external data sources)
- Use schema-based validation where available (Zod, Pydantic, JSON Schema, etc.)
- Fail fast with clear error messages on invalid input
- Never trust external data — API responses, user input, file content all need validation

## Code Quality

Before marking work complete:
- [ ] Code is readable with descriptive names
- [ ] Functions have a single clear purpose
- [ ] Files are focused and not overly long
- [ ] Nesting is manageable (refactor if hard to follow)
- [ ] Errors are handled appropriately
- [ ] Magic values are extracted to named constants or config

