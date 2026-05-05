// identifier 모듈 Property 테스트
// Property 15: 식별자 인용 왕복 (Identifier Quoting Round-Trip)
// Validates: Requirements 14.2

use proptest::prelude::*;
use td_export::identifier::{
    quote_identifier, quote_pg_identifier, unquote_identifier, unquote_pg_identifier,
    validate_identifier,
};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // 임의 문자열에 대해 quote → unquote 왕복 검증
    #[test]
    fn identifier_quote_unquote_round_trip(id in ".*") {
        let quoted = quote_identifier(&id).unwrap();
        let unquoted = unquote_identifier(&quoted).unwrap();
        prop_assert_eq!(unquoted, id);
    }

    // 위험 문자 포함 시 validate_identifier가 에러 반환
    #[test]
    fn validate_rejects_dangerous_chars(
        prefix in "[a-zA-Z0-9_]{0,10}",
        suffix in "[a-zA-Z0-9_]{0,10}",
        dangerous in prop_oneof![
            Just(";"),
            Just("/*"),
            Just("*/"),
            Just("\n"),
            Just("\r"),
        ],
    ) {
        let id = format!("{prefix}{dangerous}{suffix}");
        let result = validate_identifier(&id);
        prop_assert!(result.is_err(), "위험 문자 포함 식별자가 통과됨: {id:?}");
    }

    // 안전한 식별자는 validate_identifier 통과
    #[test]
    fn validate_accepts_safe_identifiers(
        id in "[a-zA-Z0-9_가-힣]{1,64}",
    ) {
        prop_assert!(validate_identifier(&id).is_ok());
    }
}

// Property 25: PostgreSQL 식별자 인용 왕복 (PG Identifier Round-Trip)
// **Validates: Requirements 9.1, 9.3, 9.5**
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // null 바이트를 포함하지 않는 임의 문자열에 대해 quote → unquote 왕복 검증
    #[test]
    fn pg_identifier_round_trip(id in "[^\0]*") {
        let quoted = quote_pg_identifier(&id).unwrap();
        let unquoted = unquote_pg_identifier(&quoted).unwrap();
        prop_assert_eq!(unquoted, id);
    }

    // null 바이트를 포함하는 문자열에 대해 quote_pg_identifier가 에러 반환
    #[test]
    fn pg_identifier_rejects_null_byte(
        prefix in "[a-zA-Z0-9_]{0,10}",
        suffix in "[a-zA-Z0-9_]{0,10}",
    ) {
        let id = format!("{prefix}\0{suffix}");
        let result = quote_pg_identifier(&id);
        prop_assert!(result.is_err(), "null 바이트 포함 식별자가 통과됨: {id:?}");
    }

    // quote_pg_identifier 출력이 큰따옴표로 인용되며 백틱 인용을 사용하지 않음
    #[test]
    fn pg_identifier_no_backtick_in_output(id in "[^\0]*") {
        let quoted = quote_pg_identifier(&id).unwrap();
        // 큰따옴표로 시작/끝나야 한다 (백틱 인용이 아님)
        prop_assert!(
            quoted.starts_with('"') && quoted.ends_with('"'),
            "PG 인용 결과가 큰따옴표로 감싸지지 않음: {quoted:?}"
        );
        // 백틱으로 시작/끝나지 않아야 한다
        prop_assert!(
            !quoted.starts_with('`') && !quoted.ends_with('`'),
            "PG 인용 결과가 백틱으로 감싸져 있음: {quoted:?}"
        );
    }
}
