# Implementation Plan: Code Quality Improvements

## Overview

본 계획은 td-export 프로젝트의 15개 코드 품질 개선을 **TDD(테스트 선행) + 의존성 순서 + 작은 커밋**으로 구현한다. 설계 문서의 DAG에 따라 기초 타입(`Password`) → ConnectOptions → 모듈 분할 → 호출부 재배선 → CLI 구조 → 병렬화 → 부가 개선 → CI 순으로 진행한다.

각 Task는 단일 논리 변경 + 커밋 가능 단위 + 회귀 테스트 동반을 원칙으로 한다. `*`로 표시된 서브태스크는 테스트 관련 선택 사항이다.

Convert the feature design into a series of prompts for a code-generation LLM that will implement each step with incremental progress. Make sure that each prompt builds on the previous prompts, and ends with wiring things together. There should be no hanging or orphaned code that isn't integrated into a previous step. Focus ONLY on tasks that involve writing, modifying, or testing code.

## Tasks

- [x] 1. 기초 유틸리티 및 타입 확장 (의존성 없는 항목)
  - [x] 1.1 `identifier::quote_*` 내부에 `validate_identifier` 호출 추가
    - `quote_identifier`, `quote_pg_identifier` 시작부에 `validate_identifier(id)?` 추가
    - 위험 문자 포함 시 `AppError::UnsafeIdentifier` 반환
    - _Requirements: 4.1, 4.2, 4.4_
  - [x] 1.2 위험 식별자 거부 속성 테스트
    - **Property 4: 위험 식별자 거부**
    - **Validates: Requirements 4.1, 4.2**
    - `tests/identifier_test.rs`에 proptest로 위험 문자 삽입 생성기 추가
  - [x] 1.3 식별자 라운드트립 속성 테스트 (기존 확장)
    - **Property 3: 식별자 인용 라운드트립**
    - **Validates: Requirements 2.1, 4.3**
    - MySQL/PG 두 함수 모두 `quote → unquote = original` 보증

- [x] 2. `OutputFormat` / `DbType` FromStr 구현 (Req 14)
  - [x] 2.1 `model::OutputFormat`에 `std::str::FromStr` 구현
    - 기존 `OutputFormat::from_str` 연관 함수를 `FromStr::from_str`로 위임
    - `#[allow(clippy::should_implement_trait)]` 속성 제거
    - _Requirements: 14.1, 14.2, 14.3_
  - [x] 2.2 `model::DbType`에 `FromStr` 구현 (동일 패턴)
    - _Requirements: 14.1, 14.5_
  - [x] 2.3 `clap::ValueEnum` 구현 또는 `value_parser` 어태치
    - `#[value(name = "mysql")]` 등으로 기존 이름 유지
    - `src/main.rs::Cli`의 `Option<String>` → `Option<OutputFormat>` 타입 강화 가능 (선택)
    - _Requirements: 14.4_
  - [x] 2.4 FromStr 동등성 속성 테스트
    - **Property 13: FromStr 동등성**
    - **Validates: Requirements 14.1, 14.5**
    - `tests/model_test.rs` 확장

- [x] 3. `config.rs` 빈 Vec 정규화 (Req 12)
  - [x] 3.1 `CliOverrides::target_db`와 `except_tables` 빈 Vec을 `None`으로 정규화
    - `src/config.rs::load_config`의 분기를 `.filter(|v| !v.is_empty())`로 조정
    - _Requirements: 12.1, 12.2_
  - [x] 3.2 `parse_comma_separated`에서 모든 원소가 공백/빈 문자열이면 `None` 반환
    - 쉼표만 있는 케이스(`","`, `",,,"`)를 명시적으로 커버
    - _Requirements: 12.3_
  - [x] 3.3 빈 Vec 정규화 속성 테스트
    - **Property 12: 빈 Vec 정규화**
    - **Validates: Requirements 12.1, 12.2, 12.3**
    - `tests/config_test.rs` 확장

- [x] 4. 체크포인트 — 기초 개선 검증
  - 전체 `cargo test` 통과 확인
  - `cargo clippy -- -D warnings` 통과 확인
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. `secret::Password` 래퍼 도입 (Req 1.3~1.5의 토대)
  - [x] 5.1 `src/secret.rs` 신규 작성
    - `pub struct Password(String)` + `new`, `expose` + 수동 `Debug`, `Display`, `Clone`
    - `src/lib.rs`에 `pub mod secret;` 등록
    - _Requirements: 1.3, 1.4, 1.5_
  - [x] 5.2 `RunConfig.password`를 `Password` 타입으로 변경
    - `src/model.rs` 수정
    - 수동 `Debug`에서 password 라인을 `&self.password`로 교체 (Password 자체 마스킹)
    - `src/config.rs::load_config`에서 `Password::new(password)`로 생성
    - 호출부 `config.password` → `config.password.expose()` 전수 치환
    - _Requirements: 1.3, 1.4, 1.5_
  - [x] 5.3 Password 마스킹 속성 테스트
    - **Property 1: Password 문자열 표현 마스킹**
    - **Validates: Requirements 1.3, 1.4, 1.5**
    - `tests/secret_test.rs` 신규 — proptest로 임의 문자열 → Debug/Display에 원문 미포함 확인

- [x] 6. `db::connect` 모듈 도입 (Req 1.1, 1.2)
  - [x] 6.1 `src/db/connect.rs` 신규 작성
    - `pub fn mysql_options(config: &RunConfig) -> MySqlConnectOptions`
    - `pub fn pg_options(config: &RunConfig) -> PgConnectOptions`
    - URL 포매팅 사용 금지
    - _Requirements: 1.1, 1.2_
  - [x] 6.2 `MySqlClient::connect`에서 URL을 `connect_with(mysql_options(config))`로 교체
    - 기존 URL 포매팅 라인 제거
    - 에러 매핑 유지
    - _Requirements: 1.1_
  - [x] 6.3 `PgClient::connect`에서 URL을 `connect_with(pg_options(config))`로 교체
    - postgres 분할 전 단계이므로 현재 단일 파일에 적용
    - _Requirements: 1.1_
  - [x] 6.4 ConnectOptions total-ness 속성 테스트
    - **Property 2: ConnectOptions 빌드 총체성**
    - **Validates: Requirements 1.2**
    - `tests/connect_options_test.rs` 신규 — proptest로 임의 비밀번호 → 패닉 없이 값 반환
    - URL literal 부재 확인 테스트 추가
  - [x] 6.5 특수문자 비밀번호 스모크 테스트
    - `%`, `@`, `:` 포함 비밀번호 예제로 ConnectOptions 빌드 성공 확인

- [x] 7. `postgres.rs` 모듈 분할 (Req 8)
  - [x] 7.1 디렉토리 구조 준비
    - `src/db/postgres.rs` 삭제 + `src/db/postgres/mod.rs` 생성 (본체 이동)
    - 공개 경로가 그대로 유지됨을 컴파일로 확인
    - _Requirements: 8.1, 8.3_
  - [x] 7.2 `src/db/postgres/types.rs` 추출
    - `PgDdlColumn`, `PgDdlConstraint`, `PgConstraintType`, `build_pg_column_type`, `determine_pg_extra` 이동
    - `mod.rs`에 `pub(crate) use types::*;` 재노출
    - _Requirements: 8.1, 8.2, 8.5_
  - [x] 7.3 `src/db/postgres/parse.rs` 추출
    - `parse_pg_indexdef`, `extract_columns_from_indexdef`, `split_top_level_commas`, `clean_index_column`, `parse_fk_actions_from_condef`, `extract_check_expression`, `quote_column_list` 이동
    - _Requirements: 8.1, 8.2, 8.5_
  - [x] 7.4 `src/db/postgres/ddl.rs` 추출
    - `build_pg_ddl_from_metadata`와 `get_table_ddl` 본문 이동 (PgClient 메서드 시그니처는 mod.rs에 유지)
    - _Requirements: 8.1, 8.2, 8.5_
  - [x] 7.5 각 파일 800줄 이하 확인 + 기존 테스트 통과
    - `wc -l src/db/postgres/*.rs`로 검증
    - _Requirements: 8.2, 8.4_
  - [x] 7.6 공개 API 불변 회귀 테스트
    - `tests/db_test.rs`에서 `td_export::db::postgres::PgClient`, `build_pg_column_type` 등 기존 경로 유지 확인

- [x] 8. 체크포인트 — 분할 후 회귀 확인
  - 전체 `cargo test` 통과
  - `cargo clippy -- -D warnings` 통과
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. PG 컬럼 타입 정확성 개선 (Req 11.1, 11.2)
  - [x] 9.1 `build_pg_column_type`에서 `_varchar`/`_bpchar` 배열 타입의 길이 반영
    - `character_maximum_length: Some(N)`이면 `format!("varchar({N})[]")`
    - _Requirements: 11.1_
  - [x] 9.2 `_numeric` 배열의 precision/scale 반영
    - `format!("numeric({P},{S})[]")`
    - _Requirements: 11.2_
  - [x] 9.3 배열 타입 포맷 속성 테스트
    - **Property 9: PG 배열 타입 파라미터 포맷**
    - **Validates: Requirements 11.1, 11.2**
    - `tests/pg_types_test.rs` 신규

- [x] 10. PG 파셜 인덱스 predicate 보존 (Req 11.3, 11.4)
  - [x] 10.1 `IndexInfo`에 `predicate: Option<String>` 필드 추가
    - `src/model.rs` + 기본값 `None`
    - 기존 리터럴에 `predicate: None` 기계적 추가
    - _Requirements: 11.3_
  - [x] 10.2 `parse_pg_indexdef`가 `WHERE ...` 절을 추출하여 반환값에 포함
    - 기존 튜플을 `struct ParsedIndex { is_unique, columns, predicate }`로 확장
    - `get_indexes` 호출부에서 predicate를 `IndexInfo`에 주입
    - _Requirements: 11.3_
  - [x] 10.3 predicate 파싱 속성 테스트
    - **Property 10: 파셜 인덱스 predicate 파싱**
    - **Validates: Requirements 11.3**
    - `tests/pg_indexdef_test.rs` 신규
  - [x] 10.4 Markdown/Excel에서 predicate 렌더링
    - 인덱스 라인 뒤에 `" WHERE <predicate>"` 조건부 추가
    - _Requirements: 11.4_
  - [x] 10.5 predicate 렌더링 속성 테스트
    - **Property 11: predicate 렌더링 조건부 포함**
    - **Validates: Requirements 11.4**
    - `tests/export_predicate_test.rs` 신규

- [x] 11. PG FK N+1 쿼리 제거 (Req 10)
  - [x] 11.1 `src/db/postgres/ddl.rs`에 FK JOIN 쿼리 작성
    - `pg_constraint` + `pg_attribute` + `pg_class` JOIN으로 FK 정보 일괄 수집
    - `resolve_fk_ref_columns` 루프 제거
    - _Requirements: 10.1, 10.2_
  - [x] 11.2 기존 테스트 통과 확인 + DDL 문자열 의미 동등성 검증
    - _Requirements: 10.3, 10.4_
  - [x] 11.3 FK 일괄 수집 예제 테스트
    - 샘플 스키마(2 FK) → DDL 출력 고정 기대값 비교

- [x] 12. `try_get_or_warn` 헬퍼 도입 및 마이그레이션 (Req 5)
  - [x] 12.1 `src/db/row_helpers.rs` 신규 작성
    - `pub fn try_get_or_warn<R, T>(row: &R, column: &str, schema: &str, table: &str) -> T`
    - `OnceLock<Mutex<HashSet<String>>>` dedup
    - 실패 시 `tracing::warn!` + 기본값 반환
    - _Requirements: 5.1, 5.2, 5.4_
  - [x] 12.2 `src/db/mod.rs`에 `mod row_helpers;` + 재노출
    - _Requirements: 5.1_
  - [x] 12.3 `src/db/mysql.rs`의 `try_get(...).unwrap_or_default()` 주요 경로 교체
    - `get_columns`, `get_indexes`, `get_constraints`에서 최소 5개소
    - _Requirements: 5.3_
  - [x] 12.4 `src/db/postgres/mod.rs` 및 서브모듈의 동일 패턴 교체
    - 최소 5개소 → 합계 10개소 이상
    - _Requirements: 5.3_
  - [x] 12.5 로그 dedup 속성 테스트
    - **Property 8: try_get_or_warn 로그 dedup**
    - **Validates: Requirements 5.4**
    - `tests/row_helpers_test.rs` 신규 — tracing subscriber로 이벤트 캡처

- [x] 13. 체크포인트 — DB 레이어 개선 검증
  - 전체 테스트 통과
  - `cargo fmt --check` 및 `cargo clippy -- -D warnings` 통과
  - Ensure all tests pass, ask the user if questions arise.

- [x] 14. SQL Exporter 개선 (Req 2)
  - [x] 14.1 `src/export/sql.rs`에 `Terminator` enum 도입
    - `Terminator::Mysql` / `Terminator::Postgres` + `apply(ddl) -> String`
    - 선택 로직은 `RunConfig.db_type`에서 도출
    - _Requirements: 2.2, 2.3, 2.4_
  - [x] 14.2 `DROP TABLE IF EXISTS {t.table_name};`를 식별자 인용 함수로 교체
    - MySQL: `quote_identifier`, PG: `quote_pg_identifier`
    - _Requirements: 2.1_
  - [x] 14.3 위험 식별자 테이블 스킵 + warn
    - 인용 함수가 `Err` 반환 시 `tracing::warn!` + 해당 테이블 건너뛰기
    - _Requirements: 2.5_
  - [x] 14.4 SqlExporter에 `db_type` 전달 경로 정비
    - `setup`에서 `config.db_type`을 필드에 저장
  - [x] 14.5 Terminator 속성 테스트
    - **Property 5: Terminator 단일 세미콜론 종결**
    - **Validates: Requirements 2.2, 2.3**
    - `tests/sql_terminator_test.rs` 신규

- [x] 15. Markdown VIEW 코드블록 수정 (Req 3)
  - [x] 15.1 `src/export/markdown.rs`의 VIEW 렌더 로직 교체
    - 기존 한 줄 `` ```{}``` `` 패턴 제거
    - 새 포맷: 빈 줄, 열기 펜스 라인, SQL 본문, 닫기 펜스 라인을 각각 별도 줄로
    - _Requirements: 3.1, 3.2_
  - [x] 15.2 SQL 본문이 연속 백틱을 포함하는 경우 펜스 길이 확장
    - 본문 내 최장 연속 백틱 길이 `m` → 펜스 길이 `max(3, m+1)`
    - _Requirements: 3.3_
  - [x] 15.3 BASE TABLE 출력 바이트 불변 확인
    - 기존 스냅샷 또는 고정 기대값과 비교
    - _Requirements: 3.4, 15.2_
  - [x] 15.4 VIEW 펜스 속성 테스트
    - **Property 6: Markdown VIEW fenced code block**
    - **Validates: Requirements 3.1, 3.2, 3.3**
    - `tests/markdown_view_test.rs` 신규

- [x] 16. CLI 에러 처리 단일화 (Req 6, 7)
  - [x] 16.1 `src/run.rs` 신규 작성
    - `pub async fn run() -> anyhow::Result<()>`
    - 기존 `main.rs`의 비즈니스 로직을 `?` 기반으로 이동
    - `anyhow::Context`로 에러 컨텍스트 부가
    - _Requirements: 6.1_
  - [x] 16.2 `src/main.rs` 축소
    - `async fn main() -> ExitCode`
    - `tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))).init()`
    - `run::run().await` 결과로 `ExitCode::SUCCESS`/`FAILURE` 반환 + 에러 시 단일 `tracing::error!`
    - 기존 match-exit 반복 블록 모두 제거
    - _Requirements: 6.2, 6.3, 6.4, 7.1, 7.2, 7.3_
  - [x] 16.3 `Cargo.toml`의 `anyhow` 및 `env-filter` feature 활용 확인
    - _Requirements: 7.4_
  - [x] 16.4 error 로그 단일성 속성 테스트
    - **Property 14: run 에러 로그 단일성**
    - **Validates: Requirements 6.3**
    - `tests/run_error_test.rs` 신규 — tracing subscriber로 이벤트 카운트

- [x] 17. 체크포인트 — CLI 구조 및 로그 검증
  - `RUST_LOG=debug cargo run --` 동작 확인
  - 전체 테스트 통과
  - Ensure all tests pass, ask the user if questions arise.

- [x] 18. 메타데이터 병렬 수집 (Req 9)
  - [x] 18.1 `src/concurrency.rs` 신규 작성
    - `pub async fn buffer_metadata<T, F, Fut>(items: Vec<T>, concurrency: usize, f: F) -> Vec<T>`
    - `futures::stream::iter + map + buffered(concurrency)` 기반 (순서 보존)
    - `futures` 크레이트 Cargo.toml에 추가 (버전 `0.3`, 정확 핀)
    - _Requirements: 9.1, 9.2, 9.4_
  - [x] 18.2 `run.rs`의 테이블 메타데이터 루프를 `buffer_metadata`로 교체
    - `concurrency = 4` (풀 크기와 정합)
    - 개별 테이블 실패는 `warn` 로그 + 해당 테이블 스킵
    - _Requirements: 9.1, 9.2, 9.3_
  - [x] 18.3 직렬 → 병렬 전환 후 출력 바이트 동일성 확인
    - 기존 스냅샷 또는 고정 샘플로 비교
    - _Requirements: 9.5, 15.2_
  - [x] 18.4 순서 보존 속성 테스트
    - **Property 7: 병렬 메타데이터 수집 순서 보존**
    - **Validates: Requirements 9.4, 9.5**
    - `tests/concurrency_test.rs` 신규

- [x] 19. 체크포인트 — 성능 개선 검증
  - 전체 테스트 통과
  - 간단한 벤치마크(대규모 스키마 시뮬레이션)가 있다면 개선 확인 (선택)
  - Ensure all tests pass, ask the user if questions arise.

- [x] 20. CI 파이프라인 추가 (Req 13)
  - [x] 20.1 `.github/workflows/ci.yml` 신규 작성
    - `check` 잡: matrix(rust=[1.75, stable]) × fmt/clippy/test/llvm-cov
    - `audit` 잡: rustsec/audit-check action
    - _Requirements: 13.1, 13.2, 13.3, 13.5_
  - [x] 20.2 `cargo-llvm-cov`의 `--fail-under-lines 70` 게이트 설정
    - _Requirements: 16.5_
  - [x] 20.3 `audit` 잡 실패 시 워크플로 실패
    - _Requirements: 13.4_
  - [x] 20.4 로컬에서 CI 스텝 시뮬레이션
    - `act` 또는 수동 스크립트 (선택)

- [x] 21. 최종 회귀 방지 테스트 묶음 (Req 15.4)
  - [x] 21.1 URL literal 부재 검증 테스트
    - `src/db/` 하위 파일에 `mysql://`, `postgres://`, `postgresql://` 문자열 부재 확인
    - `tests/connect_options_test.rs`에 grep 스타일 테스트 추가
    - _Requirements: 1.1, 15.4_
  - [x] 21.2 VIEW 포맷 회귀 예제 테스트
    - 고정 샘플 VIEW SQL → MarkdownExporter 출력이 언어 태그 fenced 패턴을 포함
    - _Requirements: 3, 15.4_
  - [x] 21.3 quote_* validate 호출 회귀 예제 테스트
    - `;` 포함 문자열을 quote 함수에 전달 → `AppError::UnsafeIdentifier`
    - _Requirements: 4, 15.4_

- [x] 22. 최종 체크포인트 — 전체 통합 검증
  - `cargo fmt --check` 통과
  - `cargo clippy --all-features -- -D warnings` 통과
  - `cargo test --all-features` 전체 통과 (기존 + 신규 proptest 100+ iters)
  - `cargo llvm-cov --all-features` 커버리지 ≥ 70%
  - `cargo audit` 취약점 없음
  - 각 `src/**/*.rs` 파일 800줄 이하 (`wc -l`)
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- `*`로 표시된 sub-task는 optional이며 MVP 가속용으로 스킵 가능. 단, 본 스펙의 핵심 가치인 속성 테스트는 가급적 모두 수행 권장.
- 각 체크포인트(4/8/13/17/19/22)는 커밋 경계로 사용. 실패 시 앞 단계로 되돌아간다.
- `postgres.rs` 분할(Task 7)은 가장 큰 diff가 발생하므로 단독 PR로 격리.
- `run.rs` 추출(Task 16)은 비즈니스 로직의 중심이므로 수동 테스트로 end-to-end 동작 확인 권장.
- 모든 Task는 기존 `tests/` 테스트 회귀 없음을 전제 — 실패 시 즉시 rollback 후 근본 원인 분석.
- 구현 진행 시 각 Task를 시작하기 전에 Task ID를 명시하고, 완료 시 checkbox를 체크한다.
