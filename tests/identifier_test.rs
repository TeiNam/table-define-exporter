// identifier 모듈 Property 테스트
// Property 15: 식별자 인용 왕복 (Identifier Quoting Round-Trip)
// Validates: Requirements 14.2

use proptest::prelude::*;
use td_export::identifier::{quote_identifier, unquote_identifier, validate_identifier};

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
