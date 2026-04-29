# Product Development (Solo Developer)

## Principles
- **KISS / YAGNI / DRY**: Keep it simple, build only what's needed now, extract duplication (no over-abstraction)
- Make it work → Make it right → Make it fast (only when needed)
- Ship over perfect. Iterate based on usage data

## Workflow Constraints
- **Never run code directly via terminal**
- **Never use `cat <<EOF` to create files** — use fsWrite or fsAppend tools instead
- **Never pipe long text through terminal commands** — use fsWrite or fsAppend tools instead
- Never commit commented-out code, console.log, or print statements
- Comment only non-obvious code

## Automation
- Formatting (Black, Prettier), linting (Pylint, ESLint)
- Tests (on every push), deploy (one-click), backups (scheduled)

## Acceptable Rule Relaxation
- Prototyping: skip tests OK
- Debugging: temporary print OK
- Exploring patterns: copy-paste OK

