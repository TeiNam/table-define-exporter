# td-export

![Rust](https://img.shields.io/badge/Rust-1.78+-orange.svg)
![MySQL](https://img.shields.io/badge/MySQL-5.7+-4479A1.svg)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13--17-336791.svg)
![GitHub Actions](https://img.shields.io/badge/GitHub%20Actions-CI/CD-2088FF.svg)
![License](https://img.shields.io/badge/License-MIT-green.svg)

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/teinam)

MySQL 및 PostgreSQL 테이블 정의서를 Excel(.xlsx), Markdown(.md), SQL(.sql) 형식으로 내보내는 CLI 도구입니다.

## 원본 프로젝트와의 관계

본 프로젝트는 [sizzlei/TD-EXPORT](https://github.com/sizzlei/TD-EXPORT) (Go 구현)를 원작자([@sizzlei](https://github.com/sizzlei))의 허락을 받아 **Rust로 재구현하고 고도화**한 포트입니다.

- 원본 출력 형식(바이트 단위)을 최대한 호환 유지
- PostgreSQL 지원, 파셜 인덱스·배열 타입 보존, FK N+1 쿼리 제거, 병렬 메타데이터 수집 등 품질 개선
- 원본과 동일한 **MIT 라이선스**를 따릅니다 ([LICENSE](./LICENSE))

### 라이선스 및 사용 안내

- **라이선스**: MIT — 원본 저작권(© 2023 Sizzlei)과 본 포트 저작권(© 2026 teinam) 고지가 함께 포함됩니다.
- **상업적 사용**: MIT 라이선스상 법적으로는 허용되나, 원저작자 및 본 포트 기여자의 노력을 존중하는 차원에서 **상업적 이용은 지양해 주시기 바랍니다**. 비상업 용도(사내 문서화, 교육, 오픈소스 기여, 개인 프로젝트 등)로 자유롭게 사용하세요.
- 상업적 사용이 필요하시면 원작자와 본 포트 기여자에게 먼저 문의해 주세요.

## 특징

- **MySQL / PostgreSQL 동시 지원**: `--db-type` 플래그로 DB 종류 선택 (기본값: `mysql`)
- `information_schema` 및 시스템 카탈로그에서 테이블/뷰 메타데이터 수집
- Excel, Markdown, SQL 세 가지 출력 포맷 지원
- 스키마별 파일 분리 출력
- 제외 테이블 와일드카드 패턴 지원
- 비밀번호 에코 없는 안전한 입력 (특수문자 포함 지원)
- 파셜 인덱스 `WHERE` 절 보존 및 렌더링 (PostgreSQL)
- PostgreSQL 배열 타입 파라미터 보존 (`varchar(255)[]`, `numeric(10,2)[]` 등)
- UTF-8 인코딩 출력 (BOM 미포함)
- PostgreSQL 13~17 버전 호환

## 설치

### 릴리즈 바이너리 (권장)

[GitHub Releases](../../releases)에서 플랫폼별 빌드를 내려받으세요:

| 플랫폼 | 아키텍처 | 파일 |
|--------|----------|------|
| Linux | x86_64 | `td-export-linux-x86_64.tar.gz` |
| macOS | x86_64 | `td-export-macos-x86_64.tar.gz` |
| macOS | aarch64 (Apple Silicon) | `td-export-macos-aarch64.tar.gz` |
| Windows | x86_64 | `td-export-windows-x86_64.zip` |

### 소스에서 빌드

사전 요구사항: Rust 1.78 이상 (stable).

```bash
cargo build --release
# 결과: target/release/td-export
```

## 사용법

```bash
./td-export [OPTIONS]
```

### CLI 플래그

| 플래그 | 기본값 | 설명 |
|--------|--------|------|
| `--db-type` | `mysql` | DB 종류: `mysql`, `postgres` |
| `--output` | `markdown` | 출력 포맷: `excel`, `markdown`, `sql` |
| `--endpoint` | — | DB 서버 호스트명 또는 IP |
| `--port` | MySQL 3306 / PostgreSQL 5432 | DB 서버 포트 |
| `--user` | — | DB 사용자명 |
| `--database` | — | PostgreSQL 데이터베이스 이름 (PostgreSQL 전용) |
| `--target-db` | — | 대상 스키마 목록 (쉼표 구분) |
| `--except-tables` | — | 제외 테이블 패턴 (쉼표 구분, 와일드카드 `%`) |
| `--help` | — | 도움말 |
| `--version` | — | 버전 정보 |

> 모든 플래그는 선택사항입니다. 지정하지 않은 항목은 실행 시 대화형 프롬프트로 입력받습니다. 비밀번호는 보안상 CLI 플래그로 받지 않고 항상 프롬프트로만 입력받습니다.

### 실행 예시

```bash
# 완전 대화형 (기본 출력: Markdown)
./td-export

# MySQL + Excel 출력
./td-export --output excel

# PostgreSQL + Markdown 출력
./td-export --db-type postgres --endpoint db.example.com --user postgres --database myapp

# 특정 스키마만 내보내기 + 제외 패턴
./td-export --target-db public,app_schema --except-tables 'tmp_%,log_%'

# 상세 로그 활성화
RUST_LOG=debug ./td-export
```

### 대화식 입력 순서

1. **Output Format**: `1) excel`, `2) markdown (default)`, `3) sql`
2. **DB Type**: `1) mysql (default)`, `2) postgres`
3. **Endpoint**: DB 서버 호스트명 또는 IP (필수)
4. **Port**: 포트 번호 (엔터 시 기본값 사용)
5. **User**: DB 사용자명 (필수)
6. **Password**: 비밀번호 (에코 없이 입력)
7. **Database**: PostgreSQL 데이터베이스 이름 (PostgreSQL 전용)
8. **DB**: 대상 스키마 목록 (쉼표 구분, 엔터 시 전체)
9. **Exception Tables**: 제외할 테이블 패턴 (쉼표 구분, 와일드카드 `%`)

숫자 대신 이름(`excel`, `postgres` 등) 입력도 그대로 지원합니다.

## 출력 파일 형식

### Excel (`{endpoint}.xlsx`)

- 스키마별 시트 생성
- 테이블별 블록: 테이블명, 설명, 컬럼 정보, 인덱스, 제약 조건, 테이블 정보
- 뷰(VIEW): View Create SQL 포함

### Markdown (`{schema}.md`)

- 스키마별 파일 생성
- 목차(Table List) 섹션 포함
- 테이블별 섹션: 일반 정보, 컬럼 표, 인덱스(파셜 인덱스 `WHERE` 절 포함), 제약 조건
- 뷰(VIEW): 뷰 정보 + View Create SQL 코드 블록 (언어 태그 `sql`)

### SQL (`{schema}({endpoint}).sql`)

- 스키마별 파일 생성
- 데이터베이스 헤더 주석(`/* Database : ... */`) 포함
- 테이블별: 테이블 주석(`/* Table : ... */`) + 원본 CREATE DDL (정확히 하나의 `;`로 종결)
- `DROP TABLE IF EXISTS` 구문은 출력하지 않습니다 (CREATE DDL만 출력). 단, 위험 식별자를 포함한 테이블은 안전을 위해 출력에서 스킵합니다.

## 지원 데이터베이스

### MySQL

- MySQL 5.7 이상, 기본 포트 3306
- `information_schema`에서 메타데이터 수집
- `SHOW CREATE TABLE`로 DDL 추출
- 백틱(`` ` ``) 식별자 인용

### PostgreSQL

- PostgreSQL 13, 14, 15, 16, 17 지원, 기본 포트 5432
- `information_schema` + `pg_catalog`에서 메타데이터 수집
- DDL 재구성 방식 (FK 해석은 단일 JOIN 쿼리로 처리 — N+1 없음)
- 큰따옴표(`"`) 식별자 인용

#### PostgreSQL 권한 요구사항

- 데이터베이스에 `CONNECT` + 스키마에 `USAGE` 권한
- `information_schema` 및 `pg_catalog`에 대한 `SELECT` 권한

## 개발

```bash
# 테스트 실행 (unit + integration + property-based)
cargo test --all-features

# 포맷 확인
cargo fmt --check

# 린트
cargo clippy --all-targets --all-features -- -D warnings

# 커버리지 (cargo-llvm-cov 필요)
cargo llvm-cov --all-features
```

### MSRV

**Minimum Supported Rust Version**: 1.78

### CI/CD

- **CI**: `cargo fmt` / `clippy` / `test` / `cargo-llvm-cov` (70% 라인 커버리지 게이트) / `cargo audit` — Rust 1.78와 stable 매트릭스
- **Release**: `main` 브랜치에 push 시 자동으로 patch 버전 bump + 4개 플랫폼(linux/macos×2/windows) 바이너리 빌드 + GitHub Release 생성

## 원본(Go)과의 주요 차이점

| 항목 | Go 버전 | Rust 버전 |
|------|---------|----------|
| 런타임 | GC | 제로 코스트 추상화 |
| 에러 처리 | `error` 인터페이스 | `thiserror` + `anyhow` |
| 로깅 | `logrus` | `tracing` + `EnvFilter` |
| Excel 라이브러리 | `excelize` | `rust_xlsxwriter` |
| 비밀번호 입력 | `terminal.ReadPassword` | `rpassword` + 마스킹 래퍼 |
| 테이블 메타데이터 수집 | 직렬 | 병렬(`buffered(4)`, 순서 보존) |
| PostgreSQL 지원 | — | 추가 |
| 파셜 인덱스 `WHERE` 보존 | — | 추가 |

### 출력 호환성

원본 Go 버전의 출력 바이트 시퀀스를 최대한 유지합니다. 단, 다음은 **버그 수정**으로 인한 의도된 차이입니다:

- Markdown VIEW 코드블록: 이전 한 줄 `` ```{sql}``` `` 형태 → 표준 fenced 코드블록으로 수정 (GitHub/IDE 뷰어에서 SQL 하이라이트 정상 동작)

의도적 오타(`Referance`)는 `Reference`로 수정되었습니다. 기존 Go 버전 출력물과 이 필드 라벨이 다릅니다.

## 라이선스

[MIT License](./LICENSE) — Copyright (c) 2023 Sizzlei (원본), Copyright (c) 2026 teinam (Rust 포트).
