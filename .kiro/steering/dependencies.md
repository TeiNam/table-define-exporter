# Dependency Management Rules

## Core Principles

- **Every dependency is a liability** — code you don't own, can't fix, and must keep updated
- **Prefer the standard library first** — then well-known packages, custom code last
- **Pin versions** — reproducible builds matter more than getting the latest
- **Audit before adding** — a five-minute review now saves hours of debugging later

## Before Adding a Dependency

Ask these questions:

1. **Is it in the standard library?** Check first. Often the answer is yes
2. **How active is it?** Commits in the last 6 months, issues being resolved
3. **How big is the blast radius?** Transitive dependency count matters
4. **Is it well maintained?** Look at bus factor, sponsor activity, release cadence
5. **Can I write this in under 100 lines?** If yes, and the dep is heavy, write it
6. **Does it duplicate something already in the project?** Don't add overlapping libs

## Version Pinning

Always pin to an **exact version** in lockfiles:

- `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock` — commit these
- `Cargo.lock` — commit for binaries, skip for libraries
- `requirements.txt` — use `==`, or lock via `uv lock` / `pip-tools`
- `go.sum` — commit this
- `Gemfile.lock` — commit this

In `package.json` / `pyproject.toml` / `Cargo.toml` itself, you may use ranges (`^1.2.3`) but the lockfile is the source of truth.

## Updating Dependencies

**Monthly cadence** (or after incidents):

1. Run the audit tool (`npm audit`, `pip audit`, `cargo audit`)
2. Update one package at a time for non-trivial changes
3. Run the full test suite after each update
4. Commit separately: `chore(deps): bump axios from 1.6.0 to 1.7.2`
5. Deploy to staging before prod

**Never**:
- Update all dependencies at once without testing
- Skip the lockfile update
- Pin to a git hash unless upstream is abandoned

## Handling Vulnerabilities

- **Critical / High CVE** — patch within 48 hours
- **Medium** — patch in next weekly cycle
- **Low** — patch in next monthly cycle

If the vulnerable package has no fix:
- Replace it
- Pin to a safer version even if older
- Document the decision

## Supply Chain Security

- Verify package names — typosquatting is common (`event-stream`, `request-promise-native`, etc.)
- Prefer packages from known publishers and verified publishers
- Check npm/PyPI publisher matches the GitHub org
- For critical infrastructure, use lockfile integrity verification (`npm ci`, `uv sync --frozen`)
- Enable Dependabot or Renovate for automated PRs with change logs

## Transitive Dependencies

Audit the full tree, not just direct deps:

- `npm ls` / `pnpm ls` / `yarn why`
- `cargo tree`
- `pipdeptree`
- `go mod graph`

A small direct dep with 200 transitive deps is usually worse than a medium-sized direct dep with 20.

## Dev Dependencies vs Runtime

- Keep dev deps separate (`devDependencies`, `dev = true`, `[dev-dependencies]`)
- Build tools, linters, test runners don't ship to production
- Runtime deps must pass stricter review

## Banned Patterns

- Installing packages globally on dev machines — use per-project
- Using `npm install` in CI (use `npm ci` or equivalent)
- `rm -rf node_modules && npm install` as a fix strategy — diagnose first
- Adding a dep to use one function from it
- Fork-and-forget — always document why a fork exists

## Language-Specific Notes

### Node.js
- Prefer `pnpm` or `npm ci` over `npm install` in CI
- Set `"engines"` in `package.json` and enforce in CI
- Use `overrides` / `resolutions` only with a linked issue

### Python
- Use `uv` or `poetry` for new projects
- `requirements.txt` without pinning is a recipe for "works on my machine"
- Separate `requirements.txt` (app) from `requirements-dev.txt`

### Rust
- `cargo audit` in CI
- Avoid `*` or `>=` in published crates
- Minimum Supported Rust Version (MSRV) in `Cargo.toml`

### Go
- Keep `go.mod` tidy (`go mod tidy` in CI)
- Vendor only for air-gapped builds
- Use `//go:build` tags to keep platform-specific deps out of builds that don't need them

## Removal

Removing a dep is easier than adding one. If usage is trivial:
- Check `npm ls <pkg>` / `cargo tree` for transitive usage first
- Remove imports, run tests, delete from manifest
- Commit the lockfile update in the same commit

## Review Checklist

Before merging a PR that adds or updates a dep:

- [ ] Why was this added? (PR description answers it)
- [ ] Is the version pinned?
- [ ] Lockfile updated?
- [ ] Security audit clean?
- [ ] License compatible with the project?
- [ ] Tests still pass?
- [ ] Build size impact checked (for frontend)
