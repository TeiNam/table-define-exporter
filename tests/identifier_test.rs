// identifier 모듈 Property 테스트
// Property 15: 식별자 인용 왕복 (Identifier Quoting Round-Trip)
// Validates: Requirements 14.2
//
// Feature: code-quality-improvements, Property 3: 식별자 인용 라운드트립 —
// 위험 문자(`;`, `/*`, `*/`, `\n`, `\r`)를 포함하지 않는 임의 식별자 `id`에 대해
// `unquote_identifier(quote_identifier(id).unwrap()).unwrap() == id`가 성립한다.
// `quote_pg_identifier`/`unquote_pg_identifier` 조합에 대해서도 동일하게 성립한다.
// 검증 테스트: `identifier_quote_unquote_round_trip` (MySQL, 백틱 이스케이프 포함),
//             `pg_identifier_round_trip` (PG, 큰따옴표 이스케이프 포함)
// Validates: Requirements 2.1, 4.3

use proptest::prelude::*;
use td_export::error::AppError;
use td_export::identifier::{
    quote_identifier, quote_pg_identifier, unquote_identifier, unquote_pg_identifier,
    validate_identifier,
};

/// `validate_identifier`가 거부하는 위험 시퀀스가 포함되어 있는지 검사.
fn contains_dangerous(s: &str) -> bool {
    s.contains(';') || s.contains("/*") || s.contains("*/") || s.contains('\n') || s.contains('\r')
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // 위험 문자를 포함하지 않는 임의 문자열에 대해 quote → unquote 왕복 검증
    #[test]
    fn identifier_quote_unquote_round_trip(
        id in ".*".prop_filter(
            "위험 문자 제외",
            |s| !contains_dangerous(s),
        ),
    ) {
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

    // null 바이트·위험 문자를 포함하지 않는 임의 문자열에 대해 quote → unquote 왕복 검증
    #[test]
    fn pg_identifier_round_trip(
        id in "[^\0]*".prop_filter(
            "위험 문자 제외",
            |s| !contains_dangerous(s),
        ),
    ) {
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
    fn pg_identifier_no_backtick_in_output(
        id in "[^\0]*".prop_filter(
            "위험 문자 제외",
            |s| !contains_dangerous(s),
        ),
    ) {
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

// Feature: code-quality-improvements, Property 4: 위험 식별자 거부 —
// 문자열이 위험 문자(`;`, `/*`, `*/`, `\n`, `\r`) 중 하나 이상을 포함하는 경우,
// `quote_identifier`와 `quote_pg_identifier`는 `AppError::UnsafeIdentifier`
// 에러를 반환한다.
// Validates: Requirements 4.1, 4.2
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // quote_identifier는 위험 문자가 삽입된 임의 문자열을
    // AppError::UnsafeIdentifier로 거부한다.
    #[test]
    fn quote_identifier_rejects_dangerous_chars(
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
        let result = quote_identifier(&id);
        prop_assert!(
            matches!(result, Err(AppError::UnsafeIdentifier(_))),
            "quote_identifier가 위험 문자를 거부하지 않음: id={id:?}, result={result:?}"
        );
    }

    // quote_pg_identifier는 위험 문자가 삽입된 임의 문자열을
    // AppError::UnsafeIdentifier로 거부한다.
    #[test]
    fn quote_pg_identifier_rejects_dangerous_chars(
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
        let result = quote_pg_identifier(&id);
        prop_assert!(
            matches!(result, Err(AppError::UnsafeIdentifier(_))),
            "quote_pg_identifier가 위험 문자를 거부하지 않음: id={id:?}, result={result:?}"
        );
    }
}

// Feature: code-quality-improvements, Task 21.3 — quote_* validate 호출 회귀 예제
// 결정적 예제로 세미콜론(`;`)이 포함된 문자열이 `quote_identifier` /
// `quote_pg_identifier` 경로에서 `AppError::UnsafeIdentifier`로 거부됨을 보장한다.
// 이는 Property 4(proptest)의 회귀 방지 안전망으로, 내부 `validate_identifier`
// 호출이 제거되거나 우회될 경우 즉시 실패한다.
// Validates: Requirements 4.1, 4.2, 15.4

#[test]
fn quote_identifier_rejects_semicolon() {
    let result = quote_identifier("x; DROP TABLE y");
    assert!(
        matches!(result, Err(AppError::UnsafeIdentifier(_))),
        "세미콜론 포함 식별자가 거부되지 않음: {result:?}"
    );
}

#[test]
fn quote_pg_identifier_rejects_semicolon() {
    let result = quote_pg_identifier("x; DROP TABLE y");
    assert!(
        matches!(result, Err(AppError::UnsafeIdentifier(_))),
        "세미콜론 포함 PG 식별자가 거부되지 않음: {result:?}"
    );
}
