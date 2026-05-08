//! Feature: code-quality-improvements, Property 2: ConnectOptions 빌드 총체성 —
//! 임의의 유효한 [`RunConfig`]에 대해 `mysql_options`와 `pg_options`는 패닉
//! 없이 `ConnectOptions` 값을 반환해야 한다. 비밀번호에 URL 예약 문자(`@`,
//! `:`, `/`, `?`, `#`, `%`, 공백)가 포함되어도 동일하게 동작해야 하며,
//! 이것이 빌더 API를 사용하는 핵심 이유다.
//!
//! 본 파일은 더불어 `src/db/` 하위 소스의 URL 리터럴(`"mysql://"`,
//! `"postgres://"`, `"postgresql://"`) 부재를 검사해 회귀를 방지한다
//! (Requirements 1.1, 15.4).
//!
//! Validates: Requirements 1.2, 1.1, 15.4

#![allow(clippy::needless_raw_string_hashes)]

use std::path::{Path, PathBuf};

use proptest::prelude::*;
use td_export::db::connect::{mysql_options, pg_options};
use td_export::model::{DbType, OutputFormat, RunConfig};
use td_export::secret::Password;

// ─────────────────────────────────────────────────────────────────────────────
// Property 2 PBT: ConnectOptions 빌드 총체성
// ─────────────────────────────────────────────────────────────────────────────

/// 임의의 비밀번호 문자열 생성기. URL 예약 문자(`@`, `:`, `/`, `?`, `#`, `%`)와
/// 공백을 의도적으로 포함해 빌더 API가 URL 이스케이프와 무관함을 검증한다.
/// 길이 0–100 범위에서 랜덤 샘플링한다.
fn password_strategy() -> impl Strategy<Value = String> {
    r"[@:/?#% a-zA-Z0-9]{0,100}".prop_map(|s| s.to_string())
}

/// 호스트명으로 쓸 만한 문자열. 실제 DNS 해석은 하지 않으므로 형식만 대강 맞춘다.
fn endpoint_strategy() -> impl Strategy<Value = String> {
    r"[a-z][a-z0-9.-]{0,30}".prop_map(|s| s.to_string())
}

/// 사용자 이름 생성기. 일반적인 계정명 범위로 제한한다.
fn user_strategy() -> impl Strategy<Value = String> {
    r"[a-zA-Z_][a-zA-Z0-9_]{0,20}".prop_map(|s| s.to_string())
}

/// 선택적 데이터베이스 이름 생성기.
fn optional_database_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![Just(None), r"[a-zA-Z_][a-zA-Z0-9_]{0,20}".prop_map(Some),]
}

/// target_db / except_tables 리스트 생성기 (빈 Vec 포함 가능).
fn optional_string_list_strategy() -> impl Strategy<Value = Option<Vec<String>>> {
    prop_oneof![
        Just(None),
        proptest::collection::vec(
            r"[a-zA-Z_][a-zA-Z0-9_]{0,20}".prop_map(|s| s.to_string()),
            0..5
        )
        .prop_map(Some),
    ]
}

/// 임의의 [`RunConfig`]를 생성한다. `db_type`/`output_format`은 열거형 전 변형을
/// 고루 섞어 두 빌더 함수 모두의 입력 공간을 커버한다.
fn run_config_strategy() -> impl Strategy<Value = RunConfig> {
    (
        endpoint_strategy(),
        1u16..=65535u16,
        user_strategy(),
        password_strategy(),
        optional_string_list_strategy(),
        optional_string_list_strategy(),
        prop_oneof![
            Just(OutputFormat::Excel),
            Just(OutputFormat::Markdown),
            Just(OutputFormat::Sql),
        ],
        prop_oneof![Just(DbType::MySql), Just(DbType::Postgres)],
        optional_database_strategy(),
    )
        .prop_map(
            |(
                endpoint,
                port,
                user,
                password,
                target_db,
                except_tables,
                output_format,
                db_type,
                database,
            )| RunConfig {
                endpoint,
                port,
                user,
                password: Password::new(password),
                target_db,
                except_tables,
                output_format,
                db_type,
                database,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 2a: `mysql_options(&cfg)`는 임의의 [`RunConfig`]에 대해
    /// 패닉 없이 `MySqlConnectOptions`를 반환한다.
    ///
    /// Validates: Requirements 1.2
    #[test]
    fn mysql_options_is_total(cfg in run_config_strategy()) {
        // 패닉 발생 시 proptest가 자동으로 실패로 보고한다.
        // 반환값을 변수에 바인딩해 drop 전에 빌드 자체가 성립함을 확인한다.
        let _opts = mysql_options(&cfg);
    }

    /// Property 2b: `pg_options(&cfg)`는 임의의 [`RunConfig`]에 대해
    /// 패닉 없이 `PgConnectOptions`를 반환한다.
    ///
    /// Validates: Requirements 1.2
    #[test]
    fn pg_options_is_total(cfg in run_config_strategy()) {
        let _opts = pg_options(&cfg);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 특수 문자 비밀번호 스모크 테스트: 빌더가 URL 이스케이프 문제를 우회함을 확인
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn mysql_options_accepts_password_with_url_reserved_chars() {
    // `@`, `:`, `/`, `?`, `#`, `%`, 공백 — URL 파싱이 깨질 수 있는 조합 전체.
    let raw = "p@ss:wo/rd?%#x with spaces".to_string();
    let cfg = RunConfig {
        endpoint: "db.example.com".to_string(),
        port: 3306,
        user: "root".to_string(),
        password: Password::new(raw),
        target_db: None,
        except_tables: None,
        output_format: OutputFormat::Excel,
        db_type: DbType::MySql,
        database: None,
    };
    // 패닉 없이 반환되면 성공.
    let _opts = mysql_options(&cfg);
}

#[test]
fn pg_options_accepts_password_with_url_reserved_chars() {
    let raw = "p@ss:wo/rd?%#x with spaces".to_string();
    let cfg = RunConfig {
        endpoint: "db.example.com".to_string(),
        port: 5432,
        user: "postgres".to_string(),
        password: Password::new(raw),
        target_db: None,
        except_tables: None,
        output_format: OutputFormat::Sql,
        db_type: DbType::Postgres,
        database: Some("app".to_string()),
    };
    let _opts = pg_options(&cfg);
}

// ─────────────────────────────────────────────────────────────────────────────
// URL literal 부재 검증: `src/db/` 하위 소스에 `"mysql://"` 등이 없어야 한다.
// 회귀 방지 — URL 포매팅 경로가 되살아나지 않도록 테스트로 고정한다.
// Validates: Requirements 1.1, 15.4
// ─────────────────────────────────────────────────────────────────────────────

/// 검색 대상이 되는 URL 스킴 리터럴 (따옴표 포함 — 실제 Rust 문자열 리터럴만 검출).
const FORBIDDEN_URL_LITERALS: &[&str] = &[r#""mysql://"#, r#""postgres://"#, r#""postgresql://"#];

/// `src/db/` 하위의 모든 `.rs` 파일을 재귀 수집한다.
fn collect_db_source_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("디렉토리 읽기 실패: {} ({e})", dir.display()));
    for entry in entries {
        let entry = entry.expect("디렉토리 엔트리 읽기 실패");
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_db_source_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

#[test]
fn no_url_string_literals_in_db_module() {
    // CARGO_MANIFEST_DIR는 테스트 실행 시 크레이트 루트를 가리킨다.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let db_dir = Path::new(manifest_dir).join("src").join("db");
    assert!(
        db_dir.is_dir(),
        "src/db 디렉토리를 찾을 수 없음: {}",
        db_dir.display()
    );

    let files = collect_db_source_files(&db_dir);
    assert!(
        !files.is_empty(),
        "src/db 하위에 .rs 파일이 없음 — 테스트 전제가 깨짐"
    );

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("파일 읽기 실패: {} ({e})", path.display()));
        for needle in FORBIDDEN_URL_LITERALS {
            if content.contains(needle) {
                violations.push(format!(
                    "{}: 금지된 URL 리터럴 {} 발견",
                    path.display(),
                    needle
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "src/db 하위에 URL 문자열 리터럴이 존재함 — ConnectOptions 빌더 대신 \
         URL 포매팅을 사용하고 있을 수 있음:\n{}",
        violations.join("\n")
    );
}
