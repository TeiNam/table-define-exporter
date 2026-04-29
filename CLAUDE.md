# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This project exports MySQL table/view metadata from `information_schema` into three formats: Excel (.xlsx), Markdown (.md), or SQL DDL (.sql). There are two implementations:

- **`td-export(Go)/`** — Fully functional Go v0.2.0 reference implementation
- **`td-export-rust/`** — Rust port, currently in planning/design phase (not yet implemented)

Design specs for the Rust port live in `.kiro/specs/td-export-rust/` (requirements.md, design.md, tasks.md).

## Go Version

```bash
cd "td-export(Go)"
go mod tidy
go build -o TD-EXPORT
./TD-EXPORT -output=markdown   # or -output=excel / -output=sql
```

No automated test suite exists in the Go version — testing is manual via interactive prompts.

## Rust Version (when implemented)

```bash
cd td-export-rust
cargo build --release
cargo test --all-features
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Architecture

### Data Flow (both versions)

1. **CLI/config** — Collects: endpoint, port, user, password, target schemas, excluded tables, output format
2. **db layer** — Connects to `information_schema`, queries schemas → tables → columns/indexes/constraints/views
3. **Exporters** — Receive a slice of `PerTable` structs per schema, write formatted output

### Core Data Model

- `PerTable` — One table or view: `TableName`, `GeneralInfo`, `Columns[]`, `Indexes[]`, `Constraints[]`, optional `View` and `DDL`
- `GeneralInfo` — `TableType`, `Engine`, `RowFormat`, `Collation`, `Comment`
- `ColumnInfo`, `IndexInfo`, `ConstInfo`, `ViewInfo` — Mirror their `information_schema` sources

### Output Naming

| Format   | Output                        |
|----------|-------------------------------|
| Excel    | `{endpoint}.xlsx` (one file, one sheet per schema) |
| Markdown | `{schema}.md` per schema      |
| SQL      | `{schema}({endpoint}).sql` per schema |

### Go Module Map

| File | Responsibility |
|------|---------------|
| `main.go` | Entry point, interactive prompts, pipeline orchestration |
| `lib/db.go` | MySQL connection, `information_schema` queries |
| `lib/excel.go` | Excel worksheet creation and styling |
| `lib/markdown.go` | Markdown table/list generation |
| `lib/sql.go` | DDL file generation |
| `lib/common.go` | `GetOpt()`, `PointerStr()` utilities |

### Planned Rust Module Map

| File | Responsibility |
|------|---------------|
| `src/main.rs` | clap CLI, pipeline orchestration |
| `src/config.rs` | Interactive prompts → `RunConfig` |
| `src/db.rs` | Async sqlx MySQL pool, parameterized queries |
| `src/model.rs` | `RunConfig`, `TableDef`, `OutputFormat` |
| `src/error.rs` | `AppError` enum (thiserror), password redaction |
| `src/identifier.rs` | Backtick-safe SQL identifier quoting |
| `src/export/{excel,markdown,sql}.rs` | Format-specific exporters behind a common trait |

## Key Design Constraints

**Output compatibility is strict.** The Rust port must produce byte-identical output to the Go version, including intentional quirks (e.g., `REFERNCES` typo in constraint headers, `Referance` in Markdown) preserved for parity.

**SQL injection prevention** — All user-supplied values (schema names, table names) must use `?` parameter binding in sqlx. Only pre-validated identifiers may be interpolated for `SHOW CREATE TABLE` statements, handled exclusively by `identifier.rs`.

**Password safety** — Passwords must never appear in logs, error messages, or debug output. The error module must redact them from error chains.

**Per-table failure isolation** — A single table failing to export must not abort the entire run; errors are logged and the export continues.

## Property-Based Testing (Rust)

The design specifies 16 correctness properties to verify with `proptest` (100+ iterations each), covering: `OutputFormat` round-trips, identifier quoting idempotency, DDL round-trips, Unicode preservation, NULL→None mapping, file naming determinism, and schema isolation. See `.kiro/specs/td-export-rust/design.md` for the full list.
