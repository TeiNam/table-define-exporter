# td-export

![Rust](https://img.shields.io/badge/Rust-1.94+-orange.svg)
![MySQL](https://img.shields.io/badge/MySQL-5.7+-4479A1.svg)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13--18-336791.svg)
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
- 추출 계측: 테이블별 추출이 1초를 넘으면 경고 로그, 스키마별 수집 메타데이터의 메모리 사용량 추정 표시

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

사전 요구사항: Rust 1.94 이상 (stable).

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
| `--ssl-mode` | `prefer` | TLS: `disable`, `prefer`, `require`, `verify-ca`, `verify-full` ([TLS](#지원-데이터베이스) 참고) |
| `--ssl-ca` | — | 서버 인증서를 검증할 CA 인증서(PEM) — `verify-ca`/`verify-full`과 함께 사용 |
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

# 서버 인증서 검증 (예: RDS CA 번들)
./td-export --endpoint mydb.xxxx.rds.amazonaws.com --ssl-mode verify-full --ssl-ca global-bundle.pem

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

파일명의 `{endpoint}` 자리에는 같은 호스트의 다른 인스턴스·DB 출력이 서로 덮어쓰지 않도록, 포트가 기본값(3306/5432)이 아니면 `_{port}`, PostgreSQL이면 `@{database}`가 붙습니다 (예: `public(db.local_55432@mydb).md`). 파일명에 쓸 수 없는 문자(`/ \ : * ? " < > |`)는 `_`로 바뀌고, Windows 장치 이름(`NUL`, `CON.x` 등)은 앞에 `_`가 붙으며, 255바이트를 넘는 이름은 잘라서 해시를 붙입니다.

### Excel (`{endpoint}.xlsx`)

- 스키마별 시트 생성
- 테이블별 블록: 테이블명, 설명, 컬럼 정보, 인덱스, 제약 조건, 테이블 정보
- 뷰(VIEW): View Create SQL 포함

### Markdown (`{schema}({endpoint}).md`)

- 스키마별 파일 생성
- 목차(Table List) 섹션 포함
- 테이블별 섹션: 일반 정보, 컬럼 표, 인덱스(파셜 인덱스 `WHERE` 절 포함), 제약 조건
- 인덱스 종류는 `Normal` / `Unique` / `Fulltext` / `Spatial`로 표시하고, 컬럼에는 내림차순(`DESC`), prefix 길이(`col(10)`), 함수식(`(lower(name))`), PostgreSQL의 `NULLS`·opclass를 그대로 남깁니다 (Excel 동일)
- 컬럼 기본값: `NULL`(기본값이 NULL), 빈칸(기본값 없음 — NOT NULL·generated 컬럼), 문자열·날짜 리터럴은 `'...'`로 감싸 문자열 `'NULL'`·빈 문자열 `''`과 구분합니다 (Excel 동일)
- 뷰(VIEW): 뷰 정보 + View Create SQL 코드 블록 (언어 태그 `sql`)

### SQL (`{schema}({endpoint}).sql`)

- 스키마별 파일 생성
- 데이터베이스 헤더 주석(`/* Database : ... */`) 포함
- 테이블별: 테이블 주석(`/* Table : ... */`) + 원본 CREATE DDL (정확히 하나의 `;`로 종결)
- `DROP TABLE IF EXISTS` 구문은 출력하지 않습니다 (CREATE DDL만 출력). 단, 위험 식별자를 포함한 테이블은 안전을 위해 출력에서 스킵합니다.
- 파일을 그대로 실행할 수 있도록 FK 순서를 처리합니다 — MySQL은 mysqldump처럼 파일 앞뒤에서 `FOREIGN_KEY_CHECKS`를 잠시 끄고 원래 값으로 되돌리고, PostgreSQL은 pg_dump처럼 FK를 `CREATE TABLE` 밖으로 빼 파일 끝 `/* Foreign Keys */`에 `ALTER TABLE ... ADD CONSTRAINT`로 모읍니다 (검증하지 않은 `NOT VALID` 제약과, 다른 테이블의 identity 시퀀스를 쓰는 기본값도 여기서 추가). PostgreSQL에서 다른 스키마를 참조하는 FK가 있으면 참조되는 스키마의 파일을 먼저 실행하세요.
- 뷰는 참조하는 테이블보다 늦게 만들어지도록 모든 테이블 뒤에 출력합니다. MySQL 뷰는 mysqldump처럼 DB 이름 없이 출력하므로, 실행할 DB를 먼저 선택(`USE`)하면 이름이 다른 DB에도 그대로 만들어집니다.
- PostgreSQL은 테이블이 참조하는 사용자 타입(enum·도메인·복합 타입)과 시퀀스 생성문을 파일 맨 앞 `/* Types & Sequences */`에 출력하고, 코멘트는 `COMMENT ON` 문으로 붙입니다. 파티션 테이블은 부모 DDL 뒤에 `PARTITION OF`로 하위 파티션을 이어 붙이고, 머티리얼라이즈드 뷰는 `WITH NO DATA`(데이터는 `REFRESH`로 채움), 외부 테이블은 `CREATE FOREIGN TABLE ... SERVER ...`(컬럼 옵션은 `ALTER FOREIGN TABLE ... ALTER COLUMN ... OPTIONS`)로 출력합니다 (`CREATE SERVER`·USER MAPPING은 출력하지 않으므로 실행 전에 같은 이름의 서버가 있어야 합니다). 컬럼의 `COLLATE`와 PostgreSQL 18의 NOT NULL 제약 이름·`NO INHERIT`도 보존합니다.

## 지원 데이터베이스

기본(`--ssl-mode prefer`)은 서버가 TLS를 지원하면 TLS로 접속합니다 (서버 인증서는 검증하지 않음). 덕분에 MySQL 8 기본 인증(`caching_sha2_password`)을 서버 재시작 직후에도 쓸 수 있고, SSL을 강제하는 서버(`hostssl`, RDS `rds.force_ssl` 등)에도 접속됩니다.

- `require`: TLS 필수 (인증서 검증 없음)
- `verify-ca`: 서버 인증서가 신뢰하는 CA로 서명됐는지 검증, `verify-full`: 여기에 인증서의 호스트 이름이 `--endpoint`와 같은지까지 검증. `--ssl-ca`를 주지 않으면 공개 루트 인증서로 검증하므로, 사설 CA·RDS 같은 클라우드 CA는 CA 번들 파일을 `--ssl-ca`로 지정하세요.
- `disable`: TLS를 쓰지 않습니다. TLS 1.2 이상을 지원하지 않는 구형 서버(예: yaSSL로 빌드된 MySQL 5.7.27 이하 — TLS 1.0/1.1만 지원)는 `prefer`에서 TLS 협상이 실패하므로 `--ssl-mode disable`로 접속하세요.

### MySQL

- MySQL 5.7 이상, 기본 포트 3306
- `information_schema`에서 메타데이터 수집
- `SHOW CREATE TABLE`로 DDL 추출
- 백틱(`` ` ``) 식별자 인용

### PostgreSQL

- PostgreSQL 13, 14, 15, 16, 17, 18 지원, 기본 포트 5432
- `information_schema` + `pg_catalog`에서 메타데이터 수집
- DDL 재구성 방식 (FK 해석은 단일 JOIN 쿼리로 처리 — N+1 없음). 타입은 pg_dump와 같은 `format_type` 표기 (`integer`, `character varying(11)`, `timestamp(3) without time zone`)
- 하위 파티션은 목록에 따로 나오지 않고 부모 테이블에 포함, 머티리얼라이즈드 뷰·외부 테이블 지원
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

# 의존성 보안 감사 (cargo-audit 필요: cargo install cargo-audit --locked)
cargo audit
```

### 푸시 전 체크리스트

CI가 막는 항목을 로컬에서 미리 검증하면 왕복을 줄일 수 있다. 푸시 전 다음을 순서대로 통과시킬 것:

```bash
cargo fmt --check                                       # 1. 포맷
cargo clippy --all-targets --all-features -- -D warnings # 2. 린트 (warning=에러)
cargo test --all-features                               # 3. 전체 테스트
cargo audit                                             # 4. 보안 감사
```

- **의존성 업데이트 시**: `cargo update` 후 반드시 `cargo build`로 MSRV(1.94) 호환을 확인한다. 새 버전이 더 높은 Rust를 요구하면(예: edition 2024를 요구하는 transitive crate) CI MSRV 잡이 깨진다.
- **edition/MSRV 변경 시**: `Cargo.toml`, `.github/workflows/ci.yml` 매트릭스, 이 README의 표기를 함께 갱신한다.
- **Cargo.lock 버전 포맷**: 로컬 cargo가 lock을 v4로 다시 쓸 수 있다. CI MSRV가 그 포맷을 읽을 수 있어야 한다(lock v4 → Cargo 1.78+).

### MSRV

**Minimum Supported Rust Version**: 1.94 (Rust edition 2024, sqlx 0.9 요구사항)

### CI/CD

- **CI**: `cargo fmt` / `clippy` / `test` / `cargo-llvm-cov` (65% 라인 커버리지 게이트, DB 접속 필요 파일 제외) / `cargo audit` — Rust 1.94와 stable 매트릭스
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
| 추출 시간/메모리 계측 | — | 추가 (느린 테이블 경고 + 메모리 추정) |

### 출력 호환성

원본 Go 버전의 출력 바이트 시퀀스를 최대한 유지합니다. 단, 다음은 **버그 수정**으로 인한 의도된 차이입니다:

- Markdown VIEW 코드블록: 이전 한 줄 `` ```{sql}``` `` 형태 → 표준 fenced 코드블록으로 수정 (GitHub/IDE 뷰어에서 SQL 하이라이트 정상 동작)

의도적 오타(`Referance`)는 `Reference`로 수정되었습니다. 기존 Go 버전 출력물과 이 필드 라벨이 다릅니다.

## 라이선스

[MIT License](./LICENSE) — Copyright (c) 2023 Sizzlei (원본), Copyright (c) 2026 teinam (Rust 포트).
