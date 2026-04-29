# Requirements Document

## Introduction

`td-export-rust`는 기존 Go로 작성된 **TD-EXPORT**(Table Definition Export) 도구를 Rust로 재구현하는 프로젝트입니다. 해당 도구는 MySQL에 연결하여 `information_schema`에서 테이블/뷰 메타데이터를 수집하고, 그 결과를 Excel(.xlsx), Markdown(.md), SQL(.sql) 중 선택한 포맷으로 내보내는 CLI 유틸리티입니다.

Rust 포팅의 목표는 다음과 같습니다.

- **기능 동등성(Feature Parity)**: 기존 Go 버전과 동일한 CLI 인터페이스, 동일한 입력 처리, 동일한 출력 파일 구조를 유지합니다.
- **출력 호환성**: Excel 시트 레이아웃, Markdown 섹션 구성, SQL 덤프 포맷이 기존 결과물과 의미적으로 동일해야 합니다(동일한 컬럼 순서, 동일한 제목, 동일한 파일명 규칙).
- **안전성과 성능**: Rust의 타입/메모리 안전성을 활용하여 NULL 처리 오류, SQL 주입, 자원 누수 없이 동작하며 비밀번호 등 민감 정보는 로그에 남기지 않습니다.
- **유지보수성**: `main`은 얇게 유지하고 DB 접근·포매터·공통 모듈을 분리하여 테스트 가능한 구조로 구성합니다.

## Glossary

- **TD-EXPORT_CLI**: Rust로 재구현된 최상위 실행 가능 바이너리. 사용자 입력을 받고 전체 파이프라인을 조율합니다.
- **Config_Loader**: CLI 플래그와 대화식 프롬프트로부터 실행 설정(엔드포인트/포트/사용자/비밀번호/대상 DB/제외 테이블/출력 포맷)을 수집하여 `RunConfig` 값으로 반환하는 컴포넌트입니다.
- **RunConfig**: 한 번의 실행에 필요한 모든 입력값(접속 정보, 대상 DB 목록, 제외 테이블 패턴, 출력 포맷 등)을 담는 불변 구조체입니다.
- **DB_Client**: MySQL과의 연결 및 `information_schema` 기반 메타데이터 조회를 담당하는 모듈입니다. 파라미터 바인딩을 사용합니다.
- **Schema_Catalog**: 스키마(DB) → 테이블 목록의 맵으로 표현되는 메타데이터 컨테이너입니다. `HashMap<String, Vec<TableDef>>` 형태입니다.
- **TableDef**: 한 테이블/뷰에 대한 메타데이터 집합(일반 정보, 컬럼, 인덱스, 제약, 뷰 정의, DDL)을 담는 구조체입니다. Go의 `PerTable`에 대응합니다.
- **ColumnInfo / IndexInfo / ConstInfo / ViewInfo / GeneralInfo**: `TableDef` 하위의 메타데이터 구조체들로, Go 버전과 동일한 의미론을 가집니다.
- **OutputFormat**: 출력 포맷을 나타내는 열거형. 값은 `Excel`, `Markdown`, `Sql` 세 가지입니다.
- **Exporter**: `OutputFormat`별 구현(Trait 구현체)으로, `TableDef` 컬렉션을 받아 대응하는 파일을 생성합니다.
- **ExcelExporter / MarkdownExporter / SqlExporter**: 각각 `.xlsx`, `.md`, `.sql` 출력을 담당하는 `Exporter` 구현체입니다.
- **Logger**: 구조화된 로그 출력을 담당하는 컴포넌트. Go의 `logrus`를 Rust `tracing` 또는 `log + env_logger`로 대체합니다.
- **Endpoint**: MySQL 서버의 호스트명 또는 IP 주소 문자열.
- **System_Schemas**: 내보내기에서 제외되는 MySQL 시스템 스키마 목록. `information_schema`, `mysql`, `sys`, `performance_schema`, `tmp`.
- **NonEmptyString**: 길이가 1 이상인 문자열을 보장하는 타입. Go 버전의 `*string` 비어있음 검사에 대응합니다.

## Requirements

### Requirement 1: CLI 인터페이스 및 출력 포맷 선택

**User Story:** DB 관리자로서, 기존 Go 버전과 동일한 CLI 플래그로 Rust 버전을 실행하여 Excel/Markdown/SQL 중 하나의 포맷으로 테이블 정의서를 내보내고 싶습니다. 이를 통해 기존 워크플로와 스크립트를 그대로 재사용할 수 있습니다.

#### Acceptance Criteria

1. THE TD-EXPORT_CLI SHALL accept a `--output` flag with values `excel`, `markdown`, or `sql`.
2. WHEN the `--output` flag is omitted, THE TD-EXPORT_CLI SHALL use `excel` as the default value.
3. WHEN the `--output` value (case-insensitive) matches one of `excel`, `markdown`, `sql`, THE TD-EXPORT_CLI SHALL select the corresponding Exporter.
4. IF the `--output` value does not match any supported format, THEN THE TD-EXPORT_CLI SHALL print an error message identifying the invalid value and exit with a non-zero status code.
5. WHEN `--help` or `-h` is provided, THE TD-EXPORT_CLI SHALL print usage information including application name, version, description, and supported `--output` values, then exit with status code 0.
6. WHEN `--version` or `-V` is provided, THE TD-EXPORT_CLI SHALL print the application name and semantic version string, then exit with status code 0.

**Correctness Properties (property-based testing 대상)**

- *Round-trip*: `parse_output_format(format_output_format(fmt)) == fmt` — `OutputFormat`을 문자열로 직렬화한 뒤 다시 파싱하면 원본과 같아야 한다.
- *Case-insensitive*: 임의의 대소문자 조합 문자열 `s`에 대해, `s.to_ascii_lowercase()`가 지원 목록에 있으면 `parse_output_format(s)`는 성공해야 한다.
- *Total*: 모든 입력 문자열에 대해 `parse_output_format`은 `Ok(_)` 또는 `Err(_)`를 반환하고, 패닉을 일으키지 않는다.

---

### Requirement 2: 대화식 접속 정보 입력

**User Story:** 사용자로서, 실행 시 엔드포인트·포트·사용자·비밀번호·대상 DB·제외 테이블을 대화식으로 입력하여 자동화 스크립트와 수동 실행 모두를 지원하고 싶습니다.

#### Acceptance Criteria

1. WHEN the TD-EXPORT_CLI starts, THE Config_Loader SHALL prompt for `Endpoint` on standard output and read a line from standard input.
2. IF the `Endpoint` input is empty, THEN THE Config_Loader SHALL log an error describing the missing argument and cause the process to exit with a non-zero status code.
3. WHEN the TD-EXPORT_CLI prompts for `Port`, THE Config_Loader SHALL read a line from standard input and, IF the input is empty, THEN THE Config_Loader SHALL log a warning and default the port to `3306`.
4. IF the `Port` input is non-empty and is not a base-10 integer in the range 1..=65535, THEN THE Config_Loader SHALL log an error identifying the invalid port and exit with a non-zero status code.
5. IF the `User` input is empty, THEN THE Config_Loader SHALL log an error describing the missing argument and exit with a non-zero status code.
6. WHEN the TD-EXPORT_CLI prompts for `Password`, THE Config_Loader SHALL read the input from the controlling terminal without echoing characters to the screen.
7. WHEN the TD-EXPORT_CLI prompts for `DB(Seperator , or Space(All))`, THE Config_Loader SHALL treat an empty input as "all non-system schemas" and a comma-separated input as the explicit target DB list.
8. WHEN the TD-EXPORT_CLI prompts for `Exception Tables(Seperator , or Space(none) / Use wildcard)`, THE Config_Loader SHALL treat an empty input as "no exceptions" and a comma-separated input (with SQL `LIKE` wildcards allowed) as table name patterns to exclude.
9. THE Config_Loader SHALL never write the password value to any log sink, file, or error message.

**Correctness Properties**

- *Password non-leak*: 모든 로그·오류 출력 경로에 대해, `RunConfig.password` 값이 출력 문자열에 등장하지 않는다(fuzz 기반 검증).
- *Default port idempotence*: 빈 포트 입력에 대해 두 번 호출해도 항상 `3306`을 반환한다.
- *Parse totality*: 임의 문자열 입력에 대해 포트 파서는 `Ok(u16)` 또는 `Err(_)`만 반환하며 패닉하지 않는다.

---

### Requirement 3: MySQL 연결 확립 및 상태 검증

**User Story:** 사용자로서, 수집을 시작하기 전에 DB 연결이 정상인지 조기에 확인하여 잘못된 접속 정보로 인한 시간 낭비를 피하고 싶습니다.

#### Acceptance Criteria

1. WHEN a valid RunConfig is provided, THE DB_Client SHALL open a MySQL connection pool using the configured endpoint, port, user, and password, targeting the `information_schema` default database.
2. WHEN the connection pool is established, THE DB_Client SHALL execute `SELECT 1` as a readiness probe before any metadata query.
3. IF the readiness probe fails or the connection cannot be opened within a configurable timeout, THEN THE DB_Client SHALL return an error that wraps the underlying driver error and identifies the endpoint and port (without the password).
4. WHEN the TD-EXPORT_CLI terminates (normally or due to an error), THE DB_Client SHALL release all database connections.
5. THE DB_Client SHALL apply a per-query timeout to all metadata queries to prevent indefinite blocking.

**Correctness Properties**

- *Resource safety*: 임의의 성공/실패 시나리오(mock)에서 `DB_Client` 드롭 후 보유한 연결 수는 0이다.
- *Error wrapping*: 기저 드라이버 오류가 발생했을 때 반환된 오류 체인에는 반드시 엔드포인트와 포트가 포함되며, 비밀번호 문자열은 포함되지 않는다.

---

### Requirement 4: 대상 스키마 목록 수집

**User Story:** 사용자로서, 시스템 스키마는 자동 제외되고 내가 지정한 DB만 내보내지도록 하여 불필요한 메타데이터 수집을 피하고 싶습니다.

#### Acceptance Criteria

1. THE DB_Client SHALL query `information_schema.SCHEMATA` for the list of schema names.
2. THE DB_Client SHALL exclude the System_Schemas (`information_schema`, `mysql`, `sys`, `performance_schema`, `tmp`) from the returned list.
3. WHEN RunConfig.target_db is empty, THE DB_Client SHALL return all non-system schemas.
4. WHEN RunConfig.target_db contains a comma-separated list of schema names, THE DB_Client SHALL restrict the result to schemas whose names appear in the list, using parameterized query bindings (not string concatenation).
5. IF the resulting schema list is empty, THEN THE TD-EXPORT_CLI SHALL log an informational message stating "Not in Schema." and exit with a non-zero status code.
6. THE DB_Client SHALL return schema names as a `Schema_Catalog` (map from schema name to an initially empty `Vec<TableDef>`).

**Correctness Properties**

- *System-schema exclusion invariant*: 임의의 스키마 집합 입력에 대해 반환 결과에는 System_Schemas의 요소가 하나도 포함되지 않는다.
- *Target-DB subset*: `target_db`가 비어있지 않을 때, 반환된 스키마 목록은 `target_db`의 부분집합이다.
- *No SQL injection*: 임의 문자열(특수문자 `,'";--`, 유니코드 포함)로 `target_db`를 구성해도 SQL 에러만 발생할 뿐 서버 측 명령 실행은 일어나지 않는다(파라미터 바인딩 검증).

---

### Requirement 5: 테이블 목록 및 일반 정보 수집

**User Story:** 사용자로서, 각 스키마의 테이블과 뷰를 그 일반 정보(엔진, 행 포맷, 콜레이션, 주석)와 함께 수집하여 정의서에 포함시키고 싶습니다.

#### Acceptance Criteria

1. FOR each schema in the Schema_Catalog, THE DB_Client SHALL query `information_schema.TABLES` with `table_schema` bound to the schema name.
2. THE DB_Client SHALL retrieve the following columns per row: `table_name`, `table_type`, `engine`, `row_format`, `table_collation`, `table_comment`.
3. WHEN RunConfig.except_tables is non-empty, THE DB_Client SHALL add `AND table_name NOT LIKE ?` clauses (one per pattern) using parameterized bindings.
4. THE DB_Client SHALL map MySQL NULL values in `engine`, `row_format`, `table_collation`, and `table_comment` to `Option::None` in the resulting `TableDef.general` fields.
5. THE DB_Client SHALL preserve the original row order returned by MySQL.

**Correctness Properties**

- *NULL mapping*: 임의의 `(NullableString, bool is_null)` 샘플에 대해 `is_null == true ⇔ mapped == None`.
- *Exception filter correctness*: 임의의 테이블명 집합 `T`와 패턴 집합 `P`에 대해, 모의 DB에서 반환된 결과는 `{t ∈ T | ∀ p ∈ P, !like_match(t, p)}`와 동일하다.

---

### Requirement 6: 컬럼/인덱스/제약 조건 수집 (BASE TABLE)

**User Story:** 사용자로서, 일반 테이블의 모든 컬럼 정의, 보조 인덱스, 외래 키 제약 조건을 정렬된 형태로 수집하여 정확한 정의서를 만들고 싶습니다.

#### Acceptance Criteria

1. WHERE `table_type == "BASE TABLE"`, THE DB_Client SHALL query `information_schema.COLUMNS` with bindings `(table_name, table_schema)` ordered by `ordinal_position`.
2. THE DB_Client SHALL populate `ColumnInfo` with `column_name`, `column_default`, `is_nullable`, `column_type`, `character_set_name`, `collation_name`, `column_key`, a concatenation of `extra` and `generation_expression` (space-separated, empty string if generation_expression is NULL), and `column_comment`.
3. WHERE `table_type == "BASE TABLE"`, THE DB_Client SHALL query `information_schema.STATISTICS` grouped by `(table_schema, table_name, index_name)` excluding `INDEX_NAME = 'PRIMARY'`, ordered by `INDEX_NAME`.
4. THE DB_Client SHALL populate `IndexInfo` with `index_name`, `non_unique`, and a comma-joined `GROUP_CONCAT` of `column_name` ordered by `seq_in_index` ascending.
5. WHERE `table_type == "BASE TABLE"`, THE DB_Client SHALL join `information_schema.KEY_COLUMN_USAGE` with `information_schema.REFERENTIAL_CONSTRAINTS` on `constraint_name`, filtered by `table_name` and `constraint_schema`, excluding `constraint_name = 'PRIMARY'`.
6. THE DB_Client SHALL populate `ConstInfo` with `constraint_name`, `GROUP_CONCAT(column_name)`, `CONCAT(referenced_table_name, '.', referenced_column_name)`, `delete_rule`, and `update_rule`.
7. IF a column/index/constraint query fails for one table, THEN THE TD-EXPORT_CLI SHALL log the error with schema and table name and continue processing the remaining tables (does not abort the run).

**Correctness Properties**

- *Ordinal order preservation*: 반환된 `columns` 리스트는 `ordinal_position` 오름차순으로 정렬되어 있다(단조 증가).
- *Index name order*: 반환된 `indexes` 리스트는 `index_name` 사전순으로 정렬되어 있다.
- *Per-table failure isolation*: 임의의 테이블 리스트에 대해 하나의 테이블에서 오류가 발생해도 나머지 테이블의 수집 결과는 영향을 받지 않는다.

---

### Requirement 7: 뷰 정의 수집

**User Story:** 사용자로서, `VIEW` 타입 객체의 원본 `CREATE` 구문과 캐릭터셋/콜레이션을 수집하여 뷰 정의서를 만들 수 있기를 원합니다.

#### Acceptance Criteria

1. WHERE `table_type == "VIEW"`, THE DB_Client SHALL execute `SHOW CREATE TABLE {schema}.{table}` with schema and table names safely quoted with backticks.
2. THE DB_Client SHALL populate `ViewInfo` with the `Create View` text, character set, and collation columns returned by `SHOW CREATE TABLE`.
3. IF the `SHOW CREATE TABLE` query fails for a view, THEN THE TD-EXPORT_CLI SHALL log the error with schema and view name and continue processing the remaining objects.
4. THE DB_Client SHALL not populate ColumnInfo/IndexInfo/ConstInfo for views.

**Correctness Properties**

- *Identifier safety*: 임의의 스키마/테이블 이름 문자열(`` ` ``, `;`, `--` 등 포함)에 대해, 생성되는 SQL은 MySQL 식별자 인용 규칙을 준수하며 주입 공격이 성공하지 않는다.

---

### Requirement 8: DDL 수집 (SQL 포맷 전용)

**User Story:** DB 관리자로서, 테이블의 원본 `CREATE TABLE` DDL을 그대로 수집하여 재현용 SQL 스크립트를 만들 수 있기를 원합니다.

#### Acceptance Criteria

1. WHERE `RunConfig.output == Sql`, FOR each table in the schema, THE DB_Client SHALL execute `SHOW CREATE TABLE {schema}.{table}` and capture the DDL string into `TableDef.ddl`.
2. WHERE `RunConfig.output == Sql`, THE DB_Client SHALL skip column/index/constraint metadata queries for the same table.
3. WHERE `RunConfig.output ∈ {Excel, Markdown}`, THE DB_Client SHALL not populate `TableDef.ddl`.

---

### Requirement 9: Markdown 출력 파일 생성

**User Story:** 개발자로서, 스키마별로 하나의 Markdown 파일을 생성하고 목차·컬럼표·인덱스·제약 조건·뷰 SQL을 포함한 정의서를 받고 싶습니다.

#### Acceptance Criteria

1. FOR each schema in the Schema_Catalog, THE MarkdownExporter SHALL create a file at the current working directory named `{schema}.md`, truncating any pre-existing file with the same name.
2. THE MarkdownExporter SHALL write a title line of `{schema}` followed by a `=============` underline at the top of the file.
3. THE MarkdownExporter SHALL write a `## Table List` section with a bullet list of `- [{table} ({comment})](#{table-lower})` entries in the order tables are returned by the DB_Client.
4. WHERE `table_type == "BASE TABLE"`, THE MarkdownExporter SHALL write a `## {table-lower}` section containing:
   a. A general information table with columns `Table type | Engine | Row format | Collate | Comment`.
   b. A `**Columns**` section with a Markdown table having columns `Name | Type | Nullable | Default | Charset | Collation | Key | Extra | Comment`.
   c. WHERE indexes are non-empty, an `**Index**` section listing `- [Normal]{name}({cols})` when `non_unique == 1` and `- [Unique]{name}({cols})` when `non_unique == 0`.
   d. WHERE constraints are non-empty, a `**Constraint**` section listing `- {name} FOREIGN KEY ({cols}) REFERNCES {refer} ON DELETE {del} ON UPDATE {upd}` per constraint (the misspelling `REFERNCES` MUST match the Go version for output compatibility).
5. WHERE `table_type == "VIEW"`, THE MarkdownExporter SHALL write a general information table with columns `Table type | Charset | Collate` followed by a `**View Create SQL**` section containing the view DDL wrapped in a triple-backtick fenced code block.
6. THE MarkdownExporter SHALL convert MySQL NULL values to empty strings in all written cells.
7. THE MarkdownExporter SHALL flush and close each file after all tables for its schema have been written.

**Correctness Properties**

- *Filename determinism*: 동일한 스키마 이름에 대해 생성되는 파일명은 항상 `{schema}.md`이다.
- *Table-list completeness*: 수집된 테이블 집합 `T`에 대해, `## Table List` 섹션 아래에 정확히 `|T|`개의 불릿 항목이 존재한다.
- *Section count invariant*: 출력된 `## {table}` 섹션의 수는 `|T|`와 동일하다.

---

### Requirement 10: Excel 출력 파일 생성

**User Story:** DB 관리자로서, 스키마마다 별도 시트를 갖고 테이블별로 병합 셀·헤더 스타일이 적용된 하나의 `.xlsx` 파일을 받고 싶습니다.

#### Acceptance Criteria

1. THE ExcelExporter SHALL create a single workbook and add one worksheet per schema named `{schema}`.
2. THE ExcelExporter SHALL delete the default `Sheet1` after adding the schema sheets.
3. FOR each table in a schema, THE ExcelExporter SHALL write the following block in sequence, advancing the row number after each sub-block:
   - A start row styled with the `start` border style.
   - `Table name` label (A:B merged) and table name (C:J merged).
   - `Description` label (A:B merged) and table comment (C:J merged).
   - `Column Information` title (A:J merged).
   - WHERE `table_type == "BASE TABLE"`, a header row with `No | Column | Data Type | Nullable | Key | Extra | Collate | Default | Comment(I:J merged)`, followed by one data row per column with index `i` starting at 0.
   - WHERE indexes exist, an `Indexes` title row followed by a header row (`Index Type | Index Name | Columns` with appropriate merged ranges) and one row per index labelling `Normal Index` when `non_unique == 1` else `Unique Index`.
   - WHERE constraints exist, a `Constraint` title row followed by a header (`Constraint Name | Column | Referance | ON DELETE | ON UPDATE`) and one row per constraint (the misspelling `Referance` MUST match the Go version).
   - WHERE `table_type == "VIEW"`, a `View Create SQL` title row followed by a row containing the view DDL.
   - A `Table Information` title row followed by two rows containing `Engine / Row Format` and `Table Type / Collation`.
   - An end row styled with the `end` border style, followed by one blank row.
4. THE ExcelExporter SHALL define three named styles: `title` (black background, white bold text, all-sides border), `start` (bottom border only), `end` (top border only), and apply them as specified above.
5. THE ExcelExporter SHALL save the workbook to `{endpoint}.xlsx` in the current working directory after all schemas have been written.
6. WHERE `table_type == "VIEW"`, THE ExcelExporter SHALL use `ViewInfo.collate` for the Collation cell; otherwise, THE ExcelExporter SHALL use `general.collate`.

**Correctness Properties**

- *Sheet-count equality*: 생성된 시트 수는 `|schemas|`와 같다(기본 `Sheet1`이 제거된 뒤).
- *Filename determinism*: 결과 파일명은 항상 `{endpoint}.xlsx`이다.
- *Monotonic row advance*: 한 테이블을 쓰는 동안 사용된 RowNum은 시작값보다 반드시 커진 상태로 종료한다.

---

### Requirement 11: SQL 출력 파일 생성

**User Story:** DB 관리자로서, 스키마별로 재생성용 `.sql` 파일을 받아 다른 환경에 스키마를 복제하고 싶습니다.

#### Acceptance Criteria

1. FOR each schema in the Schema_Catalog, THE SqlExporter SHALL create a file at the current working directory named `{schema}({endpoint}).sql`, truncating any pre-existing file with the same name.
2. THE SqlExporter SHALL write a `/* Database : {schema} */` header comment as the first line.
3. FOR each table in the schema, THE SqlExporter SHALL write in order:
   - `/* Table : {table_name} */`
   - `DROP TABLE IF EXISTS {table_name};`
   - `{ddl};` followed by two blank lines.
4. THE SqlExporter SHALL preserve the DDL string exactly as returned by MySQL (no trimming, no rewriting of identifiers).
5. THE SqlExporter SHALL flush and close each file after all tables for its schema have been written.

**Correctness Properties**

- *Filename determinism*: 동일한 스키마 + 엔드포인트 쌍에 대해 파일명은 항상 `{schema}({endpoint}).sql`이다.
- *DDL preservation (round-trip)*: 임의의 `ddl` 문자열 집합에 대해, 쓰여진 파일에서 `DROP TABLE IF EXISTS ...;\n` 접두어와 트레일링 `;\n\n\n`을 제거한 결과는 원본 DDL 문자열의 연결과 일치한다(왕복 성질).
- *Table-block count*: 출력된 `/* Table : */` 주석 수는 처리된 테이블 수와 동일하다.

---

### Requirement 12: 로깅 및 진행 상황 보고

**User Story:** 운영자로서, 각 단계(접속 성공, 스키마 개수, 테이블 로드, 완료)의 진행 상황을 타임스탬프와 함께 콘솔에서 확인하여 긴 실행에서 상태를 파악하고 싶습니다.

#### Acceptance Criteria

1. WHEN the TD-EXPORT_CLI starts, THE Logger SHALL emit an `INFO` message containing the application name and version string.
2. WHEN the DB connection is successfully established, THE Logger SHALL emit an `INFO` message `DB Connect Success`.
3. WHEN the Exporter setup completes, THE Logger SHALL emit an `INFO` message `Setup {Format} Files` where `{Format}` is one of `Excel`, `Markdown`, `SQL`.
4. WHEN schema discovery completes, THE Logger SHALL emit `INFO` messages `Get Schema Count : {n}` and, for each schema, `{schema} Table Load.` and `{schema} Table Count : {n}` and `{schema} Table Column/Index/Const Load`.
5. WHEN the export completes successfully, THE Logger SHALL emit `INFO` message `Export Complete.`.
6. IF any step fails with an unrecoverable error, THEN THE Logger SHALL emit an `ERROR` level message with the error chain and the process SHALL exit with a non-zero status code.
7. IF any step fails with a recoverable error (per-table query failure), THEN THE Logger SHALL emit a `WARN` or `ERROR` message and continue processing.
8. THE Logger SHALL never emit the password value at any log level.

**Correctness Properties**

- *Log-message coverage*: 성공 경로의 필수 메시지(`DB Connect Success`, `Export Complete.` 등)는 임의의 스키마 개수(N ≥ 1)에서 모두 정확히 한 번 출력된다.
- *Password redaction*: 임의의 비밀번호(ASCII 비인쇄 문자 포함)에 대해, 전체 로그 출력에는 해당 문자열이 나타나지 않는다.

---

### Requirement 13: 에러 처리 및 종료 코드 규약

**User Story:** 자동화 스크립트 작성자로서, 실패 원인에 따라 명확한 종료 코드와 에러 메시지를 받아 CI/CD 파이프라인에서 재시도 로직을 구성하고 싶습니다.

#### Acceptance Criteria

1. WHEN the TD-EXPORT_CLI completes the export successfully, THE TD-EXPORT_CLI SHALL exit with status code 0.
2. IF required input (Endpoint, User) is missing, THEN THE TD-EXPORT_CLI SHALL exit with status code 1.
3. IF the DB connection fails, THEN THE TD-EXPORT_CLI SHALL exit with status code 1.
4. IF schema discovery returns zero schemas, THEN THE TD-EXPORT_CLI SHALL exit with status code 1.
5. IF a file write operation fails for the chosen output format, THEN THE TD-EXPORT_CLI SHALL exit with status code 1.
6. WHEN a per-table metadata query fails, THE TD-EXPORT_CLI SHALL log the error and continue (does not alter the final exit code unless a later step fails).
7. THE TD-EXPORT_CLI SHALL wrap lower-level errors using Rust's error source chain (e.g., `thiserror` / `anyhow`) so that the root cause is preserved.

**Correctness Properties**

- *Exit code totality*: 모든 실행 경로(성공/각종 실패)에 대해 종료 코드는 0 또는 양의 정수이며, 정의된 매핑 테이블 외의 값은 나오지 않는다.
- *Error chain preservation*: 임의의 내부 오류에 대해 최종 출력되는 에러 메시지의 `source()` 체인에는 원본 오류 타입이 보존된다.

---

### Requirement 14: SQL 주입 및 식별자 안전성

**User Story:** 보안 담당자로서, 사용자가 제공하는 DB/테이블 이름이나 제외 패턴이 SQL 주입 공격에 악용되지 않도록 하여 DB를 보호하고 싶습니다.

#### Acceptance Criteria

1. THE DB_Client SHALL use parameter binding (`?` placeholders) for all user-supplied values in query predicates (`table_name`, `table_schema`, `LIKE` patterns, IN lists).
2. WHERE a query requires a schema or table identifier in a position that cannot be parameterized (e.g., `SHOW CREATE TABLE {schema}.{table}`), THE DB_Client SHALL reject identifiers containing the characters `` ` ``, `;`, `/*`, `*/`, or newline, OR quote them with backticks and escape any embedded backtick as `` `` ``.
3. IF an identifier fails the safety check, THEN THE DB_Client SHALL return an error without executing the query.
4. THE DB_Client SHALL never construct SQL by concatenating raw password or other secret values.

**Correctness Properties**

- *Injection resistance*: 임의의 악성 입력 문자열 집합(`'; DROP TABLE ...`, backtick injection, 유니코드 호모글리프 등)에 대해 `DB_Client`는 주입에 성공하지 않는다(실행 전 거부 또는 안전하게 인용 처리).
- *Identifier quoting round-trip*: 유효한 식별자 `id`에 대해 `unquote(quote(id)) == id`.

---

### Requirement 15: 비기능적 요구사항 (성능, 빌드, 호환성)

**User Story:** 운영자로서, Rust 버전이 기존 Go 버전과 대등한 속도로 동작하고 주요 플랫폼에서 빌드 가능해야 기존 배포 워크플로에 투입할 수 있습니다.

#### Acceptance Criteria

1. THE TD-EXPORT_CLI SHALL build successfully on stable Rust (MSRV 1.75 이상) for `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc` targets.
2. WHERE a schema has up to 1,000 tables and each table has up to 100 columns, THE TD-EXPORT_CLI SHALL complete the export on a local network within a time budget of 120 seconds (excluding DB server-side time), using connection reuse.
3. THE TD-EXPORT_CLI SHALL reuse a single MySQL connection pool (minimum 1, maximum configurable, default 4) across metadata queries to avoid per-query connection overhead.
4. THE TD-EXPORT_CLI SHALL handle schemas and tables whose names contain Unicode characters (including Korean, Japanese, Chinese) correctly in both queries and output files.
5. THE TD-EXPORT_CLI SHALL produce output files with UTF-8 encoding; Markdown and SQL files SHALL NOT include a BOM.
6. THE TD-EXPORT_CLI SHALL use `cargo fmt` formatting and pass `cargo clippy --all-targets --all-features -- -D warnings` in CI.
7. THE TD-EXPORT_CLI SHALL provide a `README.md` in Korean describing build, usage, and CLI flags, mirroring the content scope of the Go version.

**Correctness Properties**

- *Unicode preservation (round-trip)*: 임의의 유니코드 스키마/테이블 이름에 대해, 수집된 이름을 파일에 기록한 뒤 다시 읽어들이면 원본과 바이트 단위로 일치한다.
- *Connection reuse invariant*: N개 테이블에 대한 메타데이터 조회 동안 생성된 물리 커넥션 수는 설정된 최대 풀 크기를 초과하지 않는다.

---

## Parser/Serializer Round-trip Note

본 스펙은 명시적 parser 구현(예: 구성 파일 파서)을 포함하지 않지만, 다음 변환들은 parser/serializer 쌍으로 간주되어 왕복 속성 테스트를 권장합니다.

- `OutputFormat` ↔ `String` (Requirement 1)
- 식별자 ↔ 백틱 인용 식별자 (Requirement 14)
- DDL 문자열 ↔ SQL 파일 내 DDL 블록 (Requirement 11)

각 쌍에 대해 `parse(format(x)) == x` 또는 의미 동등성(semantic equality) 속성을 property-based test로 검증해야 합니다.
