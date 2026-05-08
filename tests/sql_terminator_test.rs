// SQL Terminator 속성 테스트
//
// Feature: code-quality-improvements, Property 5: Terminator 단일 세미콜론 종결 —
// 임의의 DDL 문자열 `ddl`에 대해 `Terminator::apply(ddl)`의 결과는
// 정확히 하나의 `;` 또는 `);`로 끝나며, 입력이 이미 `;`/`);`로 끝나는 경우
// 결과의 `;` 개수는 입력과 동일하다 (이중 `;;` 없음).
// Validates: Requirements 2.2, 2.3
//
// `Terminator` enum 자체는 `pub(super)`로 캡슐화된 상태이므로,
// `td_export::export::apply_sql_terminator` 공개 래퍼를 통해 검증한다.
// 이 래퍼는 `config.db_type`에서 Terminator를 선택하여 `apply`를 호출하는
// 동일한 경로를 탄다(SqlExporter::write_tables와 동일한 흐름).

use proptest::prelude::*;
use td_export::export::apply_sql_terminator;
use td_export::model::DbType;

/// `trim_end()` 이후 공백/개행이 제거된 문자열의 말미가
/// 이미 종결자(`;` 또는 `);`)로 끝나는지 판정한다.
/// `Terminator::apply`의 내부 판정과 동일한 규칙을 반영한다.
fn ends_with_terminator(s: &str) -> bool {
    let trimmed = s.trim_end();
    trimmed.ends_with(';') || trimmed.ends_with(");")
}

/// 문자열 말미의 연속된 `;` 개수를 센다.
/// `Terminator`가 보장해야 하는 "정확히 하나의 세미콜론" 속성을 검증할 때
/// `;;`(이중)과 같은 퇴행을 잡아낸다.
fn trailing_semicolon_count(s: &str) -> usize {
    s.chars().rev().take_while(|c| *c == ';').count()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // Property 5 (5a): 임의의 DDL 입력에 대해 apply_sql_terminator의 결과는
    // 정확히 하나의 `;`(또는 `);`)로 끝난다.
    // - trim_end 이후 말미가 `;` 또는 `);`
    // - 말미 `;` 개수는 정확히 1 (이중 `;;` 없음)
    // MySQL / PostgreSQL 양쪽 모두 동일한 종결 규칙을 공유한다.
    #[test]
    fn apply_always_ends_with_single_terminator_mysql(
        ddl in ".*",
    ) {
        let out = apply_sql_terminator(&ddl, DbType::MySql);
        prop_assert!(
            ends_with_terminator(&out),
            "출력이 `;` 또는 `);`로 끝나지 않음: ddl={ddl:?}, out={out:?}",
        );
        prop_assert_eq!(
            trailing_semicolon_count(&out),
            1,
            "출력 말미의 `;` 개수가 1이 아님(이중 `;;` 가능성): out={:?}",
            out,
        );
    }

    #[test]
    fn apply_always_ends_with_single_terminator_postgres(
        ddl in ".*",
    ) {
        let out = apply_sql_terminator(&ddl, DbType::Postgres);
        prop_assert!(
            ends_with_terminator(&out),
            "출력이 `;` 또는 `);`로 끝나지 않음: ddl={ddl:?}, out={out:?}",
        );
        prop_assert_eq!(
            trailing_semicolon_count(&out),
            1,
            "출력 말미의 `;` 개수가 1이 아님(이중 `;;` 가능성): out={:?}",
            out,
        );
    }

    // Property 5 (5b): 입력이 이미 `;` 또는 `);`로 끝나는 경우,
    // 결과의 말미 `;` 개수는 입력(trim_end 기준)과 동일하다.
    // - 입력 trim_end 후 개수가 1이면 출력도 1 (이중 추가 없음)
    // - 즉, `Terminator::apply`는 이미 종결된 DDL에 세미콜론을 추가하지 않는다.
    #[test]
    fn apply_preserves_existing_terminator(
        // 임의의 전위 DDL에 결정적으로 `;` 또는 `);`를 붙여 입력을 구성
        prefix in ".*",
        already_paren in any::<bool>(),
    ) {
        let ddl = if already_paren {
            format!("{prefix});")
        } else {
            format!("{prefix};")
        };

        let out_mysql = apply_sql_terminator(&ddl, DbType::MySql);
        let out_pg = apply_sql_terminator(&ddl, DbType::Postgres);

        let input_trailing = trailing_semicolon_count(ddl.trim_end());
        prop_assert_eq!(
            trailing_semicolon_count(&out_mysql),
            input_trailing,
            "MySQL: 입력 말미 `;` 개수와 출력 `;` 개수가 다름: ddl={:?}, out={:?}",
            ddl, out_mysql,
        );
        prop_assert_eq!(
            trailing_semicolon_count(&out_pg),
            input_trailing,
            "PG: 입력 말미 `;` 개수와 출력 `;` 개수가 다름: ddl={:?}, out={:?}",
            ddl, out_pg,
        );
    }

    // Property 5 (5c): 입력이 종결자로 끝나지 않는 경우(trim_end 기준),
    // 출력은 `{ddl.trim_end()};` 형태로 정확히 하나의 새 `;`가 덧붙은 형태이다.
    // - 공백/개행은 입력 말미에서 제거되고, 본문은 유지된다.
    #[test]
    fn apply_appends_single_semicolon_when_missing(
        // `;`로 끝나지 않는 본문만 생성: trim_end 후 말미가 `;`나 `);`가 아니어야 함
        ddl in ".*".prop_filter(
            "trim_end 후 종결자로 끝나지 않는 입력만 유지",
            |s| !ends_with_terminator(s),
        ),
    ) {
        let expected = format!("{};", ddl.trim_end());

        let out_mysql = apply_sql_terminator(&ddl, DbType::MySql);
        let out_pg = apply_sql_terminator(&ddl, DbType::Postgres);

        prop_assert_eq!(
            &out_mysql,
            &expected,
            "MySQL: 종결자 없는 DDL에 정확히 하나의 `;`만 덧붙여야 함",
        );
        prop_assert_eq!(
            &out_pg,
            &expected,
            "PG: 종결자 없는 DDL에 정확히 하나의 `;`만 덧붙여야 함",
        );
    }
}
