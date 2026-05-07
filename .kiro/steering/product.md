# Product Development (Solo Developer)

## Principles
- **KISS / YAGNI / DRY**: Keep it simple, build only what's needed now, extract duplication (no over-abstraction)
- Make it work → Make it right → Make it fast (only when needed)
- Ship over perfect. Iterate based on usage data

## Workflow Constraints
- **Never run code directly via terminal**
- **Never use `cat <<EOF` to create files** — use fsWrite or fsAppend tools instead
- **Never pipe long text through terminal commands** — use fsWrite or fsAppend tools instead
- Never commit commented-out code or `println!` debug statements
- Comment only non-obvious code

## Automation
- Formatting (`cargo fmt`), linting (`cargo clippy`)
- Tests (`cargo test` on every push)
- `cargo audit` for dependency vulnerabilities

## Acceptable Rule Relaxation
- Prototyping: skip tests OK
- Debugging: temporary `dbg!()` OK
- Exploring patterns: copy-paste OK
