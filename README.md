# td-export-rust

MySQL 및 PostgreSQL 테이블 정의서를 Excel(.xlsx), Markdown(.md), SQL(.sql) 형식으로 내보내는 CLI 도구입니다.
기존 Go 버전([TD-EXPORT](../td-export(Go)))을 Rust로 재구현하였으며, 출력 형식 호환성을 유지합니다.

## 특징

- **MySQL / PostgreSQL 동시 지원**: `--db-type` 플래그로 DB 종류 선택 (기본값: `mysql`)
- `information_schema` 및 시스템 카탈로그에서 테이블/뷰 메타데이터 수집
- Excel, Markdown, SQL 세 가지 출력 포맷 지원
- 스키마별 파일 분리 출력
- 제외 테이블 와일드카드 패턴 지원
- 비밀번호 에코 없는 안전한 입력
- UTF-8 인코딩 출력 (BOM 미포함)
- PostgreSQL 13~17 버전 호환

## 빌드 방법

### 사전 요구사항

- Rust 1.75 이상 (stable)
- `cargo` 패키지 매니저

### 릴리즈 빌드

```bash
cargo build --release
```

빌드 완료 후 실행 파일 위치: `target/release/td-export`

### 개발 빌드

```bash
cargo build
```

### 테스트 실행

```bash
cargo test
```

## 사용법

```bash
./td-export [OPTIONS]
```

### CLI 플래그

| 플래그 | 기본값 | 설명 |
|--------|--------|------|
| `--db-type` | `mysql` | DB 종류 선택: `mysql`, `postgres` (대소문자 무관) |
| `--output` | `markdown` | 출력 포맷 선택: `excel`, `markdown`, `sql` |
| `--endpoint` | — | DB 서버 호스트명 또는 IP (미지정 시 대화형 입력) |
| `--port` | DB 종류별 (MySQL 3306 / PostgreSQL 5432) | DB 서버 포트 |
| `--user` | — | DB 사용자명 (미지정 시 대화형 입력) |
| `--database` | — | PostgreSQL 데이터베이스 이름 (PostgreSQL 전용) |
| `--target-db` | — | 대상 스키마 목록 (쉼표 구분) |
| `--except-tables` | — | 제외 테이블 패턴 (쉼표 구분, 와일드카드 `%` 사용 가능) |
| `--help` | — | 도움말 출력 |
| `--version` | — | 버전 정보 출력 |

> **참고**: 모든 플래그는 선택사항입니다. 지정하지 않은 항목은 실행 시 대화형 프롬프트로 입력받습니다. 비밀번호는 보안상 CLI 플래그로 받지 않고 항상 프롬프트로만 입력받습니다.

### 실행 예시

```bash
# 완전 대화형 (모든 항목을 프롬프트로 입력, 기본 출력 포맷: Markdown)
./td-export

# MySQL + Excel 출력
./td-export --output excel

# MySQL + SQL 출력
./td-export --output sql

# PostgreSQL + Markdown 출력
./td-export --db-type postgres

# PostgreSQL + Excel 출력
./td-export --db-type postgres --output excel

# 접속 정보까지 CLI로 지정 (비밀번호만 프롬프트)
./td-export --db-type postgres --endpoint db.example.com --user postgres --database myapp --target-db public,app_schema

# 제외 패턴 지정
./td-export --except-tables 'tmp_%,log_%'
```

### 대화식 입력 순서

실행 시 다음 항목을 순서대로 입력합니다:

1. **Endpoint**: DB 서버 호스트명 또는 IP 주소 (필수)
2. **Port**: 포트 번호 (MySQL 기본값: 3306, PostgreSQL 기본값: 5432, 빈 입력 시 기본값 사용)
3. **User**: DB 사용자명 (필수)
4. **Password**: 비밀번호 (에코 없이 입력)
5. **Database**: PostgreSQL 데이터베이스 이름 (필수, `--db-type postgres` 전용 — MySQL에서는 표시되지 않음)
6. **DB**: 대상 스키마 목록 (쉼표 구분, 빈 입력 시 전체 비시스템 스키마)
7. **Exception Tables**: 제외할 테이블 패턴 (쉼표 구분, 와일드카드 `%` 사용 가능)

> **참고**: `Database` 프롬프트는 `--db-type postgres`일 때만 표시됩니다. PostgreSQL은 하나의 데이터베이스 안에 여러 스키마가 존재하는 구조이므로, 먼저 접속할 데이터베이스를 지정한 뒤 그 안의 스키마를 선택합니다.

### 실행 로그 예시 (MySQL)

```
Endpoint : mydb.example.com
Port (default: 3306) : 
User : root
Password : 
DB(Seperator , or Space(All)) : mydb,testdb
Exception Tables(Seperator , or Space(none) / Use wildcard) : tmp_%
INFO td-export 0.1.0
INFO DB Connect Success
INFO Setup Markdown Files
INFO Get Schema Count : 2
INFO mydb Table Load.
INFO mydb Table Count : 15
INFO mydb Table Column/Index/Const Load
INFO testdb Table Load.
INFO testdb Table Count : 8
INFO testdb Table Column/Index/Const Load
INFO Export Complete.
```

### 실행 로그 예시 (PostgreSQL)

```
Endpoint : pgdb.example.com
Port (default: 5432) : 
User : postgres
Password : 
Database : myapp
DB(Seperator , or Space(All)) : public,app_schema
Exception Tables(Seperator , or Space(none) / Use wildcard) : tmp_%
INFO td-export 0.1.0
INFO DB Connect Success
INFO Setup Markdown Files
INFO Get Schema Count : 2
INFO public Table Load.
INFO public Table Count : 12
INFO public Table Column/Index/Const Load
INFO app_schema Table Load.
INFO app_schema Table Count : 5
INFO app_schema Table Column/Index/Const Load
INFO Export Complete.
```

## 출력 파일 형식

### Excel (`{endpoint}.xlsx`)

- 스키마별 시트 생성
- 테이블별 블록: 테이블명, 설명, 컬럼 정보, 인덱스, 제약 조건, 테이블 정보
- 뷰(VIEW): View Create SQL 포함

### Markdown (`{schema}.md`)

- 스키마별 파일 생성
- 목차(Table List) 섹션 포함
- 테이블별 섹션: 일반 정보, 컬럼 표, 인덱스, 제약 조건
- 뷰(VIEW): 뷰 정보 + View Create SQL 코드 블록

### SQL (`{schema}({endpoint}).sql`)

- 스키마별 파일 생성
- 데이터베이스 헤더 주석 포함
- 테이블별: `DROP TABLE IF EXISTS` + 원본 DDL

## 지원 데이터베이스

### MySQL

- MySQL 5.7 이상
- 기본 포트: 3306
- `information_schema`에서 메타데이터 수집
- `SHOW CREATE TABLE`로 DDL 추출
- 백틱(`` ` ``) 식별자 인용

### PostgreSQL

- **지원 버전**: PostgreSQL 13, 14, 15, 16, 17
- 기본 포트: 5432
- `information_schema` + `pg_catalog` 시스템 카탈로그에서 메타데이터 수집
- DDL 재구성 방식 (PostgreSQL에는 `SHOW CREATE TABLE` 없음)
- 큰따옴표(`"`) 식별자 인용

#### PostgreSQL 접속 요구사항

| 항목 | 설명 |
|------|------|
| Endpoint | PostgreSQL 서버 호스트명 또는 IP 주소 |
| Port | 기본값 5432 |
| User | PostgreSQL 사용자명 |
| Password | 비밀번호 |
| Database | 접속할 데이터베이스 이름 (필수) |

#### PostgreSQL 권한 요구사항

PostgreSQL 사용자에게 다음 권한이 필요합니다:

- `information_schema`에 대한 `SELECT` 권한
- `pg_catalog` 시스템 카탈로그에 대한 `SELECT` 권한
- 대상 스키마의 테이블/뷰에 대한 메타데이터 조회 권한

일반적으로 데이터베이스에 `CONNECT` 권한이 있고 스키마에 `USAGE` 권한이 있는 사용자라면 메타데이터 조회가 가능합니다.

#### PostgreSQL 동작 특성

| 항목 | MySQL | PostgreSQL |
|------|-------|-----------|
| 스키마 계층 | `database == schema` | `database → schema → table` (하나의 DB 안에 여러 스키마) |
| 시스템 스키마 | `information_schema`, `mysql`, `sys`, `performance_schema`, `tmp` | `pg_catalog`, `information_schema`, `pg_toast`, `pg_temp_*`, `pg_toast_temp_*` (자동 제외) |
| 식별자 인용 | 백틱 `` ` `` | 큰따옴표 `"` |
| DDL 추출 | `SHOW CREATE TABLE` | `information_schema` + `pg_catalog` 기반 재구성 |
| 테이블 collation | 테이블별 `table_collation` | DB 레벨 collation (모든 테이블 동일) |
| Engine / Row Format | 값 존재 (예: `InnoDB`, `Dynamic`) | 해당 없음 (출력 시 빈 문자열) |
| 자동 증가 | `AUTO_INCREMENT` | `IDENTITY` 컬럼 또는 `serial`/`bigserial` (sequence 기반) |

## 지원 플랫폼

| 플랫폼 | 아키텍처 | 지원 여부 |
|--------|----------|----------|
| Linux | x86_64 | ✅ |
| macOS | x86_64 | ✅ |
| macOS | aarch64 (Apple Silicon) | ✅ |
| Windows | x86_64 | ✅ |

**MSRV (Minimum Supported Rust Version)**: 1.75

## Go 버전과의 차이점 및 호환성

### 출력 호환성

Go 버전과 동일한 출력 형식을 유지합니다:

- Excel 시트 레이아웃 및 셀 병합 구조 동일
- Markdown 섹션 구성 및 표 형식 동일
- SQL 파일 헤더/테이블 주석 형식 동일
- 의도적 오타 유지 (Go 버전 호환):
  - Markdown/Excel Constraint 섹션: `Referance` (Reference 아님)

### 주요 차이점

| 항목 | Go 버전 | Rust 버전 |
|------|---------|----------|
| 런타임 | GC 기반 | 제로 코스트 추상화 |
| 에러 처리 | `error` 인터페이스 | `thiserror` + `anyhow` |
| 로깅 | `logrus` | `tracing` + `tracing-subscriber` |
| Excel 라이브러리 | `excelize` | `rust_xlsxwriter` |
| NULL 처리 | `sql.NullString` | `Option<String>` |
| 비밀번호 입력 | `terminal.ReadPassword` | `rpassword` |

### CLI 플래그 차이

| Go 버전 | Rust 버전 |
|---------|----------|
| `-output=excel` | `--output excel` |
| — | `--db-type postgres` (Rust 버전에서 추가) |

## 라이선스

이 프로젝트는 Go 버전과 동일한 라이선스를 따릅니다.

