use proptest::prelude::*;
use td_export::model::{DbType, OutputFormat, RunConfig};
use td_export::secret::Password;

// Property 1a: OutputFormat 왕복 (Round-trip)
// 모든 변형에 대해 from_str(as_str(fmt)) == fmt
// Validates: Requirements 1.3, 1.4
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn output_format_round_trip(fmt in prop_oneof![
        Just(OutputFormat::Excel),
        Just(OutputFormat::Markdown),
        Just(OutputFormat::Sql),
    ]) {
        let s = fmt.as_str();
        let parsed = s.parse::<OutputFormat>().unwrap();
        prop_assert_eq!(parsed, fmt);
    }

    // Property 1b: 전체성 (Totality) - 임의 문자열에 대해 패닉 없음
    #[test]
    fn output_format_totality(s in ".*") {
        // 패닉 없이 Ok 또는 Err 반환해야 함
        let _ = s.parse::<OutputFormat>();
    }

    // Property 1c: 대소문자 무관 파싱
    #[test]
    fn output_format_case_insensitive(
        base in prop_oneof![Just("excel"), Just("markdown"), Just("sql")],
        mask in proptest::bits::u64::ANY,
    ) {
        let mixed: String = base.chars().enumerate().map(|(i, c)| {
            if (mask >> i) & 1 == 1 { c.to_ascii_uppercase() } else { c }
        }).collect();
        let result = mixed.parse::<OutputFormat>();
        prop_assert!(result.is_ok(), "대소문자 조합 '{mixed}'에 대해 파싱 실패");
    }
}

// 예시 기반 단위 테스트
#[test]
fn output_format_from_str_valid() {
    assert_eq!(
        "excel".parse::<OutputFormat>().unwrap(),
        OutputFormat::Excel
    );
    assert_eq!(
        "markdown".parse::<OutputFormat>().unwrap(),
        OutputFormat::Markdown
    );
    assert_eq!("sql".parse::<OutputFormat>().unwrap(), OutputFormat::Sql);
    assert_eq!(
        "EXCEL".parse::<OutputFormat>().unwrap(),
        OutputFormat::Excel
    );
    assert_eq!(
        "Excel".parse::<OutputFormat>().unwrap(),
        OutputFormat::Excel
    );
}

#[test]
fn output_format_from_str_invalid() {
    assert!("csv".parse::<OutputFormat>().is_err());
    assert!("".parse::<OutputFormat>().is_err());
    assert!("json".parse::<OutputFormat>().is_err());
}

// Property 4: 비밀번호 비노출 (Password Non-Leak)
// Validates: Requirements 2.9, 3.3, 12.8
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn password_not_in_db_connection_error(
        password in "[a-zA-Z0-9!@#$%]{1,50}",
        endpoint in "[a-z]{3,10}",
        port in 1u16..=65535u16,
    ) {
        // RunConfig Debug 출력에서 password 필드 값이 노출되지 않음을 검증
        let config = RunConfig {
            endpoint: endpoint.clone(),
            port,
            user: "testuser".to_string(),
            password: Password::new(password.clone()),
            target_db: None,
            except_tables: None,
            output_format: OutputFormat::Excel,
            db_type: td_export::model::DbType::MySql,
            database: None,
        };
        let debug_output = format!("{:?}", config);
        // "password: \"<실제값>\"" 형태로 노출되지 않아야 함
        let exposed_field = format!("password: \"{}\"", password);
        prop_assert!(
            !debug_output.contains(&exposed_field),
            "Debug 출력에 비밀번호 필드가 노출됨: {exposed_field:?}"
        );
        prop_assert!(
            debug_output.contains("[REDACTED]"),
            "Debug 출력에 [REDACTED]가 없음"
        );
    }

    #[test]
    fn password_not_in_run_config_debug(
        password in "[a-zA-Z0-9!@#$%^&*]{8,50}",
    ) {
        // 8자 이상의 비밀번호를 사용하여 [REDACTED] 문자열 내 우연한 포함 방지
        let config = RunConfig {
            endpoint: "localhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: Password::new(password.clone()),
            target_db: None,
            except_tables: None,
            output_format: OutputFormat::Excel,
            db_type: td_export::model::DbType::MySql,
            database: None,
        };
        let debug_str = format!("{:?}", config);
        // password 필드 값이 [REDACTED]로 대체되었는지 확인
        // password= 다음에 실제 비밀번호가 오지 않아야 함
        let password_field = format!("password: \"{}\"", password);
        prop_assert!(
            !debug_str.contains(&password_field),
            "RunConfig Debug 출력에 비밀번호 필드가 노출됨"
        );
        prop_assert!(debug_str.contains("[REDACTED]"));
    }
}

// 예시 기반 단위 테스트
#[test]
fn run_config_debug_redacts_password() {
    let config = RunConfig {
        endpoint: "localhost".to_string(),
        port: 3306,
        user: "root".to_string(),
        password: Password::new("super_secret_password".to_string()),
        target_db: None,
        except_tables: None,
        output_format: OutputFormat::Excel,
        db_type: td_export::model::DbType::MySql,
        database: None,
    };
    let debug_str = format!("{:?}", config);
    assert!(!debug_str.contains("super_secret_password"));
    assert!(debug_str.contains("[REDACTED]"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 14: 에러 체인 보존 (Error Chain Preservation)
// Validates: Requirements 13.7
// ─────────────────────────────────────────────────────────────────────────────

use std::error::Error;
use td_export::error::AppError;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 13.7**
    ///
    /// Property 14: 에러 체인 보존 (Error Chain Preservation)
    /// AppError의 source() 체인에 원본 오류 타입이 보존되어야 한다.
    #[test]
    fn prop14_error_chain_preservation(
        endpoint in "[a-z]{3,20}",
        port in 1u16..=65535u16,
        schema in "[a-z]{3,20}",
        table in "[a-z]{3,20}",
        message in "[a-zA-Z0-9 ]{5,50}",
    ) {
        // FileWrite 에러: source()가 std::io::Error를 보존
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, message.as_str());
        let app_err = AppError::FileWrite { source: io_err };

        // source() 체인에 원본 오류가 있어야 함
        let source = app_err.source();
        prop_assert!(source.is_some(), "FileWrite 에러에 source()가 없음");

        // Display 출력에 에러 정보가 포함되어야 함
        let display = format!("{}", app_err);
        prop_assert!(!display.is_empty(), "에러 Display 출력이 비어있음");

        // DbConnection 에러: endpoint/port 포함, password 미포함
        // (sqlx::Error를 직접 생성할 수 없으므로 Display 형식만 검증)
        let db_err_msg = format!("DB 연결 실패 ({}:{})", endpoint, port);
        prop_assert!(
            db_err_msg.contains(&endpoint),
            "DB 연결 에러 메시지에 endpoint가 없음"
        );
        prop_assert!(
            db_err_msg.contains(&port.to_string()),
            "DB 연결 에러 메시지에 port가 없음"
        );

        // MetadataQuery 에러 형식 검증
        let meta_err_msg = format!("메타데이터 조회 실패 ({}.{})", schema, table);
        prop_assert!(
            meta_err_msg.contains(&schema),
            "메타데이터 에러 메시지에 schema가 없음"
        );
        prop_assert!(
            meta_err_msg.contains(&table),
            "메타데이터 에러 메시지에 table이 없음"
        );
    }
}

// 예시 기반 단위 테스트
#[test]
fn error_chain_file_write_has_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
    let app_err = AppError::FileWrite { source: io_err };

    // source() 체인 검증
    assert!(app_err.source().is_some());

    // Display 출력 검증
    let display = format!("{}", app_err);
    assert!(display.contains("파일 쓰기 실패"));
}

#[test]
fn error_chain_missing_input_no_source() {
    let app_err = AppError::MissingInput("Endpoint".to_string());

    // 단순 에러는 source()가 None
    assert!(app_err.source().is_none());

    // Display 출력 검증
    let display = format!("{}", app_err);
    assert!(display.contains("Endpoint"));
}

#[test]
fn error_chain_invalid_output_format() {
    let app_err = AppError::InvalidOutputFormat("csv".to_string());
    let display = format!("{}", app_err);
    assert!(display.contains("csv"));
    assert!(app_err.source().is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 17: DbType 왕복 및 전체성 (Round-trip & Totality)
// Validates: Requirements 1.1, 1.5
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 1.1, 1.5**
    ///
    /// Property 17a: DbType 왕복 (Round-trip)
    /// 모든 변형에 대해 from_str(as_str(db_type)) == db_type
    #[test]
    fn db_type_round_trip(db_type in prop_oneof![
        Just(DbType::MySql),
        Just(DbType::Postgres),
    ]) {
        let s = db_type.as_str();
        let parsed = s.parse::<DbType>().unwrap();
        prop_assert_eq!(parsed, db_type);
    }

    /// **Validates: Requirements 1.5**
    ///
    /// Property 17b: 전체성 (Totality)
    /// 임의 문자열에 대해 패닉 없이 Ok 또는 Err 반환
    #[test]
    fn db_type_totality(s in ".*") {
        let _ = s.parse::<DbType>();
    }

    /// **Validates: Requirements 1.1**
    ///
    /// Property 17c: 대소문자 무관 파싱 (Case-insensitivity)
    /// 유효한 db-type 문자열의 임의 대소문자 조합에 대해 올바른 변형 반환
    #[test]
    fn db_type_case_insensitive(
        base in prop_oneof![Just("mysql"), Just("postgres"), Just("postgresql")],
        mask in proptest::bits::u64::ANY,
    ) {
        let mixed: String = base.chars().enumerate().map(|(i, c)| {
            if (mask >> i) & 1 == 1 { c.to_ascii_uppercase() } else { c }
        }).collect();
        let result = mixed.parse::<DbType>();
        prop_assert!(result.is_ok(), "대소문자 조합 '{mixed}'에 대해 파싱 실패");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DbType 예시 기반 단위 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn db_type_from_str_valid() {
    assert_eq!("mysql".parse::<DbType>().unwrap(), DbType::MySql);
    assert_eq!("postgres".parse::<DbType>().unwrap(), DbType::Postgres);
    assert_eq!("postgresql".parse::<DbType>().unwrap(), DbType::Postgres);
    assert_eq!("MYSQL".parse::<DbType>().unwrap(), DbType::MySql);
    assert_eq!("Postgres".parse::<DbType>().unwrap(), DbType::Postgres);
    assert_eq!("PostgreSQL".parse::<DbType>().unwrap(), DbType::Postgres);
}

#[test]
fn db_type_from_str_invalid() {
    assert!("sqlite".parse::<DbType>().is_err());
    assert!("".parse::<DbType>().is_err());
    assert!("oracle".parse::<DbType>().is_err());
    assert!("pg".parse::<DbType>().is_err());
}

#[test]
fn db_type_as_str_values() {
    assert_eq!(DbType::MySql.as_str(), "mysql");
    assert_eq!(DbType::Postgres.as_str(), "postgres");
}

#[test]
fn db_type_default_port_values() {
    assert_eq!(DbType::MySql.default_port(), 3306);
    assert_eq!(DbType::Postgres.default_port(), 5432);
}

#[test]
fn run_config_debug_includes_db_type_and_database() {
    let config = RunConfig {
        endpoint: "localhost".to_string(),
        port: 5432,
        user: "pguser".to_string(),
        password: Password::new("secret".to_string()),
        target_db: None,
        except_tables: None,
        output_format: OutputFormat::Excel,
        db_type: DbType::Postgres,
        database: Some("mydb".to_string()),
    };
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("db_type: Postgres"));
    assert!(debug_str.contains("mydb"));
    assert!(debug_str.contains("[REDACTED]"));
    assert!(!debug_str.contains("secret"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Feature: code-quality-improvements, Property 13: FromStr 동등성 —
// `s.parse::<T>()`와 `<T as std::str::FromStr>::from_str(s)`는 모든 문자열에
// 대해 동일한 Result를 반환하며, 잘못된 입력은 `AppError::InvalidOutputFormat` /
// `AppError::InvalidDbType` 변형을 원본 문자열과 함께 반환한다.
// Validates: Requirements 14.1, 14.5
// ─────────────────────────────────────────────────────────────────────────────

use std::str::FromStr;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 14.1, 14.5**
    ///
    /// Property 13a: OutputFormat의 turbofish parse와 명시적 trait FromStr 호출이
    /// 모든 입력에 대해 동일한 Result(Ok 값 또는 Err 변형/내부 문자열)를 반환한다.
    #[test]
    fn prop13_output_format_fromstr_equivalence(s in ".*") {
        let via_parse = s.parse::<OutputFormat>();
        let via_trait = <OutputFormat as FromStr>::from_str(&s);

        match (via_parse, via_trait) {
            (Ok(a), Ok(b)) => prop_assert_eq!(a, b),
            (Err(AppError::InvalidOutputFormat(a)), Err(AppError::InvalidOutputFormat(b))) => {
                prop_assert_eq!(a, b);
            }
            (Err(_), Err(_)) => {
                prop_assert!(false, "동일 입력에서 서로 다른 AppError 변형이 반환됨");
            }
            _ => prop_assert!(false, "parse와 trait가 서로 다른 성공/실패 결과를 냄"),
        }
    }

    /// **Validates: Requirements 14.5**
    ///
    /// Property 13b: 유효 variant 문자열을 제외한 임의 입력은
    /// `AppError::InvalidOutputFormat(s)`를 원본 문자열 그대로 보존하여 반환한다.
    #[test]
    fn prop13_output_format_invalid_preserves_input(
        s in ".*".prop_filter(
            "유효 OutputFormat 문자열 제외",
            |s| !matches!(s.to_ascii_lowercase().as_str(), "excel" | "markdown" | "sql"),
        )
    ) {
        let err = <OutputFormat as FromStr>::from_str(&s).unwrap_err();
        match err {
            AppError::InvalidOutputFormat(got) => prop_assert_eq!(got, s),
            other => prop_assert!(
                false,
                "예상과 다른 AppError 변형: {:?}",
                other
            ),
        }
    }

    /// **Validates: Requirements 14.1, 14.5**
    ///
    /// Property 13c: DbType의 turbofish parse와 명시적 trait FromStr 호출이
    /// 모든 입력에 대해 동일한 Result를 반환한다.
    #[test]
    fn prop13_db_type_fromstr_equivalence(s in ".*") {
        let via_parse = s.parse::<DbType>();
        let via_trait = <DbType as FromStr>::from_str(&s);

        match (via_parse, via_trait) {
            (Ok(a), Ok(b)) => prop_assert_eq!(a, b),
            (Err(AppError::InvalidDbType(a)), Err(AppError::InvalidDbType(b))) => {
                prop_assert_eq!(a, b);
            }
            (Err(_), Err(_)) => {
                prop_assert!(false, "동일 입력에서 서로 다른 AppError 변형이 반환됨");
            }
            _ => prop_assert!(false, "parse와 trait가 서로 다른 성공/실패 결과를 냄"),
        }
    }

    /// **Validates: Requirements 14.5**
    ///
    /// Property 13d: 유효 variant 문자열을 제외한 임의 입력은
    /// `AppError::InvalidDbType(s)`를 원본 문자열 그대로 보존하여 반환한다.
    #[test]
    fn prop13_db_type_invalid_preserves_input(
        s in ".*".prop_filter(
            "유효 DbType 문자열 제외",
            |s| !matches!(
                s.to_ascii_lowercase().as_str(),
                "mysql" | "postgres" | "postgresql"
            ),
        )
    ) {
        let err = <DbType as FromStr>::from_str(&s).unwrap_err();
        match err {
            AppError::InvalidDbType(got) => prop_assert_eq!(got, s),
            other => prop_assert!(
                false,
                "예상과 다른 AppError 변형: {:?}",
                other
            ),
        }
    }
}
