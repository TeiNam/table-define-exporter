//! Feature: code-quality-improvements, Property 9: PG 배열 타입 파라미터 포맷 —
//! `build_pg_column_type`이 PostgreSQL 배열 타입(`_varchar`, `_bpchar`,
//! `_numeric`)에 대해 길이/정밀도/스케일 정보를 괄호로 보존하여
//! `"varchar(N)[]"`, `"bpchar(N)[]"`, `"numeric(P,S)[]"` 형식을 생성해야 함을
//! 임의의 값에 대해 검증한다.
//!
//! 설계 문서(Property 9) 및 현재 구현(`src/db/postgres/types.rs`)에 따르면
//! `_bpchar` 배열의 표시 이름은 `bpchar`로 유지된다. 비배열 `bpchar`는
//! `char(N)`으로 표시되지만, 배열 경로(`_bpchar`)는 길이를 반영할 때도
//! 원본 UDT 이름을 그대로 사용한다는 점에 주의한다.
//!
//! Validates: Requirements 11.1, 11.2

#![allow(clippy::needless_raw_string_hashes)]

use proptest::prelude::*;
use td_export::db::postgres::build_pg_column_type;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 9a: `_varchar` 배열 + character_maximum_length(N) →
    /// `"varchar(N)[]"`.
    ///
    /// Validates: Requirements 11.1
    #[test]
    fn varchar_array_with_length_formats_as_varchar_n_brackets(
        n in 1i32..=65535i32,
    ) {
        let actual = build_pg_column_type("_varchar", Some(n), None, None);
        let expected = format!("varchar({n})[]");
        prop_assert_eq!(actual, expected);
    }

    /// Property 9b: `_bpchar` 배열 + character_maximum_length(N) →
    /// `"bpchar(N)[]"`.
    ///
    /// 현재 구현은 배열 경로에서 UDT 기본 이름(`bpchar`)을 유지하며,
    /// 이는 design.md Property 9의 명세와 일치한다.
    ///
    /// Validates: Requirements 11.1
    #[test]
    fn bpchar_array_with_length_formats_as_bpchar_n_brackets(
        n in 1i32..=65535i32,
    ) {
        let actual = build_pg_column_type("_bpchar", Some(n), None, None);
        let expected = format!("bpchar({n})[]");
        prop_assert_eq!(actual, expected);
    }

    /// Property 9c: `_numeric` 배열 + precision(P) + scale(S) →
    /// `"numeric(P,S)[]"`.
    ///
    /// precision P와 scale S의 관계는 PostgreSQL 규칙상 `0 ≤ S ≤ P`를 만족한다
    /// (실제 메타데이터에서 전달되는 값의 범위).
    ///
    /// Validates: Requirements 11.2
    #[test]
    fn numeric_array_with_precision_and_scale_formats_correctly(
        p in 1i32..=1000i32,
        s in 0i32..=1000i32,
    ) {
        // scale이 precision을 초과하지 않도록 입력 공간을 제약한다.
        prop_assume!(s <= p);
        let actual = build_pg_column_type("_numeric", None, Some(p), Some(s));
        let expected = format!("numeric({p},{s})[]");
        prop_assert_eq!(actual, expected);
    }
}
