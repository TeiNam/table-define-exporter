//! Feature: code-quality-improvements, Property 10: 파셜 인덱스 predicate 파싱 —
//! `parse_pg_indexdef`가 `CREATE INDEX ... WHERE <pred>` 형태의 indexdef에서
//! `WHERE` 절을 올바르게 추출하고, `WHERE` 절이 없으면 `predicate`가 `None`임을
//! 임의의 predicate 표현식과 컬럼 이름에 대해 검증한다.
//!
//! 전략은 간단하게 유지하여 shrinking이 잘 동작하도록 한다.
//! 제어 문자·세미콜론·백슬래시 등 파서에 혼란을 줄 수 있는 문자는 배제하고,
//! 식별자/비교 연산자/공백/언더스코어만 허용하는 문자 집합으로 제약한다.
//!
//! Validates: Requirements 11.3

#![allow(clippy::needless_raw_string_hashes)]

use proptest::prelude::*;
use td_export::db::postgres::{parse_pg_indexdef, ParsedIndex};

// --- 회귀 예제 테스트 ---

/// WHERE 절이 없으면 predicate는 `None`이어야 한다.
#[test]
fn example_without_where_clause_returns_none() {
    let parsed = parse_pg_indexdef("CREATE INDEX i ON t (c)");
    assert_eq!(parsed.predicate, None);
}

/// 단순 WHERE 절은 원문 그대로 보존된다.
#[test]
fn example_simple_where_clause_is_extracted() {
    let parsed = parse_pg_indexdef("CREATE INDEX i ON t (c) WHERE deleted_at IS NULL");
    assert_eq!(parsed.predicate.as_deref(), Some("deleted_at IS NULL"));
}

/// 괄호로 묶인 predicate + UNIQUE 인덱스 조합도 올바르게 파싱된다.
#[test]
fn example_parenthesized_predicate_on_unique_index() {
    let parsed = parse_pg_indexdef("CREATE UNIQUE INDEX i ON t (c) WHERE (a > 0 AND b < 10)");
    let ParsedIndex {
        is_unique,
        columns,
        predicate,
    } = parsed;
    assert!(is_unique);
    assert_eq!(columns, "c");
    assert_eq!(predicate.as_deref(), Some("(a > 0 AND b < 10)"));
}

// --- 속성 기반 테스트 ---

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 10 (양성): 임의의 predicate 표현식 `pred`에 대해,
    /// `CREATE INDEX ... WHERE {pred}` 형태의 indexdef를 파싱한 결과의
    /// `predicate` 필드는 공백만 정규화된 `pred`와 동등하다.
    ///
    /// Validates: Requirements 11.3
    #[test]
    fn parse_extracts_predicate_for_any_simple_expression(
        pred in "[a-zA-Z][a-zA-Z0-9 _=<>!]{1,30}",
    ) {
        // 파서는 `trim()`만 수행하므로, 입력도 동일하게 정규화하여 비교한다.
        let pred_trimmed = pred.trim().to_string();
        prop_assume!(!pred_trimmed.is_empty());

        let indexdef = format!(
            "CREATE INDEX idx ON t USING btree (col) WHERE {pred_trimmed}"
        );
        let parsed = parse_pg_indexdef(&indexdef);

        prop_assert_eq!(parsed.predicate, Some(pred_trimmed));
    }

    /// Property 10 (음성): WHERE 절이 없는 indexdef는 항상
    /// `predicate: None`을 반환한다.
    ///
    /// Validates: Requirements 11.3
    #[test]
    fn parse_returns_none_predicate_when_where_is_absent(
        col in "[a-zA-Z][a-zA-Z0-9_]{0,20}",
    ) {
        let indexdef = format!("CREATE INDEX idx ON t USING btree ({col})");
        let parsed = parse_pg_indexdef(&indexdef);

        prop_assert_eq!(parsed.predicate, None);
    }
}
