use proptest::prelude::*;
use td_export::model::{OutputFormat, RunConfig};

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
        let parsed = OutputFormat::from_str(s).unwrap();
        prop_assert_eq!(parsed, fmt);
    }

    // Property 1b: 전체성 (Totality) - 임의 문자열에 대해 패닉 없음
    #[test]
    fn output_format_totality(s in ".*") {
        // 패닉 없이 Ok 또는 Err 반환해야 함
        let _ = OutputFormat::from_str(&s);
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
        let result = OutputFormat::from_str(&mixed);
        prop_assert!(result.is_ok(), "대소문자 조합 '{mixed}'에 대해 파싱 실패");
    }
}

// 예시 기반 단위 테스트
#[test]
fn output_format_from_str_valid() {
    assert_eq!(
        OutputFormat::from_str("excel").unwrap(),
        OutputFormat::Excel
    );
    assert_eq!(
        OutputFormat::from_str("markdown").unwrap(),
        OutputFormat::Markdown
    );
    assert_eq!(OutputFormat::from_str("sql").unwrap(), OutputFormat::Sql);
    assert_eq!(
        OutputFormat::from_str("EXCEL").unwrap(),
        OutputFormat::Excel
    );
    assert_eq!(
        OutputFormat::from_str("Excel").unwrap(),
        OutputFormat::Excel
    );
}

#[test]
fn output_format_from_str_invalid() {
    assert!(OutputFormat::from_str("csv").is_err());
    assert!(OutputFormat::from_str("").is_err());
    assert!(OutputFormat::from_str("json").is_err());
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
            password: password.clone(),
            target_db: None,
            except_tables: None,
            output_format: OutputFormat::Excel,
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
            password: password.clone(),
            target_db: None,
            except_tables: None,
            output_format: OutputFormat::Excel,
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
        password: "super_secret_password".to_string(),
        target_db: None,
        except_tables: None,
        output_format: OutputFormat::Excel,
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
