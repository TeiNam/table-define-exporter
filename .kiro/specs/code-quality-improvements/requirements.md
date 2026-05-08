# Requirements Document

## Introduction

본 스펙은 `td-export` Rust 프로젝트의 코드 리뷰에서 식별된 15개 개선 항목을 체계적으로 반영하기 위한 요구사항을 정의한다. 범위는 보안/정확성 버그 수정(P0), 정확성·품질 개선(P1), 성능 최적화(P2), 부가 개선(P3)으로 구분된다.

개선의 핵심 목적은 다음과 같다.

- 비밀번호·커넥션 문자열의 로그 노출 제거 및 특수문자 포함 자격 증명 지원
- SQL 출력물의 정확성(식별자 인용, 세미콜론, Markdown 코드블록 포맷) 보장
- `src/db/postgres.rs`의 거대 파일(1,216줄)을 coding-style 가이드(≤800줄)에 맞게 분리
- 에러 처리·관측성·성능의 일관성 확보 (`anyhow::Result<()>` 패턴, `EnvFilter`, 병렬 메타데이터 수집)
- 기존 출력 바이트 호환성(Go 버전과 동일) 유지 — 단, 검증된 버그(Markdown VIEW 코드블록)는 정정

본 스펙은 기능 추가가 아닌 **품질 개선·버그 수정**에 집중하며, 모든 변경은 기존 테스트 회귀 방지를 최우선으로 한다.

## Glossary

- **td-export**: 본 프로젝트의 바이너리 크레이트 이름. MySQL/PostgreSQL 테이블 정의서를 Excel/Markdown/SQL로 내보낸다.
- **Postgres_Module**: `src/db/postgres.rs` 단일 파일 → `src/db/postgres/{mod,ddl,parse,types}.rs` 하위 모듈 집합으로 재구성된 결과.
- **Identifier_Module**: `src/identifier.rs`에 정의된 식별자 인용·검증 API (`quote_identifier`, `quote_pg_identifier`, `validate_identifier`).
- **Exporter**: `src/export/` 하위의 `ExcelExporter`, `MarkdownExporter`, `SqlExporter` 구현.
- **ConnectOptions**: `sqlx::MySqlConnectOptions` 및 `sqlx::postgres::PgConnectOptions`. URL 문자열 대신 구조체로 자격 증명을 전달하여 특수문자 이스케이프 문제를 회피한다.
- **CLI_Entry**: `src/main.rs`의 `#[tokio::main] async fn main()` 진입점.
- **Run_Function**: 본 스펙에서 도입할 `async fn run(...) -> anyhow::Result<()>` 함수. `main`은 `ExitCode`만 반환한다.
- **Metadata_Pipeline**: `main.rs`에서 각 테이블의 컬럼/인덱스/제약/DDL을 수집하는 루프.
- **Round_Trip_Property**: 인용/복원 같이 `f⁻¹(f(x)) == x`를 만족해야 하는 속성.

## Requirements

### Requirement 1: 비밀번호·커넥션 문자열 노출 방지 (P0-#2)

**User Story:** 운영자로서, 비밀번호와 전체 커넥션 문자열이 로그·에러 메시지에 절대 노출되지 않아야, 로그를 안심하고 저장·공유할 수 있다.

#### Acceptance Criteria

1. WHEN `td-export`가 MySQL 또는 PostgreSQL에 연결할 때, THE td-export SHALL `sqlx::MySqlConnectOptions` 또는 `sqlx::postgres::PgConnectOptions` 구조체를 사용하여 자격 증명을 전달한다 (URL 문자열 포매팅 금지).
2. IF 비밀번호에 URL 특수문자(`@`, `:`, `/`, `?`, `#`, `%`, 공백)가 포함된 경우, THEN THE td-export SHALL 인코딩/파싱 오류 없이 연결을 성공시킨다.
3. THE td-export SHALL 어떤 로그 레벨(`error`, `warn`, `info`, `debug`, `trace`)에서도 비밀번호 평문과 전체 커넥션 URL을 출력하지 않는다.
4. WHEN DB 연결이 실패할 때, THE td-export SHALL 에러 메시지에서 비밀번호를 마스킹하거나 제거한 형태로만 원인을 제공한다.
5. THE td-export SHALL 비밀번호를 구조체 필드에 보관할 때 `Debug` 출력에서 비밀번호가 그대로 노출되지 않도록 수동 `Debug` 구현 또는 비밀번호 격리 래퍼를 사용한다.

### Requirement 2: SQL Exporter 식별자 인용 및 세미콜론 정정 (P0-#3)

**User Story:** SQL 출력 사용자로서, 예약어/특수문자 테이블명과 PostgreSQL DDL이 포함된 SQL 파일이 문법 오류 없이 실행되어야, 스키마 복제·백업에 바로 사용할 수 있다.

#### Acceptance Criteria

1. WHEN `SqlExporter`가 `DROP TABLE IF EXISTS` 구문을 작성할 때, THE SqlExporter SHALL `Identifier_Module`의 인용 함수를 통해 스키마별 규칙(MySQL=백틱, PostgreSQL=큰따옴표)으로 테이블명을 인용한다.
2. WHEN DDL 원문이 이미 세미콜론(`;`) 또는 `);`로 종결되는 경우, THE SqlExporter SHALL 추가 세미콜론을 덧붙이지 않는다.
3. WHEN DDL 원문이 세미콜론 없이 종결되는 경우, THE SqlExporter SHALL 정확히 하나의 세미콜론으로 종결한다.
4. THE SqlExporter SHALL DB 종류별(`mysql`, `postgres`) 종결 규칙을 `DbType` enum으로 분기하며 하드코딩된 포맷 문자열 반복을 제거한다.
5. IF 테이블명이 `Identifier_Module`의 `validate_identifier`를 통과하지 못하는 경우, THEN THE SqlExporter SHALL 해당 테이블을 건너뛰고 `warn` 로그를 출력한다.

### Requirement 3: Markdown VIEW 코드블록 렌더링 정정 (P0-#4)

**User Story:** Markdown 결과 사용자로서, VIEW의 SQL이 코드블록으로 올바르게 렌더링되어야, GitHub/IDE 뷰어에서 SQL 하이라이트를 받을 수 있다.

#### Acceptance Criteria

1. WHEN `MarkdownExporter`가 VIEW의 `view_query`를 출력할 때, THE MarkdownExporter SHALL 다음 포맷을 사용한다: 빈 줄 다음에 ```` ```sql ```` 라인, VIEW SQL 본문, ```` ``` ```` 라인을 각각 별도 줄로 출력.
2. THE MarkdownExporter SHALL 한 줄 안에 백틱 언어 태그와 SQL 본문을 동시에 배치하지 않는다 (현재 `` ```{}``` `` 패턴 제거).
3. WHEN VIEW SQL 자체가 세 개 이상의 연속 백틱을 포함하는 경우, THE MarkdownExporter SHALL 펜스 길이를 충돌하지 않는 길이(예: 네 개 이상의 백틱)로 확장한다.
4. THE MarkdownExporter SHALL 기존 테이블(BASE TABLE) 출력 바이트를 변경하지 않는다 (Go 버전 호환 유지).

### Requirement 4: 식별자 인용의 방어적 검증 (P1-#5)

**User Story:** 보안 리뷰어로서, 식별자 인용 함수가 항상 위험 문자 검증을 수행해야, 외부 스키마명이 인용을 우회하여 SQL 인젝션을 일으킬 수 없다.

#### Acceptance Criteria

1. WHEN `quote_identifier` 또는 `quote_pg_identifier`가 호출될 때, THE Identifier_Module SHALL 내부에서 `validate_identifier`를 먼저 호출하여 위험 문자(`;`, `/*`, `*/`, 개행, 캐리지 리턴)를 검사한다.
2. IF 입력 식별자가 `validate_identifier`를 통과하지 못하는 경우, THEN THE Identifier_Module SHALL `AppError::UnsafeIdentifier` 에러를 반환한다.
3. FOR ALL 유효 식별자(위험 문자 없음), `quote → unquote`는 원본과 동일한 문자열을 반환한다 (라운드트립 속성).
4. THE Identifier_Module SHALL 기존 `validate_identifier`의 공개 API와 호환성을 유지한다.

### Requirement 5: 메타데이터 컬럼 누락 경고 (P1-#6)

**User Story:** 개발자로서, SQL 결과의 컬럼이 누락됐을 때 조용히 기본값으로 대체되지 않고 경고 로그가 남아야, 스키마 변경/오타를 조기에 감지할 수 있다.

#### Acceptance Criteria

1. THE td-export SHALL `sqlx::Row::try_get()` 실패 시 기본값을 반환하는 공용 헬퍼 함수를 `src/db/` 하위에 제공한다.
2. WHEN 헬퍼 함수가 `try_get` 실패를 감지할 때, THE td-export SHALL `warn` 레벨로 스키마/테이블/컬럼 이름을 포함한 로그를 출력한다.
3. THE td-export SHALL `postgres.rs`와 `mysql.rs`의 기존 `try_get(...).unwrap_or_default()` 호출을 새 헬퍼로 교체하되, 최소 주요 경로 10개소 이상을 마이그레이션한다.
4. THE td-export SHALL 헬퍼 함수가 경고 로그를 반복 출력하지 않도록 동일 (schema, table, column) 조합에 대해 실행당 한 번만 로그한다.

### Requirement 6: CLI 에러 처리 단일화 (P1-#7)

**User Story:** 운영자로서, 모든 CLI 실패가 일관된 형태로 보고되고 프로세스가 올바른 종료 코드로 끝나야, 스크립트·CI에서 실패 여부를 판별할 수 있다.

#### Acceptance Criteria

1. THE CLI_Entry SHALL `Run_Function`(`async fn run(cli: Cli) -> anyhow::Result<()>`)을 호출하여 모든 비즈니스 로직을 위임한다.
2. THE CLI_Entry SHALL `main`에서 `Run_Function`의 결과를 받아 성공 시 `ExitCode::SUCCESS`, 실패 시 `ExitCode::FAILURE`를 반환한다.
3. WHEN `Run_Function`이 에러를 반환할 때, THE CLI_Entry SHALL `tracing::error!`로 에러 체인을 한 번만 출력한다.
4. THE CLI_Entry SHALL 기존 `match { Err(e) => { error! + exit(1) } }` 반복 블록을 모두 제거한다.
5. THE td-export SHALL 기존 종료 코드 의미(실패 시 비영)를 유지한다.

### Requirement 7: 로그 레벨 환경변수 제어 (P1-#8)

**User Story:** 디버거로서, `RUST_LOG` 환경변수로 로그 레벨을 조정할 수 있어야, 재빌드 없이 `debug`/`trace` 로그를 켤 수 있다.

#### Acceptance Criteria

1. THE CLI_Entry SHALL `tracing_subscriber::fmt()` 빌더에 `EnvFilter::from_default_env()`을 연결하여 초기화한다.
2. WHEN `RUST_LOG` 환경변수가 설정되지 않은 경우, THE td-export SHALL 기본 레벨 `info`를 적용한다.
3. WHEN `RUST_LOG=debug td-export ...` 형태로 실행될 때, THE td-export SHALL `debug` 레벨 이상의 로그를 출력한다.
4. THE td-export SHALL `Cargo.toml`의 `tracing-subscriber` features에 이미 포함된 `env-filter`를 활용한다 (신규 의존성 추가 금지).

### Requirement 8: postgres.rs 모듈 분리 (P0-#1)

**User Story:** 유지보수자로서, `postgres.rs`가 coding-style 가이드(≤800줄)를 준수해야, 파일당 책임 경계가 명확하고 수정이 쉬워진다.

#### Acceptance Criteria

1. THE Postgres_Module SHALL `src/db/postgres/`를 루트로 하여 `mod.rs`, `ddl.rs`, `parse.rs`, `types.rs` 파일로 분리된다.
2. THE Postgres_Module SHALL 분리 후 각 파일이 800줄을 초과하지 않는다.
3. THE Postgres_Module SHALL 공개 API(`PostgresClient` 및 `DbClient` trait 구현)의 시그니처를 변경하지 않는다.
4. THE Postgres_Module SHALL 기존 단위 테스트(`tests/db_test.rs` 포함)를 수정 없이 통과시킨다.
5. THE Postgres_Module SHALL `mod.rs`에서 하위 모듈을 `pub(crate) use`로 재노출하여 외부 경로 변경 없이 접근 가능하게 한다.

### Requirement 9: 메타데이터 병렬 수집 (P2-#9)

**User Story:** 대규모 스키마를 다루는 운영자로서, 테이블 메타데이터 조회가 병렬 처리되어야, 대기 시간이 단축된다.

#### Acceptance Criteria

1. THE Metadata_Pipeline SHALL `futures::stream::FuturesUnordered` 또는 `stream::iter(...).buffer_unordered(N)`을 사용하여 테이블 메타데이터를 동시 조회한다.
2. THE Metadata_Pipeline SHALL 동시성 상한(`N`)을 커넥션 풀 크기와 동일하거나 작게 설정한다.
3. WHEN 한 테이블의 메타데이터 조회가 실패할 때, THE Metadata_Pipeline SHALL 해당 테이블의 에러를 로그하고 다른 테이블의 수집을 계속 진행한다.
4. THE Metadata_Pipeline SHALL 병렬 처리 후 테이블 목록의 순서를 결정적(deterministic)으로 유지한다.
5. THE td-export SHALL 병렬화 이후에도 출력 파일의 바이트 단위 동일성을 유지한다 (동일 입력 → 동일 출력).

### Requirement 10: PostgreSQL FK N+1 쿼리 제거 (P2-#10)

**User Story:** 외래키가 많은 PostgreSQL 스키마 사용자로서, 외래키 해석이 한 번의 JOIN 쿼리로 수행되어야, 테이블당 FK 수에 비례한 지연이 사라진다.

#### Acceptance Criteria

1. WHEN `get_table_ddl`이 외래키 참조 컬럼을 해석할 때, THE Postgres_Module SHALL 테이블별 FK 개수만큼 별도 쿼리를 발행하지 않는다.
2. THE Postgres_Module SHALL FK 정보(제약명, 로컬 컬럼, 참조 테이블, 참조 컬럼, ON DELETE/UPDATE 액션)를 하나의 JOIN 쿼리로 수집한다.
3. THE Postgres_Module SHALL 변경 전·후 생성되는 DDL 텍스트의 내용이 의미적으로 동일함을 보장한다.
4. THE Postgres_Module SHALL N+1 제거 후에도 기존 테스트를 통과시킨다.

### Requirement 11: PostgreSQL 컬럼 타입 및 인덱스 정확성 (P3-#11, #12)

**User Story:** PostgreSQL 스키마 문서 사용자로서, 배열 컬럼의 길이와 파셜 인덱스의 `WHERE` 절이 출력에 보존되어야, 정확한 정의서를 얻을 수 있다.

#### Acceptance Criteria

1. WHEN 컬럼의 `udt_name`이 `_varchar` 또는 `_bpchar`이고 `character_maximum_length`가 존재할 때, THE Postgres_Module SHALL 타입 문자열을 `varchar(N)[]` 형식으로 생성한다.
2. WHEN 컬럼의 `udt_name`이 `_numeric`이고 `numeric_precision`, `numeric_scale`이 존재할 때, THE Postgres_Module SHALL 타입 문자열을 `numeric(P,S)[]` 형식으로 생성한다.
3. WHEN `pg_indexes.indexdef`에 `WHERE ...` 절이 존재할 때, THE Postgres_Module SHALL 해당 predicate 문자열을 `IndexInfo`의 신설 필드 `predicate: Option<String>`에 저장한다.
4. THE Metadata_Pipeline SHALL Markdown/Excel 출력에서 `predicate`가 `Some`이면 인덱스 뒤에 `WHERE <predicate>` 형태로 렌더링한다.

### Requirement 12: 설정 빈 Vec 가드 (P3-#13)

**User Story:** CLI 사용자로서, `--target-db ""`나 빈 값 입력이 모든 스키마를 무작위로 필터아웃시키지 않아야, 실수로 빈 출력물을 생성하지 않는다.

#### Acceptance Criteria

1. WHEN `CliOverrides.target_db`가 `Some(v)`이고 `v.is_empty()`일 때, THE config_Module SHALL 이를 `None`(전체 선택)으로 정규화한다.
2. WHEN `CliOverrides.except_tables`가 `Some(v)`이고 `v.is_empty()`일 때, THE config_Module SHALL 이를 `None`으로 정규화한다.
3. WHEN `parse_comma_separated`가 쉼표만 있는 입력(예: `,`, `,,`)을 받을 때, THE config_Module SHALL 빈 원소를 제거하며 모든 원소가 비어 있으면 `None`을 반환한다.
4. THE config_Module SHALL 기존 단위 테스트의 의미적 계약(빈 입력 → None)을 유지한다.

### Requirement 13: CI 파이프라인 (P3-#14)

**User Story:** 팀원으로서, 모든 푸시에 대해 자동으로 포맷·린트·테스트·커버리지·감사가 실행되어야, 회귀를 조기에 발견할 수 있다.

#### Acceptance Criteria

1. THE td-export SHALL `.github/workflows/ci.yml` 파일을 포함한다.
2. THE CI 파이프라인 SHALL 다음 잡을 순차 또는 병렬로 수행한다: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --all-features`, `cargo llvm-cov`, `cargo audit`.
3. WHEN `clippy` 경고가 발생할 때, THE CI 파이프라인 SHALL 실패 상태로 종료한다.
4. WHEN `cargo audit`이 취약점을 보고할 때, THE CI 파이프라인 SHALL 실패 상태로 종료한다.
5. THE CI 파이프라인 SHALL Rust 버전으로 MSRV(1.75)와 `stable` 두 가지를 교차 매트릭스로 테스트한다.

### Requirement 14: OutputFormat / DbType Trait 구현 (P3-#15)

**User Story:** 라이브러리 사용자로서, `OutputFormat`과 `DbType`이 표준 `FromStr` 트레이트를 구현해야, `str::parse()`와 clap `ValueEnum` 통합이 자연스럽다.

#### Acceptance Criteria

1. THE model_Module SHALL `OutputFormat` 및 `DbType`에 대해 `std::str::FromStr` 트레이트를 구현한다.
2. THE model_Module SHALL 기존 연관 함수 형태의 `from_str`을 제거하거나 `FromStr::from_str`로 위임한다.
3. THE CLI_Entry SHALL `#[allow(clippy::should_implement_trait)]` 억제 속성을 제거한다.
4. THE model_Module SHALL `clap::ValueEnum`을 추가 구현하거나 `FromStr` 기반 파서를 `#[arg(value_parser = ...)]`로 연결한다 (선택지는 설계 단계에서 결정).
5. WHEN 잘못된 문자열을 파싱할 때, THE model_Module SHALL `AppError::InvalidOutputFormat` 또는 `AppError::InvalidDbType`을 반환한다.

### Requirement 15: 기존 출력 바이트 호환성 및 테스트 회귀 방지

**User Story:** 기존 사용자로서, 이번 개선 이후에도 내 현재 출력물과 바이트 단위로 동일한 결과를 얻을 수 있어야, 기존 문서·스크립트가 그대로 동작한다.

#### Acceptance Criteria

1. THE td-export SHALL 기존 의도된 오타(`Referance`, `REFERNCES`)를 제거하지 않는다 (Go 버전 호환 유지).
2. THE td-export SHALL Requirement 3의 Markdown VIEW 코드블록 수정 외에는 출력물의 바이트 시퀀스를 변경하지 않는다.
3. WHEN 본 스펙의 모든 Task가 완료된 후 `cargo test --all-features`가 실행될 때, THE td-export SHALL 기존 `tests/` 디렉토리의 모든 테스트를 통과시킨다.
4. THE td-export SHALL 신규 회귀 방지 테스트로 (a) 커넥션 URL 노출 여부, (b) Markdown VIEW 코드블록 포맷, (c) 식별자 인용의 `validate` 호출 흐름 각각에 대한 단위 테스트 또는 속성 테스트를 추가한다.
5. THE td-export SHALL `cargo clippy -- -D warnings`를 경고 없이 통과한다.

### Requirement 16: 비기능 요구사항 (성능·보안·가이드 준수)

**User Story:** 프로젝트 오너로서, 개선 이후에도 `.kiro/steering/` 규칙이 모두 충족되어야, 코드베이스의 품질 기준이 유지된다.

#### Acceptance Criteria

1. THE td-export SHALL 모든 Rust 소스 파일을 `cargo fmt --check`로 통과시킨다.
2. THE td-export SHALL `src/` 하위 각 파일을 800줄 이하로 유지한다 (coding-style 가이드 준수).
3. THE td-export SHALL 프로덕션 코드에서 `unwrap()`/`expect()` 사용을 추가하지 않는다 (테스트 코드 예외).
4. THE td-export SHALL 새로 추가되는 `unsafe` 블록을 포함하지 않는다.
5. THE td-export SHALL 비즈니스 로직의 테스트 커버리지를 70% 이상으로 유지한다 (`cargo llvm-cov` 기준).
6. THE td-export SHALL 모든 신규·수정 의존성을 정확 버전(또는 lockfile)에 고정한다 (`dependencies.md` 준수).
7. THE td-export SHALL 관측성 규칙에 따라 구조화된 `tracing` 호출만 사용하며 `println!`/`eprintln!`을 프로덕션 경로에 도입하지 않는다 (CLI stdout 프롬프트는 예외).
