// config 모듈 테스트
// Property 12: 빈 Vec 정규화 (Empty Vec Normalization)
// Property 18: DbType별 기본 포트 (Default Port by DbType)

use proptest::prelude::*;
use td_export::config::parse_comma_separated;
use td_export::model::DbType;

// parse_port는 pub(crate)이므로 통합 테스트에서 직접 호출할 수 없다.
// 대신 DbType::default_port()를 검증하고, parse_port의 동작은
// src/config.rs 내부 #[cfg(test)] 모듈에서 검증한다.
// 여기서는 DbType별 기본 포트 값의 정확성을 속성 기반으로 검증한다.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // Property 18: DbType별 기본 포트 정확성
    // For any DbType 변형에 대해, default_port()는 올바른 기본 포트를 반환해야 한다.
    // - DbType::MySql → 3306
    // - DbType::Postgres → 5432
    //
    // **Validates: Requirements 2.1, 2.3, 2.4**
    #[test]
    fn default_port_matches_db_type(
        db_type in prop_oneof![
            Just(DbType::MySql),
            Just(DbType::Postgres),
        ],
    ) {
        let port = db_type.default_port();
        match db_type {
            DbType::MySql => prop_assert_eq!(port, 3306, "MySQL 기본 포트는 3306이어야 함"),
            DbType::Postgres => prop_assert_eq!(port, 5432, "Postgres 기본 포트는 5432이어야 함"),
        }
    }

    // Property 18: DbType별 기본 포트 — from_str 후 default_port 일관성
    // 유효한 db-type 문자열로 파싱한 DbType의 default_port()가 올바른 값을 반환해야 한다.
    //
    // **Validates: Requirements 2.1, 2.3, 2.4**
    #[test]
    fn default_port_consistent_after_from_str(
        input in prop_oneof![
            Just("mysql".to_string()),
            Just("postgres".to_string()),
            Just("postgresql".to_string()),
            Just("MYSQL".to_string()),
            Just("POSTGRES".to_string()),
            Just("PostgreSQL".to_string()),
        ],
    ) {
        let db_type = input.parse::<DbType>().unwrap();
        let port = db_type.default_port();
        match db_type {
            DbType::MySql => prop_assert_eq!(port, 3306),
            DbType::Postgres => prop_assert_eq!(port, 5432),
        }
    }
}

// Feature: code-quality-improvements, Property 12: 빈 Vec 정규화 —
// For any `v: Vec<String>`, `Some(v).filter(|v| !v.is_empty())`의 결과는
// `v.is_empty()`이면 `None`이고 그 외에는 `Some(v)`이다.
// `None`은 항상 `None`으로 유지되며, 쉼표/공백만으로 구성된 문자열을
// `parse_comma_separated`에 넘기면 `None`을 반환한다.
//
// **Validates: Requirements 12.1, 12.2, 12.3**
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // Property 12a: 쉼표와 공백만 있는 입력은 항상 None을 반환한다.
    // 입력 문자열이 ',' 및 ASCII 공백(스페이스, 탭)으로만 구성된 경우
    // `parse_comma_separated` 결과는 `None`이어야 한다 (Req 12.3).
    #[test]
    fn parse_comma_separated_only_commas_and_spaces_is_none(
        s in "[, \t]{0,30}",
    ) {
        prop_assert!(
            parse_comma_separated(&s).is_none(),
            "입력 {:?}는 None을 반환해야 하지만 {:?}를 반환함",
            s,
            parse_comma_separated(&s)
        );
    }

    // Property 12b: Option<Vec<String>>.filter(|v| !v.is_empty()) 동작 검증
    // `load_config`의 오버라이드 정규화 로직과 동등하다 (Req 12.1, 12.2).
    // - 빈 Vec이면 None으로 정규화
    // - 비어있지 않은 Vec은 그대로 Some(v)
    // - None은 None으로 유지
    #[test]
    fn option_vec_normalize_empty_to_none(
        v in proptest::collection::vec(any::<String>(), 0..=10),
    ) {
        let input = Some(v.clone());
        let normalized = input.filter(|inner| !inner.is_empty());
        if v.is_empty() {
            prop_assert!(
                normalized.is_none(),
                "빈 Vec은 None으로 정규화되어야 함"
            );
        } else {
            prop_assert_eq!(
                normalized,
                Some(v),
                "비어있지 않은 Vec은 Some(v)로 유지되어야 함"
            );
        }
    }

    // Property 12b': None 입력은 항상 None으로 유지
    #[test]
    fn option_vec_normalize_none_stays_none(_dummy in 0u8..=255u8) {
        let input: Option<Vec<String>> = None;
        let normalized = input.filter(|inner| !inner.is_empty());
        prop_assert!(normalized.is_none());
    }

    // Property 12c: 모든 원소가 공백/빈 문자열인 Vec<String>을 쉼표로 이어
    // `parse_comma_separated`에 넘기면 `None`을 반환한다.
    // 이는 사용자가 `--target-db " , ,  "` 같은 입력을 주더라도 전체 스키마가
    // 필터아웃되지 않도록 하는 안전 장치다 (Req 12.3).
    #[test]
    fn parse_comma_separated_all_blank_elements_is_none(
        parts in proptest::collection::vec("[ \t]{0,10}", 0..=8),
    ) {
        let input = parts.join(",");
        prop_assert!(
            parse_comma_separated(&input).is_none(),
            "공백 원소만 포함한 입력 {:?}는 None이어야 하지만 {:?}를 반환함",
            input,
            parse_comma_separated(&input)
        );
    }

    // Property 12d: 최소 한 개의 비공백 원소가 섞인 입력은 Some(non_empty_vec)을
    // 반환하며, 반환된 Vec의 모든 원소는 trim된 비공백 문자열이다.
    #[test]
    fn parse_comma_separated_preserves_nonblank_items(
        items in proptest::collection::vec("[a-zA-Z0-9_*%]{1,10}", 1..=5),
        fillers in proptest::collection::vec("[ \t]{0,5}", 0..=3),
    ) {
        // items + 공백 filler를 섞어 쉼표 구분 문자열을 만든다.
        let mut parts: Vec<String> = items.clone();
        parts.extend(fillers.iter().cloned());
        let input = parts.join(",");

        let result = parse_comma_separated(&input);
        prop_assert!(
            result.is_some(),
            "비공백 원소가 있으면 Some을 반환해야 함: 입력={:?}",
            input
        );
        let vec = result.unwrap();
        prop_assert!(!vec.is_empty(), "결과 Vec은 비어있지 않아야 함");
        for s in &vec {
            prop_assert!(!s.is_empty(), "모든 원소는 비어있지 않아야 함: {:?}", s);
            prop_assert_eq!(s.trim(), s.as_str(), "모든 원소는 trim 상태여야 함");
        }
        // items가 그대로 포함되어야 한다 (순서 보존).
        prop_assert_eq!(
            vec.iter().filter(|s| items.contains(s)).count(),
            items.len(),
            "원본 유효 원소가 모두 결과에 포함되어야 함"
        );
    }
}
