# Requirements Document: pgsql-support

## Introduction

`pgsql-support`는 기존 MySQL 전용 `td-export-rust` CLI 도구에 **PostgreSQL 지원을 추가**하는 피처입니다.
단일 바이너리를 유지하면서 `DbType` 열거형과 `DbClient` 트레이트를 도입하여 MySQL과 PostgreSQL을 동시에 지원합니다.

### 피처 목표

- **단일 바이너리**: 별도 바이너리 분리 없이 `--db-type` 플래그로 DB 종류를 선택
- **트레이트 기반 추상화**: `DbClient` 트레이트를 정의하고 `MySqlClient` / `PgClient`로 구현
- **PostgreSQL 특성 처리**: 스키마 계층(`database → schema → table`), 시스템 스키마 제외, 큰따옴표 식별자 인용, DDL 추출 방식 차이 처리
- **기존 MySQL 기능 완전 보존**: 기존 `td-export-rust` 요구사항(Requirements 1~15)은 변경 없이 유지

### PostgreSQL vs MySQL 주요 차이점

| 항목 | MySQL | PostgreSQL |
|------|-------|-----------|
| 스키마 계층 | `database == schema` | `database → schema → table` |
| 시스템 스키마 | `information_schema`, `mysql`, `sys`, `performance_schema`, `tmp` | `pg_catalog`, `information_schema`, `pg_toast`, `pg_temp_*` 등 |
| 식별자 인용 | 백틱 `` ` `` | 큰따옴표 `"` |
| 기본 포트 | 3306 | 5432 |
| DDL 추출 | `SHOW CREATE TABLE` | `pg_get_tabledef()` 또는 `information_schema` + `pg_catalog` 조합 |
| CAST 필요 여부 | `CAST(... AS CHAR)` 필요 | 불필요 |
| 뷰 정의 조회 | `SHOW CREATE TABLE` | `pg_get_viewdef()` |

## Glossary

- **DbType**: DB 종류를 나타내는 열거형. 값은 `MySql`, `Postgres` 두 가지입니다.
- **DbClient**: DB 연결 및 메타데이터 조회를 추상화하는 트레이트. `MySqlClient`와 `PgClient`가 구현합니다.
- **MySqlClient**: 기존 MySQL 전용 DB 클라이언트. `DbClient` 트레이트를 구현합니다.
- **PgClient**: PostgreSQL 전용 DB 클라이언트. `DbClient` 트레이트를 구현합니다.
- **PG_System_Schemas**: PostgreSQL에서 내보내기 대상에서 제외되는 시스템 스키마 목록. `pg_catalog`, `information_schema`, `pg_toast`, 그리고 `pg_temp_` 또는 `pg_toast_temp_` 접두어를 가진 모든 스키마.
- **PG_Schema**: PostgreSQL의 논리적 네임스페이스. 하나의 데이터베이스 안에 여러 스키마가 존재할 수 있습니다. MySQL의 `database`와 대응됩니다.
- **PG_Identifier_Quoter**: PostgreSQL 식별자를 큰따옴표(`"`)로 인용하고, 내부 큰따옴표를 이중 큰따옴표(`""`)로 이스케이프하는 유틸리티.
- **RunConfig**: 기존 구조체에 `db_type: DbType` 필드가 추가됩니다.
- **TD-EXPORT_CLI**: 기존과 동일한 최상위 실행 바이너리. `--db-type` 플래그가 추가됩니다.

## Requirements

### Requirement 1: DB 종류 선택 CLI 플래그 추가

**User Story:** DB 관리자로서, 기존 MySQL 워크플로를 유지하면서 `--db-type postgres` 플래그 하나로 PostgreSQL 대상 내보내기를 실행하고 싶습니다.

#### Acceptance Criteria

1. THE TD-EXPORT_CLI SHALL accept a `--db-type` flag with values `mysql` or `postgres` (case-insensitive).
2. WHEN the `--db-type` flag is omitted, THE TD-EXPORT_CLI SHALL default to `mysql` to preserve backward compatibility.
3. WHEN `--db-type mysql` is specified, THE TD-EXPORT_CLI SHALL use `MySqlClient` for all database operations.
4. WHEN `--db-type postgres` is specified, THE TD-EXPORT_CLI SHALL use `PgClient` for all database operations.
5. IF the `--db-type` value does not match `mysql` or `postgres`, THEN THE TD-EXPORT_CLI SHALL print an error message identifying the invalid value and exit with a non-zero status code.
6. WHEN `--db-type postgres` is specified and `--output` flag is omitted, THE TD-EXPORT_CLI SHALL use `excel` as the default output format (same as MySQL).
7. THE TD-EXPORT_CLI SHALL include `--db-type` in the `--help` output with supported values and default.

**Correctness Properties**

- *Round-trip*: `parse_db_type(format_db_type(t)) == t` — `DbType`을 문자열로 직렬화한 뒤 다시 파싱하면 원본과 같아야 한다.
- *Default backward compatibility*: `--db-type` 플래그 없이 실행했을 때의 동작은 기존 MySQL 전용 버전과 동일해야 한다.
- *Total*: 모든 입력 문자열에 대해 `parse_db_type`은 `Ok(_)` 또는 `Err(_)`를 반환하고 패닉하지 않는다.

---

### Requirement 2: PostgreSQL 기본 포트 및 접속 정보 입력

**User Story:** 사용자로서, PostgreSQL 접속 시 기본 포트가 5432로 안내되어 매번 포트를 직접 입력하지 않아도 되기를 원합니다.

#### Acceptance Criteria

1. WHEN `--db-type postgres` is specified and the TD-EXPORT_CLI prompts for `Port`, THE Config_Loader SHALL display `Port (default: 5432)` as the prompt text.
2. WHEN `--db-type postgres` is specified and the port input is empty, THE Config_Loader SHALL default the port to `5432`.
3. WHEN `--db-type mysql` is specified and the port input is empty, THE Config_Loader SHALL default the port to `3306` (existing behavior preserved).
4. THE Config_Loader SHALL validate the port range `1..=65535` regardless of `DbType`.
5. WHEN `--db-type postgres` is specified, THE Config_Loader SHALL prompt for `Database` (the PostgreSQL database name to connect to) as an additional required field after `Password`.
6. IF the `Database` input is empty when `--db-type postgres` is specified, THEN THE Config_Loader SHALL log an error and exit with a non-zero status code.
7. THE Config_Loader SHALL store the PostgreSQL database name in `RunConfig` for use in the connection URL.

**Correctness Properties**

- *Default port by DbType*: `DbType::Postgres`일 때 빈 포트 입력은 항상 `5432`를 반환하고, `DbType::MySql`일 때는 항상 `3306`을 반환한다.
- *Parse totality*: 임의 문자열 입력에 대해 포트 파서는 `Ok(u16)` 또는 `Err(_)`만 반환하며 패닉하지 않는다.

---

### Requirement 3: PostgreSQL 연결 확립 및 상태 검증

**User Story:** 사용자로서, PostgreSQL 서버에 연결하기 전에 접속 정보가 올바른지 조기에 확인하여 잘못된 설정으로 인한 시간 낭비를 피하고 싶습니다.

#### Acceptance Criteria

1. WHEN `--db-type postgres` is specified and a valid RunConfig is provided, THE PgClient SHALL open a PostgreSQL connection pool using the configured endpoint, port, user, password, and database name.
2. WHEN the PostgreSQL connection pool is established, THE PgClient SHALL execute `SELECT 1` as a readiness probe before any metadata query.
3. IF the PostgreSQL readiness probe fails or the connection cannot be opened within a configurable timeout, THEN THE PgClient SHALL return an error that wraps the underlying driver error and identifies the endpoint and port (without the password).
4. THE PgClient SHALL use `sqlx` with the `postgres` feature enabled in `Cargo.toml` (`sqlx = { version = "0.8", features = ["mysql", "postgres", "runtime-tokio", "macros"] }`).
5. WHEN the TD-EXPORT_CLI terminates (normally or due to an error), THE PgClient SHALL release all PostgreSQL connections.
6. THE PgClient SHALL apply a per-query timeout to all metadata queries to prevent indefinite blocking.

**Correctness Properties**

- *Error wrapping*: 기저 드라이버 오류가 발생했을 때 반환된 오류 체인에는 반드시 엔드포인트와 포트가 포함되며, 비밀번호 문자열은 포함되지 않는다.
- *Resource safety*: 임의의 성공/실패 시나리오(mock)에서 `PgClient` 드롭 후 보유한 연결 수는 0이다.

---

### Requirement 4: PostgreSQL 스키마 목록 수집

**User Story:** 사용자로서, PostgreSQL의 시스템 스키마(`pg_catalog`, `pg_toast` 등)는 자동으로 제외되고, 내가 지정한 스키마만 내보내지도록 하여 불필요한 메타데이터 수집을 피하고 싶습니다.

#### Acceptance Criteria

1. WHEN `--db-type postgres` is specified, THE PgClient SHALL query `information_schema.schemata` for the list of `schema_name` values within the connected database.
2. THE PgClient SHALL exclude schemas whose names are `pg_catalog`, `information_schema`, or `pg_toast`, or whose names begin with `pg_temp_` or `pg_toast_temp_`.
3. WHEN `RunConfig.target_db` is empty, THE PgClient SHALL return all non-system schemas in the connected database.
4. WHEN `RunConfig.target_db` contains schema names, THE PgClient SHALL restrict the result to schemas whose names appear in the list, using parameterized query bindings.
5. IF the resulting schema list is empty, THEN THE TD-EXPORT_CLI SHALL log an informational message stating "Not in Schema." and exit with a non-zero status code.
6. THE PgClient SHALL return schema names as a `Schema_Catalog` (map from schema name to an initially empty `Vec<TableDef>`).

**Correctness Properties**

- *PG system-schema exclusion invariant*: 임의의 스키마 집합 입력에 대해 반환 결과에는 `pg_catalog`, `information_schema`, `pg_toast`, `pg_temp_*`, `pg_toast_temp_*` 패턴의 스키마가 하나도 포함되지 않는다.
- *Target-DB subset*: `target_db`가 비어있지 않을 때, 반환된 스키마 목록은 `target_db`의 부분집합이다.
- *No SQL injection*: 임의 문자열(특수문자, 유니코드 포함)로 `target_db`를 구성해도 파라미터 바인딩으로 처리되어 SQL 주입이 발생하지 않는다.

---

### Requirement 5: PostgreSQL 테이블/뷰 목록 및 일반 정보 수집

**User Story:** 사용자로서, PostgreSQL 스키마 내의 테이블과 뷰를 일반 정보(테이블 타입, 주석)와 함께 수집하여 정의서에 포함시키고 싶습니다.

#### Acceptance Criteria

1. WHEN `--db-type postgres` is specified, FOR each schema in the Schema_Catalog, THE PgClient SHALL query `information_schema.tables` with `table_schema` bound to the schema name.
2. THE PgClient SHALL retrieve `table_name` and `table_type` per row; `engine` and `row_format` SHALL be set to `None` as PostgreSQL does not have these concepts.
3. THE PgClient SHALL retrieve table comments by joining `pg_catalog.pg_class` and `pg_catalog.pg_description` using `obj_description(pg_class.oid, 'pg_class')`.
4. WHEN `RunConfig.except_tables` is non-empty, THE PgClient SHALL add `AND table_name NOT LIKE $N` clauses using parameterized bindings (PostgreSQL uses `$1`, `$2`, ... placeholders).
5. THE PgClient SHALL map absent table comments to `Option::None` in `TableDef.general.comment`.
6. THE PgClient SHALL preserve the original row order returned by PostgreSQL (ordered by `table_name`).

**Correctness Properties**

- *NULL mapping*: 임의의 `(NullableString, bool is_null)` 샘플에 대해 `is_null == true ⇔ mapped == None`.
- *Engine/RowFormat always None for PG*: `PgClient`가 반환하는 모든 `TableDef`에서 `general.engine`과 `general.row_format`은 항상 `None`이다.

---

### Requirement 6: PostgreSQL 컬럼/인덱스/제약 조건 수집

**User Story:** 사용자로서, PostgreSQL 테이블의 컬럼 정의, 인덱스, 외래 키 제약 조건을 정확하게 수집하여 MySQL과 동일한 형식의 정의서를 만들고 싶습니다.

#### Acceptance Criteria

1. WHERE `table_type == "BASE TABLE"` and `--db-type postgres`, THE PgClient SHALL query `information_schema.columns` with bindings `(table_schema, table_name)` ordered by `ordinal_position`.
2. THE PgClient SHALL populate `ColumnInfo` with `column_name`, `column_default`, `is_nullable`, `data_type` (using `udt_name` for user-defined types), `character_set_name`, `collation_name`; `column_key` and `extra` SHALL be populated from `pg_catalog` as described below.
3. THE PgClient SHALL determine `column_key` by querying `pg_catalog.pg_index` and `pg_catalog.pg_attribute`: `"PRI"` if the column is part of the primary key, `"UNI"` if part of a unique index, `"MUL"` if part of a non-unique index, otherwise `None`.
4. THE PgClient SHALL determine `extra` by checking `pg_catalog.pg_attribute.attidentity` (`'a'` → `"auto_increment"`, `'d'` → `"auto_increment default"`) and `pg_catalog.pg_attrdef` for generated columns (`attgenerated == 's'` → `"GENERATED ALWAYS AS STORED"`).
5. WHERE `table_type == "BASE TABLE"` and `--db-type postgres`, THE PgClient SHALL query `pg_catalog.pg_indexes` (or `information_schema.statistics` equivalent) to retrieve non-primary indexes, grouped by index name with columns joined by comma.
6. THE PgClient SHALL populate `IndexInfo` with `index_name`, `non_unique` (0 for unique indexes, 1 for non-unique), and comma-joined column list ordered by position.
7. WHERE `table_type == "BASE TABLE"` and `--db-type postgres`, THE PgClient SHALL query `information_schema.referential_constraints` joined with `information_schema.key_column_usage` to retrieve foreign key constraints.
8. THE PgClient SHALL populate `ConstInfo` with `constraint_name`, comma-joined `column_name`, `CONCAT(referenced_table_name, '.', referenced_column_name)`, `delete_rule`, and `update_rule`.
9. IF a column/index/constraint query fails for one table, THEN THE TD-EXPORT_CLI SHALL log the error with schema and table name and continue processing the remaining tables.

**Correctness Properties**

- *Ordinal order preservation*: 반환된 `columns` 리스트는 `ordinal_position` 오름차순으로 정렬되어 있다(단조 증가).
- *Per-table failure isolation*: 임의의 테이블 리스트에 대해 하나의 테이블에서 오류가 발생해도 나머지 테이블의 수집 결과는 영향을 받지 않는다.

---

### Requirement 7: PostgreSQL 뷰 정의 수집

**User Story:** 사용자로서, PostgreSQL `VIEW` 타입 객체의 원본 뷰 정의 SQL을 수집하여 뷰 정의서를 만들 수 있기를 원합니다.

#### Acceptance Criteria

1. WHERE `table_type == "VIEW"` and `--db-type postgres`, THE PgClient SHALL call `pg_get_viewdef('"schema"."view_name"', true)` with schema and view names safely quoted with double-quotes to retrieve the view definition.
2. THE PgClient SHALL populate `ViewInfo.view_query` with the result of `pg_get_viewdef()`; `ViewInfo.charset` and `ViewInfo.collate` SHALL be set to empty strings as PostgreSQL does not expose per-view charset/collation in the same way.
3. IF the `pg_get_viewdef()` call fails for a view, THEN THE TD-EXPORT_CLI SHALL log the error with schema and view name and continue processing the remaining objects.
4. THE PgClient SHALL not populate `ColumnInfo`/`IndexInfo`/`ConstInfo` for views.

**Correctness Properties**

- *PG identifier safety*: 임의의 스키마/뷰 이름 문자열(큰따옴표, 세미콜론, `--` 등 포함)에 대해, 생성되는 SQL은 PostgreSQL 식별자 인용 규칙을 준수하며 주입 공격이 성공하지 않는다.

---

### Requirement 8: PostgreSQL DDL 수집 (SQL 포맷 전용)

**User Story:** DB 관리자로서, PostgreSQL 테이블의 DDL을 수집하여 재현용 SQL 스크립트를 만들 수 있기를 원합니다.

#### Acceptance Criteria

1. WHERE `RunConfig.output == Sql` and `--db-type postgres`, FOR each table in the schema, THE PgClient SHALL attempt to retrieve DDL using `pg_get_tabledef('"schema"."table"')` if the function is available in the connected PostgreSQL instance.
2. IF `pg_get_tabledef()` is not available, THEN THE PgClient SHALL reconstruct a `CREATE TABLE` DDL string from `information_schema.columns` and `pg_catalog` metadata as a fallback, producing a syntactically valid PostgreSQL DDL.
3. THE PgClient SHALL store the retrieved or reconstructed DDL string in `TableDef.ddl`.
4. WHERE `RunConfig.output == Sql` and `--db-type postgres`, THE PgClient SHALL skip column/index/constraint metadata queries for the same table (DDL-only mode).
5. WHERE `RunConfig.output ∈ {Excel, Markdown}` and `--db-type postgres`, THE PgClient SHALL not populate `TableDef.ddl`.

**Correctness Properties**

- *DDL syntactic validity*: 재구성된 DDL 문자열은 PostgreSQL 파서가 오류 없이 파싱할 수 있는 유효한 `CREATE TABLE` 구문이어야 한다(mock 파서 또는 sqlx prepare 기반 검증).
- *DDL non-empty*: 유효한 테이블에 대해 반환된 `TableDef.ddl`은 `Some(s)`이며 `s`는 비어있지 않다.

---

### Requirement 9: PostgreSQL 식별자 인용 (큰따옴표)

**User Story:** 보안 담당자로서, PostgreSQL에서 사용자가 제공하는 스키마/테이블 이름이 SQL 주입 공격에 악용되지 않도록 큰따옴표 인용 방식으로 안전하게 처리되기를 원합니다.

#### Acceptance Criteria

1. THE PG_Identifier_Quoter SHALL quote PostgreSQL identifiers with double-quotes (`"`), escaping any embedded double-quote as `""` (two consecutive double-quotes).
2. WHERE a query requires a schema or table identifier in a non-parameterizable position (e.g., `pg_get_viewdef`, `pg_get_tabledef`, dynamic schema-qualified names), THE PgClient SHALL use `PG_Identifier_Quoter` to quote the identifier before embedding it in the SQL string.
3. IF an identifier contains a null byte (`\0`), THEN THE PG_Identifier_Quoter SHALL return an error without producing a quoted string.
4. THE PG_Identifier_Quoter SHALL be implemented as a separate function from the existing MySQL backtick quoter in `identifier.rs`, sharing the same module but with distinct function names (`quote_pg_identifier` vs `quote_identifier`).
5. THE PgClient SHALL never use backtick quoting for PostgreSQL identifiers.

**Correctness Properties**

- *PG identifier round-trip*: 임의의 유효한 PostgreSQL 식별자 문자열 `id`에 대해, `unquote_pg_identifier(quote_pg_identifier(id)) == id`.
- *Injection resistance*: 임의의 악성 입력 문자열(`"; DROP TABLE ...`, 큰따옴표 주입, 유니코드 호모글리프 등)에 대해 `PG_Identifier_Quoter`는 주입에 성공하지 않는다(실행 전 거부 또는 안전하게 인용 처리).
- *Backtick never appears in PG SQL*: `PgClient`가 생성하는 모든 SQL 문자열에는 백틱(`` ` ``)이 포함되지 않는다.

---

### Requirement 10: DbClient 트레이트 추상화

**User Story:** 개발자로서, MySQL과 PostgreSQL 클라이언트가 동일한 트레이트 인터페이스를 구현하여 `main.rs`와 `Exporter`가 DB 종류에 무관하게 동작하기를 원합니다.

#### Acceptance Criteria

1. THE TD-EXPORT_CLI SHALL define a `DbClient` trait with the following async methods: `connect`, `get_schemas`, `get_tables`, `get_columns`, `get_indexes`, `get_constraints`, `get_view_info`, `get_table_ddl`.
2. THE `MySqlClient` SHALL implement the `DbClient` trait, wrapping the existing MySQL implementation with no behavioral change.
3. THE `PgClient` SHALL implement the `DbClient` trait with PostgreSQL-specific query logic.
4. THE `main.rs` pipeline SHALL accept a `Box<dyn DbClient>` (or equivalent trait object / enum dispatch) so that the same orchestration code handles both MySQL and PostgreSQL.
5. WHEN `--db-type mysql` is selected, THE TD-EXPORT_CLI SHALL instantiate `MySqlClient`; WHEN `--db-type postgres` is selected, THE TD-EXPORT_CLI SHALL instantiate `PgClient`.
6. THE `DbClient` trait SHALL be `Send + Sync` to support async runtimes.
7. THE existing `Exporter` trait and its implementations (ExcelExporter, MarkdownExporter, SqlExporter) SHALL require no changes to support PostgreSQL, as they operate on `TableDef` / `SchemaCatalog` which are DB-agnostic.

**Correctness Properties**

- *Trait object safety*: `DbClient` 트레이트는 object-safe해야 하며, `Box<dyn DbClient>`로 사용할 수 있어야 한다.
- *MySQL behavioral equivalence*: `MySqlClient`를 `DbClient` 트레이트로 래핑한 후의 동작은 기존 직접 구현과 동일해야 한다(동일 입력 → 동일 출력).

---

### Requirement 11: 출력 파일 포맷 호환성 (PostgreSQL)

**User Story:** DB 관리자로서, PostgreSQL에서 수집한 메타데이터가 MySQL과 동일한 Excel/Markdown/SQL 포맷으로 출력되어 기존 문서화 워크플로를 그대로 사용할 수 있기를 원합니다.

#### Acceptance Criteria

1. WHEN `--db-type postgres` and `--output excel` are specified, THE ExcelExporter SHALL produce an `.xlsx` file using the same layout as MySQL output; cells for `Engine` and `Row Format` SHALL be rendered as empty strings when the values are `None`.
2. WHEN `--db-type postgres` and `--output markdown` are specified, THE MarkdownExporter SHALL produce a `.md` file using the same section structure as MySQL output; `Engine` and `Row Format` cells SHALL be rendered as empty strings.
3. WHEN `--db-type postgres` and `--output sql` are specified, THE SqlExporter SHALL write the DDL string retrieved by `PgClient` using the same file format as MySQL: `/* Database : {schema} */` header, `/* Table : {table} */` per-table comment, `DROP TABLE IF EXISTS {table};`, and `{ddl};` followed by two blank lines.
4. THE output filename conventions SHALL remain identical regardless of `DbType`: `{endpoint}.xlsx`, `{schema}.md`, `{schema}({endpoint}).sql`.
5. WHERE `ViewInfo.charset` and `ViewInfo.collate` are empty strings (PostgreSQL), THE ExcelExporter and MarkdownExporter SHALL render those cells as empty strings without error.

**Correctness Properties**

- *Filename determinism*: 동일한 스키마 이름과 엔드포인트에 대해 `DbType`에 무관하게 파일명은 항상 동일한 규칙을 따른다.
- *None-to-empty-string rendering*: `Option::None` 필드는 출력 파일에서 항상 빈 문자열로 렌더링되며, 패닉이나 오류를 발생시키지 않는다.

---

### Requirement 12: 로깅 및 진행 상황 보고 (PostgreSQL)

**User Story:** 운영자로서, PostgreSQL 내보내기 실행 시에도 MySQL과 동일한 형식의 진행 상황 로그를 확인하여 실행 상태를 파악하고 싶습니다.

#### Acceptance Criteria

1. WHEN `--db-type postgres` is specified and the DB connection is successfully established, THE Logger SHALL emit an `INFO` message `DB Connect Success` (same as MySQL).
2. WHEN `--db-type postgres` is specified and schema discovery completes, THE Logger SHALL emit `INFO` messages `Get Schema Count : {n}` and, for each schema, `{schema} Table Load.` and `{schema} Table Count : {n}`.
3. WHEN the export completes successfully with `--db-type postgres`, THE Logger SHALL emit `INFO` message `Export Complete.`.
4. IF any step fails with an unrecoverable error during PostgreSQL export, THEN THE Logger SHALL emit an `ERROR` level message with the error chain and the process SHALL exit with a non-zero status code.
5. THE Logger SHALL never emit the password value at any log level, regardless of `DbType`.

**Correctness Properties**

- *Password redaction*: 임의의 비밀번호(ASCII 비인쇄 문자 포함)에 대해, PostgreSQL 연결 오류 메시지를 포함한 전체 로그 출력에는 해당 문자열이 나타나지 않는다.

---

### Requirement 13: 비기능적 요구사항 (PostgreSQL 지원 추가)

**User Story:** 운영자로서, PostgreSQL 지원이 추가된 후에도 기존 빌드 타겟, 성능 기준, 코드 품질 기준이 유지되기를 원합니다.

#### Acceptance Criteria

1. THE TD-EXPORT_CLI SHALL build successfully on stable Rust (MSRV 1.75 이상) for all existing targets (`x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`) after adding PostgreSQL support.
2. THE `Cargo.toml` SHALL add `"postgres"` to the `sqlx` features list: `sqlx = { version = "0.8", features = ["mysql", "postgres", "runtime-tokio", "macros"] }`.
3. WHERE a PostgreSQL schema has up to 1,000 tables and each table has up to 100 columns, THE TD-EXPORT_CLI SHALL complete the export on a local network within a time budget of 120 seconds (excluding DB server-side time), using connection reuse.
4. THE TD-EXPORT_CLI SHALL reuse a single PostgreSQL connection pool (minimum 1, maximum configurable, default 4) across metadata queries.
5. THE TD-EXPORT_CLI SHALL handle PostgreSQL schema and table names containing Unicode characters (including Korean, Japanese, Chinese) correctly in both queries and output files.
6. THE TD-EXPORT_CLI SHALL pass `cargo clippy --all-targets --all-features -- -D warnings` after adding PostgreSQL support.
7. THE `README.md` SHALL be updated to document the new `--db-type` flag, PostgreSQL connection requirements, and the `Database` prompt added for PostgreSQL.
8. THE existing MySQL behavior SHALL remain unchanged: all existing tests SHALL continue to pass after the PostgreSQL feature is added.

**Correctness Properties**

- *Existing tests green*: PostgreSQL 지원 추가 후 기존 `td-export-rust` 스펙의 모든 속성 기반 테스트(Property 1~16)는 변경 없이 통과해야 한다.
- *Unicode preservation (PG)*: 임의의 유니코드 PostgreSQL 스키마/테이블 이름에 대해, 수집된 이름을 파일에 기록한 뒤 다시 읽어들이면 원본과 바이트 단위로 일치한다.
- *Connection pool invariant (PG)*: N개 테이블에 대한 PostgreSQL 메타데이터 조회 동안 생성된 물리 커넥션 수는 설정된 최대 풀 크기를 초과하지 않는다.

---

## Parser/Serializer Round-trip Note

본 스펙에서 parser/serializer 쌍으로 간주되어 왕복 속성 테스트를 권장하는 변환:

- `DbType` ↔ `String` (Requirement 1): `parse_db_type(format_db_type(t)) == t`
- PostgreSQL 식별자 ↔ 큰따옴표 인용 식별자 (Requirement 9): `unquote_pg_identifier(quote_pg_identifier(id)) == id`
- PostgreSQL DDL 문자열 ↔ SQL 파일 내 DDL 블록 (Requirement 8, 11): 기존 MySQL DDL 왕복 속성과 동일한 패턴 적용

각 쌍에 대해 `parse(format(x)) == x` 또는 의미 동등성(semantic equality) 속성을 property-based test로 검증해야 합니다.

## 기존 요구사항과의 관계

본 스펙(`pgsql-support`)은 기존 `td-export-rust` 스펙의 **Requirements 1~15를 변경하지 않습니다**. 기존 요구사항은 MySQL 동작의 명세로 그대로 유지되며, 본 스펙은 PostgreSQL 지원을 위한 **추가(additive) 요구사항**만을 정의합니다.

| 기존 요구사항 | 본 스펙에서의 처리 |
|-------------|-----------------|
| Req 1: CLI 인터페이스 | Req 1 (본 스펙): `--db-type` 플래그 추가 |
| Req 2: 대화식 입력 | Req 2 (본 스펙): PostgreSQL 기본 포트 5432, `Database` 프롬프트 추가 |
| Req 3: MySQL 연결 | Req 3 (본 스펙): PostgreSQL 연결 추가 (`PgClient`) |
| Req 4: 스키마 수집 | Req 4 (본 스펙): PostgreSQL 시스템 스키마 제외 규칙 추가 |
| Req 5~8: 메타데이터 수집 | Req 5~8 (본 스펙): PostgreSQL 전용 쿼리 로직 추가 |
| Req 9~11: 출력 포맷 | Req 11 (본 스펙): PostgreSQL 출력 호환성 확인 |
| Req 14: SQL 주입 방지 | Req 9 (본 스펙): PostgreSQL 큰따옴표 식별자 인용 추가 |
| Req 15: 비기능 요구사항 | Req 13 (본 스펙): PostgreSQL 추가 후 기준 유지 |
