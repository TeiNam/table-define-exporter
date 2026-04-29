# 구현 계획: td-export-rust

## 개요

기존 Go 기반 TD-EXPORT 도구를 Rust로 재구현합니다. MySQL `information_schema`에서 테이블/뷰 메타데이터를 수집하여 Excel(.xlsx), Markdown(.md), SQL(.sql) 포맷으로 내보내는 CLI 유틸리티입니다. 모듈 분리와 트레이트 기반 추상화를 통해 테스트 가능한 구조로 구성합니다.

## Tasks

- [x] 1. 프로젝트 초기화 및 기본 구조 설정
  - [x] 1.1 Cargo 프로젝트 생성 및 Cargo.toml 설정
    - `td-export-rust/` 디렉터리에 Cargo 프로젝트 생성
    - 의존성 추가: clap (v4, derive), sqlx (mysql, runtime-tokio), tokio (rt-multi-thread, macros), rust_xlsxwriter, tracing, tracing-subscriber, thiserror, anyhow, rpassword
    - dev-dependencies: proptest, tempfile
    - edition = "2021", MSRV 1.75 이상
    - _Requirements: 15.1, 15.6_

  - [x] 1.2 디렉터리 구조 및 모듈 스켈레톤 생성
    - `src/main.rs`, `src/config.rs`, `src/db.rs`, `src/model.rs`, `src/error.rs`, `src/identifier.rs` 생성
    - `src/export/mod.rs`, `src/export/excel.rs`, `src/export/markdown.rs`, `src/export/sql.rs` 생성
    - `tests/` 디렉터리 생성 (config_test.rs, model_test.rs, identifier_test.rs, export_test.rs)
    - 각 모듈에 빈 구조체/함수 선언으로 컴파일 가능한 상태 유지
    - _Requirements: 15.1_

- [x] 2. 에러 타입 및 데이터 모델 정의
  - [x] 2.1 에러 타입 정의 (`error.rs`)
    - `AppError` 열거형 정의 (thiserror 기반)
    - 변형: InvalidOutputFormat, MissingInput, InvalidPort, DbConnection, MetadataQuery, NoSchemas, UnsafeIdentifier, FileWrite, ExcelWrite, InputRead
    - `DbConnection`에 endpoint/port 포함, password 미포함
    - _Requirements: 13.7, 3.3, 2.9_

  - [x] 2.2 데이터 모델 정의 (`model.rs`)
    - `RunConfig` 구조체 (Debug 구현에서 password를 `[REDACTED]`로 대체)
    - `OutputFormat` 열거형 + `from_str()`, `as_str()`, `display_name()` 메서드
    - `SchemaCatalog` 타입 별칭 (`HashMap<String, Vec<TableDef>>`)
    - `TableDef`, `GeneralInfo`, `ColumnInfo`, `IndexInfo`, `ConstInfo`, `ViewInfo` 구조체
    - MySQL NULL 값은 `Option<String>`으로 매핑
    - _Requirements: 1.3, 1.4, 5.4, 6.1, 6.2, 6.4, 6.6, 7.2_

  - [x] 2.3 Property 테스트: OutputFormat 왕복 및 전체성
    - **Property 1: OutputFormat 왕복 및 전체성 (Round-trip & Totality)**
    - 모든 OutputFormat 변형에 대해 `from_str(as_str(fmt)) == fmt` 검증
    - 임의 문자열에 대해 `from_str()`이 패닉 없이 Ok 또는 Err 반환 검증
    - 유효 포맷의 임의 대소문자 조합에 대해 올바른 변형 반환 검증
    - **Validates: Requirements 1.3, 1.4**

  - [x] 2.4 Property 테스트: 비밀번호 비노출
    - **Property 4: 비밀번호 비노출 (Password Non-Leak)**
    - `AppError::DbConnection`의 Display 출력에 비밀번호 미포함 검증
    - `RunConfig`의 Debug 출력에 비밀번호 미포함 검증
    - **Validates: Requirements 2.9, 3.3, 12.8**

- [x] 3. 식별자 유틸리티 구현
  - [x] 3.1 식별자 인용/검증 함수 구현 (`identifier.rs`)
    - `quote_identifier()`: 백틱 인용, 내부 백틱 이중 이스케이프
    - `unquote_identifier()`: 인용된 식별자에서 원본 복원
    - `validate_identifier()`: 위험 문자(`;`, `/*`, `*/`, 개행) 검사
    - _Requirements: 14.2, 14.3, 7.1_

  - [x] 3.2 Property 테스트: 식별자 인용 왕복
    - **Property 15: 식별자 인용 왕복 (Identifier Quoting Round-Trip)**
    - 유효한 식별자에 대해 `unquote(quote(id)) == id` 검증
    - 위험 문자 포함 문자열에 대해 안전 이스케이프 또는 에러 반환 검증
    - **Validates: Requirements 14.2**

- [x] 4. Checkpoint - 기본 구조 검증
  - `cargo build` 성공 확인
  - 기존 테스트 모두 통과 확인
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Config Loader 구현
  - [x] 5.1 대화식 입력 및 RunConfig 생성 (`config.rs`)
    - `load_config(output_format: OutputFormat) -> Result<RunConfig, AppError>` 구현
    - Endpoint: 빈 입력 시 에러
    - Port: 빈 입력 시 기본값 3306, 범위 1..=65535 검증
    - User: 빈 입력 시 에러
    - Password: `rpassword::read_password()` 사용 (에코 없음)
    - DB: 빈 입력 → None (전체 스키마), 쉼표 구분 → Vec<String>
    - Exception Tables: 빈 입력 → None, 쉼표 구분 → Vec<String>
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9_

  - [x] 5.2 Property 테스트: 포트 파싱 전체성
    - **Property 2: 포트 파싱 전체성 (Port Parse Totality)**
    - 임의 문자열에 대해 포트 파서가 패닉 없이 Ok(u16) 또는 Err 반환 검증
    - 빈 문자열에 대해 기본값 3306 반환 검증
    - **Validates: Requirements 2.3, 2.4**

  - [x] 5.3 Property 테스트: 쉼표 구분 입력 파싱
    - **Property 3: 쉼표 구분 입력 파싱 (Comma-Separated Input Parsing)**
    - 빈 입력 → None, 비어있지 않은 입력 → 쉼표 분리 Vec 검증
    - 분리된 각 요소가 원본 입력의 쉼표 사이 부분 문자열과 일치 검증
    - **Validates: Requirements 2.7, 2.8**

- [x] 6. DB Client 구현
  - [x] 6.1 MySQL 연결 및 기본 조회 (`db.rs`)
    - `DbClient` 구조체 (sqlx::MySqlPool 래핑)
    - `connect()`: 커넥션 풀 생성 + `SELECT 1` 검증
    - `get_schemas()`: 시스템 스키마 제외, target_db 필터링 (파라미터 바인딩)
    - `get_tables()`: 테이블 목록 + 일반 정보 조회, except_tables LIKE 패턴 적용
    - _Requirements: 3.1, 3.2, 3.5, 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 5.1, 5.2, 5.3, 5.4, 5.5_

  - [x] 6.2 메타데이터 상세 조회 (`db.rs`)
    - `get_columns()`: information_schema.COLUMNS 조회, ordinal_position 정렬
    - `get_indexes()`: information_schema.STATISTICS 조회, GROUP_CONCAT, PRIMARY 제외
    - `get_constraints()`: KEY_COLUMN_USAGE + REFERENTIAL_CONSTRAINTS 조인
    - `get_view_info()`: SHOW CREATE TABLE (백틱 인용 식별자 사용)
    - `get_table_ddl()`: SHOW CREATE TABLE (SQL 포맷 전용)
    - 개별 테이블 실패 시 로그 + continue (격리)
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 7.1, 7.2, 7.3, 7.4, 8.1, 8.2, 8.3, 14.1, 14.2_

  - [x] 6.3 Property 테스트: 스키마 필터링 정확성
    - **Property 5: 스키마 필터링 정확성 (Schema Filtering Correctness)**
    - 시스템 스키마가 결과에 포함되지 않음 검증
    - target_db 지정 시 반환 스키마가 target_db의 부분집합임 검증
    - **Validates: Requirements 4.2, 4.4**

  - [x] 6.4 Property 테스트: 컬럼 순서 보존
    - **Property 7: 컬럼 순서 보존 (Column Ordinal Order Preservation)**
    - 반환된 Vec<ColumnInfo>가 ordinal_position 오름차순 정렬 검증
    - **Validates: Requirements 6.1**

  - [x] 6.5 Property 테스트: 테이블별 실패 격리
    - **Property 8: 테이블별 실패 격리 (Per-Table Failure Isolation)**
    - 하나의 테이블 조회 실패 시 나머지 테이블 데이터 무영향 검증
    - **Validates: Requirements 6.7, 7.3**

  - [x] 6.6 Property 테스트: NULL-to-Option 매핑
    - **Property 6: NULL-to-Option 매핑 정확성**
    - is_null == true → Option::None, is_null == false → Option::Some(값) 검증
    - **Validates: Requirements 5.4**

- [x] 7. Checkpoint - DB 레이어 검증
  - `cargo build` 성공 확인
  - 기존 테스트 모두 통과 확인
  - Ensure all tests pass, ask the user if questions arise.

- [x] 8. Exporter 트레이트 및 팩토리 구현
  - [x] 8.1 Exporter 트레이트 정의 (`export/mod.rs`)
    - `Exporter` 트레이트: `setup()`, `write_tables()`, `finish()` 메서드
    - `create_exporter(format: OutputFormat) -> Box<dyn Exporter>` 팩토리 함수
    - _Requirements: 1.3_

  - [x] 8.2 Property 테스트: 출력 파일명 결정성
    - **Property 9: 출력 파일명 결정성 (Filename Determinism)**
    - Markdown: `{schema}.md`, Excel: `{endpoint}.xlsx`, SQL: `{schema}({endpoint}).sql`
    - 동일 입력에 대해 항상 동일 파일명 생성 검증
    - **Validates: Requirements 9.1, 10.5, 11.1**

- [x] 9. Markdown Exporter 구현
  - [x] 9.1 MarkdownExporter 구현 (`export/markdown.rs`)
    - 스키마별 `{schema}.md` 파일 생성 (기존 파일 덮어쓰기)
    - 제목 + `=============` 밑줄
    - `## Table List` 섹션: `- [{table} ({comment})](#{table-lower})` 불릿 목록
    - BASE TABLE: 일반 정보 표, Columns 표, Index 섹션, Constraint 섹션
    - VIEW: 일반 정보 표 + View Create SQL (코드 블록)
    - Constraint 오타 `Referance` 유지 (Go 버전 호환)
    - Option::None → 빈 문자열 렌더링
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7_

  - [x] 9.2 Property 테스트: Markdown 출력 완전성
    - **Property 10: Markdown 출력 완전성 (Markdown Output Completeness)**
    - Table List 불릿 수 == 테이블 수 검증
    - `## {table}` 섹션 수 == 테이블 수 검증
    - BASE TABLE은 일반 정보/컬럼/인덱스/제약 섹션 포함 검증
    - VIEW는 뷰 정보 + View Create SQL 섹션 포함 검증
    - **Validates: Requirements 9.3, 9.4, 9.5, 9.6**

- [x] 10. Excel Exporter 구현
  - [x] 10.1 ExcelExporter 구현 (`export/excel.rs`)
    - 단일 워크북, 스키마별 시트 생성, 기본 Sheet1 삭제
    - 스타일 정의: title (검정 배경, 흰색 볼드, 전면 테두리), start (하단 테두리), end (상단 테두리)
    - 테이블별 블록: start row → Table name → Description → Column Information
    - BASE TABLE: 컬럼 헤더 + 데이터 행, Indexes, Constraint 섹션
    - VIEW: View Create SQL 섹션
    - Table Information 섹션 (Engine/Row Format, Table Type/Collation)
    - VIEW의 Collation은 ViewInfo.collate 사용
    - Constraint 헤더 오타 `Referance` 유지 (Go 버전 호환)
    - 파일 저장: `{endpoint}.xlsx`
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6_

  - [x] 10.2 Property 테스트: Excel 시트 수 동등성
    - **Property 11: Excel 시트 수 동등성 (Sheet Count Equality)**
    - 생성된 시트 수 == 스키마 수 검증
    - **Validates: Requirements 10.1**

  - [x] 10.3 Property 테스트: Excel 행 번호 단조 증가
    - **Property 12: Excel 행 번호 단조 증가 (Monotonic Row Advance)**
    - 한 테이블 블록 기록 후 행 번호가 시작값보다 큼 검증
    - **Validates: Requirements 10.3**

- [x] 11. SQL Exporter 구현
  - [x] 11.1 SqlExporter 구현 (`export/sql.rs`)
    - 스키마별 `{schema}({endpoint}).sql` 파일 생성 (기존 파일 덮어쓰기)
    - `/* Database : {schema} */` 헤더 주석
    - 테이블별: `/* Table : {table_name} */` → `DROP TABLE IF EXISTS {table_name};` → `{ddl};\n\n\n`
    - DDL 원본 그대로 보존 (트리밍/재작성 금지)
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5_

  - [x] 11.2 Property 테스트: DDL 보존 왕복
    - **Property 13: DDL 보존 왕복 (DDL Preservation Round-Trip)**
    - SQL 파일에서 접두어/접미어 제거 후 원본 DDL과 일치 검증
    - `/* Table : */` 주석 수 == 테이블 수 검증
    - **Validates: Requirements 11.3, 11.4**

- [x] 12. Checkpoint - Exporter 검증
  - `cargo build` 성공 확인
  - 기존 테스트 모두 통과 확인
  - Ensure all tests pass, ask the user if questions arise.

- [x] 13. main.rs 파이프라인 통합 및 로깅
  - [x] 13.1 main.rs 파이프라인 구현
    - `Cli::parse()` → `OutputFormat::from_str()` → `config::load_config()`
    - `DbClient::connect()` → `get_schemas()` → 스키마 없으면 exit(1)
    - `create_exporter()` → 스키마별 루프 (테이블 수집 → write_tables)
    - `exporter.finish()` → 성공 로그
    - 에러 시 `tracing::error!` + exit(1)
    - _Requirements: 1.1, 1.2, 1.5, 1.6, 3.4, 4.5, 13.1, 13.2, 13.3, 13.4, 13.5, 13.6_

  - [x] 13.2 로깅 설정 (tracing-subscriber)
    - `tracing_subscriber::fmt::init()` 초기화
    - 필수 로그 메시지: 앱 이름/버전, `DB Connect Success`, `Setup {Format} Files`, `Get Schema Count`, `{schema} Table Load.`, `{schema} Table Count`, `Export Complete.`
    - 에러 시 ERROR 레벨, 복구 가능 에러 시 WARN/ERROR + continue
    - 비밀번호 로그 출력 금지
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 12.6, 12.7, 12.8_

  - [x] 13.3 Property 테스트: 에러 체인 보존
    - **Property 14: 에러 체인 보존 (Error Chain Preservation)**
    - AppError의 source() 체인에 원본 오류 타입 보존 검증
    - **Validates: Requirements 13.7**

- [x] 14. 유니코드 및 추가 속성 테스트
  - [x] 14.1 Property 테스트: 유니코드 보존 왕복
    - **Property 16: 유니코드 보존 왕복 (Unicode Preservation Round-Trip)**
    - 한국어/일본어/중국어/이모지 포함 문자열을 파일에 기록 후 읽기 시 바이트 일치 검증
    - UTF-8 인코딩, BOM 미포함 검증
    - **Validates: Requirements 15.4, 15.5**

- [x] 15. README.md 작성
  - [x] 15.1 한국어 README.md 작성
    - 프로젝트 설명, 빌드 방법 (`cargo build --release`)
    - 사용법 및 CLI 플래그 설명 (`--output excel|markdown|sql`)
    - 지원 플랫폼, MSRV 정보
    - Go 버전과의 차이점/호환성 설명
    - _Requirements: 15.7_

- [x] 16. Final Checkpoint - 전체 검증
  - `cargo fmt --check` 통과 확인
  - `cargo clippy --all-targets --all-features -- -D warnings` 통과 확인
  - `cargo test --all-features` 전체 테스트 통과 확인
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- `*` 표시된 태스크는 선택적이며 빠른 MVP를 위해 건너뛸 수 있습니다
- 각 태스크는 관련 요구사항 번호를 참조하여 추적 가능합니다
- Checkpoint에서 빌드/테스트 실패 시 이전 태스크를 수정합니다
- Property 테스트는 proptest 크레이트를 사용하며 최소 100회 반복 실행합니다
- Go 버전과의 출력 호환성을 위해 의도적 오타(`Referance`)를 유지합니다
- 모든 DB 쿼리는 파라미터 바인딩을 사용하며, SHOW CREATE TABLE에서만 백틱 인용 식별자를 사용합니다
