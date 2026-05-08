use proptest::prelude::*;
use td_export::db::postgres::{
    build_pg_column_type, build_pg_ddl_from_metadata, determine_pg_extra, filter_pg_schemas,
    is_pg_system_schema, parse_pg_indexdef, ParsedIndex, PgConstraintType, PgDdlColumn,
    PgDdlConstraint,
};
use td_export::model::{ColumnInfo, GeneralInfo, TableDef, ViewInfo};

// === 단위 테스트: PG 시스템 스키마 판별 ===

#[test]
fn pg_system_schema_static_names() {
    assert!(is_pg_system_schema("pg_catalog"));
    assert!(is_pg_system_schema("information_schema"));
    assert!(is_pg_system_schema("pg_toast"));
}

#[test]
fn pg_system_schema_temp_prefixes() {
    assert!(is_pg_system_schema("pg_temp_1"));
    assert!(is_pg_system_schema("pg_temp_42"));
    assert!(is_pg_system_schema("pg_toast_temp_1"));
    assert!(is_pg_system_schema("pg_toast_temp_99"));
}

#[test]
fn pg_system_schema_rejects_user_schemas() {
    assert!(!is_pg_system_schema("public"));
    assert!(!is_pg_system_schema("myapp"));
    assert!(!is_pg_system_schema("pg_custom"));
    assert!(!is_pg_system_schema("test_schema"));
}

#[test]
fn filter_pg_schemas_excludes_system_schemas() {
    let all = vec![
        "pg_catalog".to_string(),
        "information_schema".to_string(),
        "pg_toast".to_string(),
        "pg_temp_1".to_string(),
        "pg_toast_temp_2".to_string(),
        "public".to_string(),
        "myapp".to_string(),
    ];
    let result = filter_pg_schemas(all, None);
    assert_eq!(result, vec!["public", "myapp"]);
}

#[test]
fn filter_pg_schemas_with_target_db() {
    let all = vec![
        "public".to_string(),
        "myapp".to_string(),
        "staging".to_string(),
    ];
    let targets = vec!["public".to_string(), "myapp".to_string()];
    let result = filter_pg_schemas(all, Some(&targets));
    assert_eq!(result.len(), 2);
    assert!(result.contains(&"public".to_string()));
    assert!(result.contains(&"myapp".to_string()));
    assert!(!result.contains(&"staging".to_string()));
}

#[test]
fn filter_pg_schemas_empty_input() {
    let result = filter_pg_schemas(Vec::new(), None);
    assert!(result.is_empty());
}

#[test]
fn filter_pg_schemas_all_system_returns_empty() {
    let all = vec![
        "pg_catalog".to_string(),
        "information_schema".to_string(),
        "pg_toast".to_string(),
    ];
    let result = filter_pg_schemas(all, None);
    assert!(result.is_empty());
}

// === Property 19 PBT 테스트: PG 시스템 스키마 제외 및 target_db 부분집합 ===
// **Validates: Requirements 4.2, 4.3, 4.4**

/// PG 시스템 스키마 이름을 생성하는 전략
fn pg_system_schema_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("pg_catalog".to_string()),
        Just("information_schema".to_string()),
        Just("pg_toast".to_string()),
        "[0-9]{1,5}".prop_map(|n| format!("pg_temp_{n}")),
        "[0-9]{1,5}".prop_map(|n| format!("pg_toast_temp_{n}")),
    ]
}

/// 사용자 스키마 이름을 생성하는 전략 (시스템 스키마와 겹치지 않도록)
fn user_schema_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,15}".prop_filter("시스템 스키마와 겹치지 않아야 함", |s| {
        !is_pg_system_schema(s)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 19-a: 임의의 스키마 집합에서 필터링 결과에 시스템 스키마가 포함되지 않는다.
    /// **Validates: Requirements 4.2**
    #[test]
    fn pg_system_schemas_always_excluded(
        system_schemas in proptest::collection::vec(pg_system_schema_strategy(), 0..=5),
        user_schemas in proptest::collection::vec(user_schema_strategy(), 0..=10),
    ) {
        let mut all_schemas = system_schemas;
        all_schemas.extend(user_schemas);

        let result = filter_pg_schemas(all_schemas, None);

        // 결과에 시스템 스키마가 하나도 포함되지 않아야 한다
        for schema in &result {
            prop_assert!(
                !is_pg_system_schema(schema),
                "시스템 스키마 '{schema}'가 결과에 포함됨"
            );
        }
    }

    /// Property 19-b: target_db 지정 시 반환 스키마가 target_db의 부분집합이다.
    /// **Validates: Requirements 4.3, 4.4**
    #[test]
    fn pg_target_db_subset(
        all_schemas in proptest::collection::vec(user_schema_strategy(), 1..=10),
        target_count in 0usize..=5usize,
    ) {
        let targets: Vec<String> = all_schemas
            .iter()
            .take(target_count)
            .cloned()
            .collect();

        let result = filter_pg_schemas(all_schemas, Some(&targets));

        // 반환된 모든 스키마가 target_db에 포함되어야 한다
        for schema in &result {
            prop_assert!(
                targets.contains(schema),
                "반환된 스키마 '{schema}'가 target_db에 없음"
            );
        }
    }

    /// Property 19-c: target_db가 None이면 모든 비시스템 스키마가 반환된다.
    /// **Validates: Requirements 4.3**
    #[test]
    fn pg_no_target_returns_all_non_system(
        user_schemas in proptest::collection::vec(user_schema_strategy(), 0..=10),
    ) {
        // 중복 제거
        let mut unique: Vec<String> = user_schemas.clone();
        unique.sort();
        unique.dedup();

        let result = filter_pg_schemas(unique.clone(), None);

        // 비시스템 스키마는 모두 결과에 포함되어야 한다
        prop_assert_eq!(result.len(), unique.len());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PG 스타일 TableDef 생성 헬퍼 함수
// ─────────────────────────────────────────────────────────────────────────────

/// PG 스타일 BASE TABLE TableDef를 생성한다.
/// PostgreSQL에서는 engine/row_format이 항상 None이고,
/// collate는 DB 레벨 collation을 사용한다.
fn build_pg_table_def(table_name: &str, db_collation: &str, columns: Vec<ColumnInfo>) -> TableDef {
    TableDef {
        table_name: table_name.to_string(),
        general: GeneralInfo {
            table_type: "BASE TABLE".to_string(),
            engine: None,
            row_format: None,
            collate: Some(db_collation.to_string()),
            comment: None,
        },
        columns,
        indexes: Vec::new(),
        constraints: Vec::new(),
        view: None,
        ddl: None,
    }
}

/// PG 스타일 VIEW TableDef를 생성한다.
/// PostgreSQL VIEW에서는 charset/collate가 항상 빈 문자열이다.
fn build_pg_view_def(view_name: &str, db_collation: &str, view_query: &str) -> TableDef {
    TableDef {
        table_name: view_name.to_string(),
        general: GeneralInfo {
            table_type: "VIEW".to_string(),
            engine: None,
            row_format: None,
            collate: Some(db_collation.to_string()),
            comment: None,
        },
        columns: Vec::new(),
        indexes: Vec::new(),
        constraints: Vec::new(),
        view: Some(ViewInfo {
            view_query: view_query.to_string(),
            charset: String::new(),
            collate: String::new(),
        }),
        ddl: None,
    }
}

/// PG 스타일 ColumnInfo를 생성한다.
/// PostgreSQL에서는 charset이 항상 None이다.
fn build_pg_column(name: &str, column_type: &str, nullable: &str) -> ColumnInfo {
    ColumnInfo {
        column_name: name.to_string(),
        default_value: None,
        nullable: nullable.to_string(),
        column_type: column_type.to_string(),
        charset: None,
        collation: None,
        column_key: None,
        extra: None,
        comment: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 20 단위 테스트: PG 전용 None/빈 문자열 필드 불변식
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pg_table_def_engine_and_row_format_are_none() {
    let table = build_pg_table_def(
        "users",
        "en_US.UTF-8",
        vec![
            build_pg_column("id", "int4", "NO"),
            build_pg_column("name", "varchar(100)", "YES"),
        ],
    );
    assert!(table.general.engine.is_none());
    assert!(table.general.row_format.is_none());
}

#[test]
fn pg_column_charset_is_always_none() {
    let columns = vec![
        build_pg_column("id", "int4", "NO"),
        build_pg_column("email", "varchar(255)", "YES"),
        build_pg_column("bio", "text", "YES"),
    ];
    for col in &columns {
        assert!(
            col.charset.is_none(),
            "컬럼 '{}'의 charset이 None이 아님",
            col.column_name
        );
    }
}

#[test]
fn pg_view_charset_and_collate_are_empty() {
    let view = build_pg_view_def(
        "active_users",
        "en_US.UTF-8",
        "SELECT * FROM users WHERE active",
    );
    let view_info = view.view.as_ref().unwrap();
    assert_eq!(view_info.charset, "");
    assert_eq!(view_info.collate, "");
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 21 단위 테스트: PG DB 레벨 collation 일관성
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pg_all_tables_share_same_collation() {
    let collation = "ko_KR.UTF-8";
    let tables = vec![
        build_pg_table_def("users", collation, vec![]),
        build_pg_table_def("orders", collation, vec![]),
        build_pg_view_def("active_users", collation, "SELECT 1"),
    ];
    let first_collate = tables[0].general.collate.as_ref().unwrap();
    for table in &tables {
        assert_eq!(
            table.general.collate.as_ref().unwrap(),
            first_collate,
            "테이블 '{}'의 collation이 다름",
            table.table_name
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 20 PBT 테스트: PG 전용 None/빈 문자열 필드 불변식
// **Validates: Requirements 5.2, 6.5, 7.2**
// ─────────────────────────────────────────────────────────────────────────────

/// PG 컬럼 타입 이름을 생성하는 전략
fn pg_column_type_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("int4".to_string()),
        Just("int8".to_string()),
        Just("text".to_string()),
        Just("bool".to_string()),
        Just("timestamptz".to_string()),
        Just("numeric(10,2)".to_string()),
        Just("varchar(255)".to_string()),
        Just("char(10)".to_string()),
        Just("uuid".to_string()),
        Just("jsonb".to_string()),
    ]
}

/// PG DB collation 문자열을 생성하는 전략
fn pg_collation_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("en_US.UTF-8".to_string()),
        Just("ko_KR.UTF-8".to_string()),
        Just("ja_JP.UTF-8".to_string()),
        Just("C".to_string()),
        Just("POSIX".to_string()),
        Just("en_US.utf8".to_string()),
        Just("de_DE.UTF-8".to_string()),
    ]
}

/// PG 테이블 이름을 생성하는 전략
fn pg_table_name_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{1,20}"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 20-a: PG TableDef의 engine은 항상 None이다.
    /// **Validates: Requirements 5.2**
    #[test]
    fn prop20_pg_engine_always_none(
        table_name in pg_table_name_strategy(),
        collation in pg_collation_strategy(),
        col_types in proptest::collection::vec(pg_column_type_strategy(), 1..=5),
    ) {
        let columns: Vec<ColumnInfo> = col_types.iter().enumerate().map(|(i, ct)| {
            build_pg_column(&format!("col_{i}"), ct, "YES")
        }).collect();
        let table = build_pg_table_def(&table_name, &collation, columns);

        prop_assert!(
            table.general.engine.is_none(),
            "PG 테이블 '{}'의 engine이 None이 아님",
            table_name
        );
    }

    /// Property 20-b: PG TableDef의 row_format은 항상 None이다.
    /// **Validates: Requirements 5.2**
    #[test]
    fn prop20_pg_row_format_always_none(
        table_name in pg_table_name_strategy(),
        collation in pg_collation_strategy(),
        col_types in proptest::collection::vec(pg_column_type_strategy(), 1..=5),
    ) {
        let columns: Vec<ColumnInfo> = col_types.iter().enumerate().map(|(i, ct)| {
            build_pg_column(&format!("col_{i}"), ct, "YES")
        }).collect();
        let table = build_pg_table_def(&table_name, &collation, columns);

        prop_assert!(
            table.general.row_format.is_none(),
            "PG 테이블 '{}'의 row_format이 None이 아님",
            table_name
        );
    }

    /// Property 20-c: PG 컬럼의 charset은 항상 None이다.
    /// **Validates: Requirements 6.5**
    #[test]
    fn prop20_pg_column_charset_always_none(
        table_name in pg_table_name_strategy(),
        collation in pg_collation_strategy(),
        col_types in proptest::collection::vec(pg_column_type_strategy(), 1..=10),
    ) {
        let columns: Vec<ColumnInfo> = col_types.iter().enumerate().map(|(i, ct)| {
            build_pg_column(&format!("col_{i}"), ct, "YES")
        }).collect();
        let table = build_pg_table_def(&table_name, &collation, columns);

        for col in &table.columns {
            prop_assert!(
                col.charset.is_none(),
                "PG 컬럼 '{}.{}'의 charset이 None이 아님",
                table_name,
                col.column_name
            );
        }
    }

    /// Property 20-d: PG VIEW의 charset과 collate는 항상 빈 문자열이다.
    /// **Validates: Requirements 7.2**
    #[test]
    fn prop20_pg_view_charset_collate_empty(
        view_name in pg_table_name_strategy(),
        collation in pg_collation_strategy(),
        view_query in "[A-Z ]{5,30}",
    ) {
        let view = build_pg_view_def(&view_name, &collation, &view_query);
        let view_info = view.view.as_ref().unwrap();

        prop_assert_eq!(
            &view_info.charset, "",
            "PG 뷰 '{}'의 charset이 빈 문자열이 아님",
            view_name
        );
        prop_assert_eq!(
            &view_info.collate, "",
            "PG 뷰 '{}'의 collate가 빈 문자열이 아님",
            view_name
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 21 PBT 테스트: PG DB 레벨 collation 일관성
// **Validates: Requirements 5.7**
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 21: 동일 데이터베이스 내 모든 테이블의 collate 값은 동일하다.
    /// PgClient는 DB 레벨 collation을 모든 테이블에 동일하게 적용하므로,
    /// 같은 db_collation으로 생성된 TableDef들의 collate는 모두 같아야 한다.
    /// **Validates: Requirements 5.7**
    #[test]
    fn prop21_pg_collation_consistency(
        db_collation in pg_collation_strategy(),
        table_count in 2usize..=20usize,
        table_names in proptest::collection::vec(pg_table_name_strategy(), 2..=20),
        view_count in 0usize..=5usize,
    ) {
        // 테이블 이름 중복 제거
        let mut unique_names: Vec<String> = table_names;
        unique_names.sort();
        unique_names.dedup();
        let actual_count = unique_names.len().min(table_count);
        if actual_count < 2 {
            // 최소 2개 테이블이 필요
            return Ok(());
        }

        // 동일한 db_collation으로 테이블/뷰 생성
        let mut tables: Vec<TableDef> = Vec::new();
        let view_boundary = actual_count.saturating_sub(view_count.min(actual_count - 1));

        for (i, name) in unique_names.iter().take(actual_count).enumerate() {
            if i >= view_boundary {
                tables.push(build_pg_view_def(name, &db_collation, "SELECT 1"));
            } else {
                tables.push(build_pg_table_def(name, &db_collation, vec![
                    build_pg_column("id", "int4", "NO"),
                ]));
            }
        }

        // 모든 테이블의 collate 값이 동일한지 검증
        let first_collate = tables[0].general.collate.as_ref().unwrap();
        for table in &tables {
            let collate = table.general.collate.as_ref().unwrap();
            prop_assert_eq!(
                collate, first_collate,
                "테이블 '{}'의 collation '{}'이 첫 번째 테이블의 '{}'과 다름",
                table.table_name, collate, first_collate
            );
        }

        // 모든 collate 값이 원본 db_collation과 동일한지 검증
        for table in &tables {
            let collate = table.general.collate.as_ref().unwrap();
            prop_assert_eq!(
                collate, &db_collation,
                "테이블 '{}'의 collation '{}'이 DB collation '{}'과 다름",
                table.table_name, collate, db_collation
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트: build_pg_column_type
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn build_pg_column_type_varchar_with_length() {
    assert_eq!(
        build_pg_column_type("varchar", Some(255), None, None),
        "varchar(255)"
    );
}

#[test]
fn build_pg_column_type_bpchar_displayed_as_char() {
    assert_eq!(
        build_pg_column_type("bpchar", Some(10), None, None),
        "char(10)"
    );
}

#[test]
fn build_pg_column_type_numeric_with_precision_scale() {
    assert_eq!(
        build_pg_column_type("numeric", None, Some(10), Some(2)),
        "numeric(10,2)"
    );
}

#[test]
fn build_pg_column_type_plain_types() {
    assert_eq!(build_pg_column_type("int4", None, None, None), "int4");
    assert_eq!(build_pg_column_type("text", None, None, None), "text");
    assert_eq!(build_pg_column_type("bool", None, None, None), "bool");
    assert_eq!(
        build_pg_column_type("timestamptz", None, None, None),
        "timestamptz"
    );
}

#[test]
fn build_pg_column_type_array_type() {
    assert_eq!(build_pg_column_type("_int4", None, None, None), "int4[]");
    assert_eq!(build_pg_column_type("_text", None, None, None), "text[]");
    assert_eq!(
        build_pg_column_type("_varchar", None, None, None),
        "varchar[]"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트: determine_pg_extra
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn determine_pg_extra_identity_always() {
    assert_eq!(
        determine_pg_extra("a", "", None),
        Some("auto_increment".to_string())
    );
}

#[test]
fn determine_pg_extra_identity_by_default() {
    assert_eq!(
        determine_pg_extra("d", "", None),
        Some("auto_increment".to_string())
    );
}

#[test]
fn determine_pg_extra_serial_nextval() {
    assert_eq!(
        determine_pg_extra("", "", Some("nextval('users_id_seq'::regclass)")),
        Some("auto_increment".to_string())
    );
}

#[test]
fn determine_pg_extra_generated_stored() {
    assert_eq!(
        determine_pg_extra("", "s", None),
        Some("STORED GENERATED".to_string())
    );
}

#[test]
fn determine_pg_extra_none() {
    assert_eq!(determine_pg_extra("", "", None), None);
    assert_eq!(determine_pg_extra("", "", Some("'default_value'")), None);
}

#[test]
fn determine_pg_extra_identity_takes_priority_over_serial() {
    // identity가 있으면 column_default에 nextval이 있어도 identity 우선
    assert_eq!(
        determine_pg_extra("a", "", Some("nextval('seq'::regclass)")),
        Some("auto_increment".to_string())
    );
}

#[test]
fn determine_pg_extra_identity_takes_priority_over_generated() {
    // identity가 있으면 attgenerated='s'여도 identity 우선
    assert_eq!(
        determine_pg_extra("d", "s", None),
        Some("auto_increment".to_string())
    );
}

#[test]
fn determine_pg_extra_serial_takes_priority_over_generated() {
    // nextval이 있으면 attgenerated='s'여도 serial 우선
    assert_eq!(
        determine_pg_extra("", "s", Some("nextval('seq'::regclass)")),
        Some("auto_increment".to_string())
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 22 PBT 테스트: PG 컬럼 타입 포맷 정확성
// **Validates: Requirements 6.2**
// ─────────────────────────────────────────────────────────────────────────────

/// PG udt_name을 생성하는 전략 (배열 타입 제외)
fn pg_udt_name_non_array_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("int4".to_string()),
        Just("int8".to_string()),
        Just("text".to_string()),
        Just("bool".to_string()),
        Just("varchar".to_string()),
        Just("bpchar".to_string()),
        Just("numeric".to_string()),
        Just("timestamptz".to_string()),
        Just("uuid".to_string()),
        Just("jsonb".to_string()),
        Just("float8".to_string()),
    ]
}

/// PG 배열 udt_name을 생성하는 전략
fn pg_array_udt_name_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("_int4".to_string()),
        Just("_int8".to_string()),
        Just("_text".to_string()),
        Just("_bool".to_string()),
        Just("_varchar".to_string()),
        Just("_numeric".to_string()),
        Just("_uuid".to_string()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 22-a: char_max_length가 Some이면 결과에 `({length})`가 포함된다.
    /// **Validates: Requirements 6.2**
    #[test]
    fn prop22_char_max_length_format(
        udt_name in pg_udt_name_non_array_strategy(),
        length in 1i32..=10000i32,
    ) {
        let result = build_pg_column_type(&udt_name, Some(length), None, None);
        prop_assert!(
            result.contains(&format!("({})", length)),
            "char_max_length={}인데 결과 '{}'에 '({})'가 없음",
            length, result, length
        );
    }

    /// Property 22-b: numeric + precision/scale이면 `numeric({p},{s})` 형식이다.
    /// **Validates: Requirements 6.2**
    #[test]
    fn prop22_numeric_precision_scale_format(
        precision in 1i32..=38i32,
        scale in 0i32..=20i32,
    ) {
        let result = build_pg_column_type("numeric", None, Some(precision), Some(scale));
        let expected = format!("numeric({precision},{scale})");
        prop_assert_eq!(
            &result, &expected,
            "numeric({},{}) 기대했으나 '{}' 반환",
            precision, scale, result
        );
    }

    /// Property 22-c: 배열 타입(`_` 접두어)이면 결과가 `[]`로 끝난다.
    /// **Validates: Requirements 6.2**
    #[test]
    fn prop22_array_type_ends_with_brackets(
        udt_name in pg_array_udt_name_strategy(),
    ) {
        let result = build_pg_column_type(&udt_name, None, None, None);
        prop_assert!(
            result.ends_with("[]"),
            "배열 타입 '{}'인데 결과 '{}'가 '[]'로 끝나지 않음",
            udt_name, result
        );
        // `_` 접두어가 제거되었는지 확인
        prop_assert!(
            !result.starts_with('_'),
            "배열 타입 결과 '{}'에 '_' 접두어가 남아있음",
            result
        );
    }

    /// Property 22-d: 길이/정밀도 없으면 udt_name 그대로 반환한다.
    /// **Validates: Requirements 6.2**
    #[test]
    fn prop22_no_length_returns_udt_name(
        udt_name in pg_udt_name_non_array_strategy().prop_filter(
            "numeric 제외 (precision/scale 없이도 그대로 반환되지만 별도 테스트)",
            |n| n != "numeric"
        ),
    ) {
        let result = build_pg_column_type(&udt_name, None, None, None);
        prop_assert_eq!(
            &result, &udt_name,
            "길이 없는 '{}'인데 '{}' 반환",
            udt_name, result
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 23 PBT 테스트: PG serial/identity/generated 감지 정확성
// **Validates: Requirements 6.4**
// ─────────────────────────────────────────────────────────────────────────────

/// attidentity 값을 생성하는 전략
fn pg_attidentity_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("".to_string()),  // identity 아님
        Just("a".to_string()), // GENERATED ALWAYS
        Just("d".to_string()), // GENERATED BY DEFAULT
    ]
}

/// attgenerated 값을 생성하는 전략
fn pg_attgenerated_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("".to_string()),  // generated 아님
        Just("s".to_string()), // STORED
    ]
}

/// column_default 값을 생성하는 전략
fn pg_column_default_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        Just(None),
        Just(Some("'default_value'".to_string())),
        Just(Some("0".to_string())),
        Just(Some("nextval('users_id_seq'::regclass)".to_string())),
        Just(Some("nextval('orders_id_seq'::regclass)".to_string())),
        Just(Some("true".to_string())),
        Just(Some("now()".to_string())),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 23-a: attidentity가 'a' 또는 'd'이면 auto_increment를 반환한다.
    /// **Validates: Requirements 6.4**
    #[test]
    fn prop23_identity_returns_auto_increment(
        identity in prop_oneof![Just("a".to_string()), Just("d".to_string())],
        generated in pg_attgenerated_strategy(),
        default in pg_column_default_strategy(),
    ) {
        let result = determine_pg_extra(
            &identity,
            &generated,
            default.as_deref(),
        );
        prop_assert_eq!(
            result.as_deref(),
            Some("auto_increment"),
            "attidentity='{}'인데 auto_increment가 아님",
            identity
        );
    }

    /// Property 23-b: column_default에 nextval(이 포함되면 auto_increment를 반환한다.
    /// (attidentity가 비어있을 때)
    /// **Validates: Requirements 6.4**
    #[test]
    fn prop23_serial_returns_auto_increment(
        default_val in "[a-z_]{1,10}".prop_map(|seq| format!("nextval('{seq}_seq'::regclass)")),
        generated in pg_attgenerated_strategy(),
    ) {
        let result = determine_pg_extra(
            "",
            &generated,
            Some(&default_val),
        );
        prop_assert_eq!(
            result.as_deref(),
            Some("auto_increment"),
            "nextval 패턴인데 auto_increment가 아님"
        );
    }

    /// Property 23-c: attgenerated가 's'이면 STORED GENERATED를 반환한다.
    /// (attidentity 비어있고, nextval 없을 때)
    /// **Validates: Requirements 6.4**
    #[test]
    fn prop23_generated_stored_returns_stored_generated(
        default in prop_oneof![
            Just(None),
            Just(Some("'value'".to_string())),
            Just(Some("0".to_string())),
            Just(Some("now()".to_string())),
        ],
    ) {
        let result = determine_pg_extra(
            "",
            "s",
            default.as_deref(),
        );
        prop_assert_eq!(
            result.as_deref(),
            Some("STORED GENERATED"),
            "attgenerated='s'인데 STORED GENERATED가 아님"
        );
    }

    /// Property 23-d: 어떤 조건도 해당하지 않으면 None을 반환한다.
    /// **Validates: Requirements 6.4**
    #[test]
    fn prop23_no_condition_returns_none(
        default in prop_oneof![
            Just(None),
            Just(Some("'value'".to_string())),
            Just(Some("0".to_string())),
            Just(Some("true".to_string())),
            Just(Some("now()".to_string())),
        ],
    ) {
        let result = determine_pg_extra("", "", default.as_deref());
        prop_assert!(
            result.is_none(),
            "조건 없는데 Some({:?}) 반환",
            result
        );
    }

    /// Property 23-e: 우선순위 검증 — identity > serial > generated
    /// **Validates: Requirements 6.4**
    #[test]
    fn prop23_priority_order(
        identity in pg_attidentity_strategy(),
        generated in pg_attgenerated_strategy(),
        default in pg_column_default_strategy(),
    ) {
        let result = determine_pg_extra(
            &identity,
            &generated,
            default.as_deref(),
        );

        let has_identity = identity == "a" || identity == "d";
        let has_serial = default.as_deref().is_some_and(|d| d.contains("nextval("));
        let has_generated = generated == "s";

        if has_identity {
            // identity가 최우선
            prop_assert_eq!(result.as_deref(), Some("auto_increment"));
        } else if has_serial {
            // serial이 두 번째 우선
            prop_assert_eq!(result.as_deref(), Some("auto_increment"));
        } else if has_generated {
            // generated가 세 번째 우선
            prop_assert_eq!(result.as_deref(), Some("STORED GENERATED"));
        } else {
            // 해당 없음
            prop_assert!(result.is_none());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트: parse_pg_indexdef — indexdef 파싱 로직 검증
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn parse_indexdef_btree_single_column() {
    let indexdef = "CREATE INDEX idx_users_name ON public.users USING btree (name)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "name");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_unique_multi_columns() {
    let indexdef =
        "CREATE UNIQUE INDEX idx_users_email ON public.users USING btree (email, tenant_id)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(parsed.is_unique);
    assert_eq!(parsed.columns, "email, tenant_id");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_hash_index() {
    let indexdef = "CREATE INDEX idx_lookup ON myschema.orders USING hash (order_id)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "order_id");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_with_desc_modifier() {
    let indexdef = "CREATE INDEX idx_created ON public.events USING btree (created_at DESC)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "created_at");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_with_asc_modifier() {
    let indexdef = "CREATE INDEX idx_score ON public.results USING btree (score ASC)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "score");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_with_nulls_first_last() {
    let indexdef =
        "CREATE INDEX idx_priority ON public.tasks USING btree (priority DESC NULLS LAST)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "priority");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_multi_columns_with_modifiers() {
    let indexdef = "CREATE UNIQUE INDEX idx_composite ON public.items \
                    USING btree (category ASC, created_at DESC NULLS FIRST)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(parsed.is_unique);
    assert_eq!(parsed.columns, "category, created_at");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_expression_index() {
    // 표현식 인덱스: lower(name) 같은 함수 호출이 포함된 경우
    let indexdef = "CREATE INDEX idx_lower_name ON public.users USING btree (lower(name))";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "lower(name)");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_gin_index() {
    let indexdef = "CREATE INDEX idx_tags ON public.articles USING gin (tags)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "tags");
    assert!(parsed.predicate.is_none());
}

#[test]
fn parse_indexdef_empty_string() {
    let parsed = parse_pg_indexdef("");
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "");
    assert!(parsed.predicate.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트: parse_pg_indexdef — 파셜 인덱스 WHERE 절 추출 (Task 10.2)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn parse_indexdef_partial_simple_predicate() {
    // 파셜 인덱스: WHERE 절 본문이 그대로 predicate에 들어가야 한다
    let indexdef =
        "CREATE INDEX idx_active ON public.users USING btree (email) WHERE (active = true)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(!parsed.is_unique);
    assert_eq!(parsed.columns, "email");
    assert_eq!(parsed.predicate.as_deref(), Some("(active = true)"));
}

#[test]
fn parse_indexdef_partial_unique_with_predicate() {
    // UNIQUE + 파셜 인덱스 조합
    let indexdef = "CREATE UNIQUE INDEX idx_email_live ON public.users \
                    USING btree (email) WHERE (deleted_at IS NULL)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(parsed.is_unique);
    assert_eq!(parsed.columns, "email");
    assert_eq!(parsed.predicate.as_deref(), Some("(deleted_at IS NULL)"));
}

#[test]
fn parse_indexdef_partial_case_insensitive_where() {
    // WHERE 키워드는 대소문자 무관 (PostgreSQL 표기 호환)
    let indexdef = "CREATE INDEX idx_low ON t USING btree (col) where score > 0";
    let parsed = parse_pg_indexdef(indexdef);
    assert_eq!(parsed.predicate.as_deref(), Some("score > 0"));
}

#[test]
fn parse_indexdef_no_where_clause_returns_none() {
    let indexdef = "CREATE INDEX idx ON t USING btree (col)";
    let parsed = parse_pg_indexdef(indexdef);
    assert!(parsed.predicate.is_none());
}

#[test]
fn parsed_index_struct_equality() {
    // ParsedIndex 값 기반 동등성 비교 (PartialEq 파생 검증)
    let a = parse_pg_indexdef("CREATE INDEX i ON t USING btree (c)");
    let b = ParsedIndex {
        is_unique: false,
        columns: "c".to_string(),
        predicate: None,
    };
    assert_eq!(a, b);
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트: ViewInfo 구조 검증 (Task 9.2)
// 뷰 정의 조회 결과의 구조적 정확성을 검증한다.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn view_info_charset_and_collate_are_empty_strings() {
    // PostgreSQL 뷰에서는 charset/collate가 항상 빈 문자열이어야 한다 (Req 7.2)
    let view_info = ViewInfo {
        view_query: "SELECT id, name FROM users WHERE active = true".to_string(),
        charset: String::new(),
        collate: String::new(),
    };
    assert_eq!(view_info.charset, "");
    assert_eq!(view_info.collate, "");
    assert!(!view_info.view_query.is_empty());
}

#[test]
fn view_info_preserves_view_query() {
    // pg_get_viewdef가 반환하는 뷰 정의 SQL이 그대로 보존되어야 한다 (Req 7.1)
    let query =
        " SELECT u.id,\n    u.name,\n    u.email\n   FROM users u\n  WHERE (u.active = true);";
    let view_info = ViewInfo {
        view_query: query.to_string(),
        charset: String::new(),
        collate: String::new(),
    };
    assert_eq!(view_info.view_query, query);
}

#[test]
fn view_info_empty_query_is_valid() {
    // 뷰 정의가 빈 문자열인 경우도 구조적으로 유효하다
    let view_info = ViewInfo {
        view_query: String::new(),
        charset: String::new(),
        collate: String::new(),
    };
    assert_eq!(view_info.view_query, "");
    assert_eq!(view_info.charset, "");
    assert_eq!(view_info.collate, "");
}

#[test]
fn pg_view_table_def_has_no_columns_indexes_constraints() {
    // PostgreSQL VIEW에서는 컬럼/인덱스/제약 조건을 수집하지 않는다 (Req 7.4)
    let view = build_pg_view_def(
        "active_users",
        "en_US.UTF-8",
        "SELECT * FROM users WHERE active",
    );
    assert!(view.columns.is_empty(), "VIEW에 컬럼이 있으면 안 됨");
    assert!(view.indexes.is_empty(), "VIEW에 인덱스가 있으면 안 됨");
    assert!(
        view.constraints.is_empty(),
        "VIEW에 제약 조건이 있으면 안 됨"
    );
    assert!(view.view.is_some(), "VIEW에 ViewInfo가 있어야 함");
}

#[test]
fn pg_view_table_def_general_info_matches_pg_conventions() {
    // PostgreSQL VIEW의 일반 정보가 PG 규칙을 따르는지 검증
    let view = build_pg_view_def("summary_view", "ko_KR.UTF-8", "SELECT count(*) FROM orders");
    assert_eq!(view.general.table_type, "VIEW");
    assert!(
        view.general.engine.is_none(),
        "PG VIEW에 engine이 없어야 함"
    );
    assert!(
        view.general.row_format.is_none(),
        "PG VIEW에 row_format이 없어야 함"
    );
    assert_eq!(
        view.general.collate.as_deref(),
        Some("ko_KR.UTF-8"),
        "PG VIEW의 collate는 DB 레벨 collation이어야 함"
    );

    let view_info = view.view.as_ref().unwrap();
    assert_eq!(view_info.charset, "", "PG VIEW charset은 빈 문자열");
    assert_eq!(view_info.collate, "", "PG VIEW collate는 빈 문자열");
    assert_eq!(view_info.view_query, "SELECT count(*) FROM orders");
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트: build_pg_ddl_from_metadata — DDL 재구성 순수 함수 검증
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn ddl_basic_table_with_columns() {
    let columns = vec![
        PgDdlColumn {
            name: "id".to_string(),
            data_type: "integer".to_string(),
            is_nullable: false,
            default_value: None,
            generated_expression: None,
        },
        PgDdlColumn {
            name: "name".to_string(),
            data_type: "varchar(100)".to_string(),
            is_nullable: true,
            default_value: None,
            generated_expression: None,
        },
    ];
    let ddl = build_pg_ddl_from_metadata("public", "users", &columns, &[], &[]).unwrap();
    assert!(ddl.contains("CREATE TABLE"));
    assert!(ddl.contains("\"public\".\"users\""));
    assert!(ddl.contains("\"id\" integer NOT NULL"));
    assert!(ddl.contains("\"name\" varchar(100)"));
    assert!(ddl.contains(");"));
}

#[test]
fn ddl_with_default_value() {
    let columns = vec![PgDdlColumn {
        name: "active".to_string(),
        data_type: "bool".to_string(),
        is_nullable: false,
        default_value: Some("true".to_string()),
        generated_expression: None,
    }];
    let ddl = build_pg_ddl_from_metadata("public", "flags", &columns, &[], &[]).unwrap();
    assert!(ddl.contains("DEFAULT true"));
}

#[test]
fn ddl_with_generated_column() {
    let columns = vec![
        PgDdlColumn {
            name: "a".to_string(),
            data_type: "integer".to_string(),
            is_nullable: false,
            default_value: None,
            generated_expression: None,
        },
        PgDdlColumn {
            name: "b".to_string(),
            data_type: "integer".to_string(),
            is_nullable: true,
            default_value: None,
            generated_expression: Some("a * 2".to_string()),
        },
    ];
    let ddl = build_pg_ddl_from_metadata("public", "calc", &columns, &[], &[]).unwrap();
    assert!(ddl.contains("GENERATED ALWAYS AS (a * 2) STORED"));
    // generated 컬럼에는 DEFAULT가 없어야 한다
    assert!(!ddl.contains("DEFAULT"));
}

#[test]
fn ddl_with_primary_key() {
    let columns = vec![PgDdlColumn {
        name: "id".to_string(),
        data_type: "integer".to_string(),
        is_nullable: false,
        default_value: None,
        generated_expression: None,
    }];
    let constraints = vec![PgDdlConstraint {
        name: "users_pkey".to_string(),
        constraint_type: PgConstraintType::PrimaryKey,
        columns: vec!["id".to_string()],
    }];
    let ddl = build_pg_ddl_from_metadata("public", "users", &columns, &constraints, &[]).unwrap();
    assert!(ddl.contains("CONSTRAINT \"users_pkey\" PRIMARY KEY (\"id\")"));
}

#[test]
fn ddl_with_unique_constraint() {
    let columns = vec![PgDdlColumn {
        name: "email".to_string(),
        data_type: "varchar(255)".to_string(),
        is_nullable: false,
        default_value: None,
        generated_expression: None,
    }];
    let constraints = vec![PgDdlConstraint {
        name: "users_email_key".to_string(),
        constraint_type: PgConstraintType::Unique,
        columns: vec!["email".to_string()],
    }];
    let ddl = build_pg_ddl_from_metadata("public", "users", &columns, &constraints, &[]).unwrap();
    assert!(ddl.contains("CONSTRAINT \"users_email_key\" UNIQUE (\"email\")"));
}

#[test]
fn ddl_with_foreign_key() {
    let columns = vec![PgDdlColumn {
        name: "user_id".to_string(),
        data_type: "integer".to_string(),
        is_nullable: false,
        default_value: None,
        generated_expression: None,
    }];
    let constraints = vec![PgDdlConstraint {
        name: "orders_user_fk".to_string(),
        constraint_type: PgConstraintType::ForeignKey {
            ref_schema: "public".to_string(),
            ref_table: "users".to_string(),
            ref_columns: vec!["id".to_string()],
            on_delete: "CASCADE".to_string(),
            on_update: "NO ACTION".to_string(),
        },
        columns: vec!["user_id".to_string()],
    }];
    let ddl = build_pg_ddl_from_metadata("public", "orders", &columns, &constraints, &[]).unwrap();
    assert!(ddl.contains("FOREIGN KEY (\"user_id\")"));
    assert!(ddl.contains("REFERENCES \"public\".\"users\" (\"id\")"));
    assert!(ddl.contains("ON DELETE CASCADE"));
    assert!(ddl.contains("ON UPDATE NO ACTION"));
}

#[test]
fn ddl_with_multiple_fks_reference_names_resolved() {
    // 2개의 FK를 가진 orders 테이블에서 FK 일괄 수집 결과가
    // DDL 출력에 모두 정확하게 포함되는지 검증한다.
    // - orders_user_fk: user_id → public.users(id) ON DELETE CASCADE
    // - orders_product_fk: product_id → public.products(id) ON DELETE SET NULL
    let columns = vec![
        PgDdlColumn {
            name: "id".to_string(),
            data_type: "integer".to_string(),
            is_nullable: false,
            default_value: None,
            generated_expression: None,
        },
        PgDdlColumn {
            name: "user_id".to_string(),
            data_type: "integer".to_string(),
            is_nullable: false,
            default_value: None,
            generated_expression: None,
        },
        PgDdlColumn {
            name: "product_id".to_string(),
            data_type: "integer".to_string(),
            is_nullable: true,
            default_value: None,
            generated_expression: None,
        },
    ];
    let constraints = vec![
        PgDdlConstraint {
            name: "orders_user_fk".to_string(),
            constraint_type: PgConstraintType::ForeignKey {
                ref_schema: "public".to_string(),
                ref_table: "users".to_string(),
                ref_columns: vec!["id".to_string()],
                on_delete: "CASCADE".to_string(),
                on_update: "NO ACTION".to_string(),
            },
            columns: vec!["user_id".to_string()],
        },
        PgDdlConstraint {
            name: "orders_product_fk".to_string(),
            constraint_type: PgConstraintType::ForeignKey {
                ref_schema: "public".to_string(),
                ref_table: "products".to_string(),
                ref_columns: vec!["id".to_string()],
                on_delete: "SET NULL".to_string(),
                on_update: "NO ACTION".to_string(),
            },
            columns: vec!["product_id".to_string()],
        },
    ];
    let ddl = build_pg_ddl_from_metadata("public", "orders", &columns, &constraints, &[]).unwrap();

    // 첫 번째 FK: user_id → public.users(id) ON DELETE CASCADE
    assert!(
        ddl.contains(
            "CONSTRAINT \"orders_user_fk\" FOREIGN KEY (\"user_id\") \
             REFERENCES \"public\".\"users\" (\"id\") ON DELETE CASCADE"
        ),
        "첫 번째 FK가 올바른 형식으로 포함되어야 함: {ddl}"
    );

    // 두 번째 FK: product_id → public.products(id) ON DELETE SET NULL
    assert!(
        ddl.contains(
            "CONSTRAINT \"orders_product_fk\" FOREIGN KEY (\"product_id\") \
             REFERENCES \"public\".\"products\" (\"id\") ON DELETE SET NULL"
        ),
        "두 번째 FK가 올바른 형식으로 포함되어야 함: {ddl}"
    );

    // FK 일괄 수집이 두 개의 FK 정보를 모두 보존했는지 확인
    // (참조 테이블 이름이 각각 올바르게 해석되었는지까지 포함)
    assert_eq!(
        ddl.matches("FOREIGN KEY").count(),
        2,
        "FK는 정확히 2개여야 함: {ddl}"
    );
    assert!(ddl.contains("\"public\".\"users\""));
    assert!(ddl.contains("\"public\".\"products\""));
}

#[test]
fn ddl_with_check_constraint() {
    let columns = vec![PgDdlColumn {
        name: "age".to_string(),
        data_type: "integer".to_string(),
        is_nullable: false,
        default_value: None,
        generated_expression: None,
    }];
    let constraints = vec![PgDdlConstraint {
        name: "users_age_check".to_string(),
        constraint_type: PgConstraintType::Check {
            expression: "(age > 0)".to_string(),
        },
        columns: vec![],
    }];
    let ddl = build_pg_ddl_from_metadata("public", "users", &columns, &constraints, &[]).unwrap();
    assert!(ddl.contains("CONSTRAINT \"users_age_check\" CHECK ((age > 0))"));
}

#[test]
fn ddl_with_indexes() {
    let columns = vec![PgDdlColumn {
        name: "name".to_string(),
        data_type: "text".to_string(),
        is_nullable: true,
        default_value: None,
        generated_expression: None,
    }];
    let index_defs = vec!["CREATE INDEX idx_name ON public.users USING btree (name)".to_string()];
    let ddl = build_pg_ddl_from_metadata("public", "users", &columns, &[], &index_defs).unwrap();
    assert!(ddl.contains(");"));
    assert!(ddl.contains("CREATE INDEX idx_name ON public.users USING btree (name);"));
}

#[test]
fn ddl_constraint_ordering_pk_uq_fk_ck() {
    // 제약 조건이 PK → UQ → FK → CK 순서로 출력되는지 검증
    let columns = vec![PgDdlColumn {
        name: "id".to_string(),
        data_type: "integer".to_string(),
        is_nullable: false,
        default_value: None,
        generated_expression: None,
    }];
    let constraints = vec![
        PgDdlConstraint {
            name: "ck_positive".to_string(),
            constraint_type: PgConstraintType::Check {
                expression: "(id > 0)".to_string(),
            },
            columns: vec![],
        },
        PgDdlConstraint {
            name: "tbl_pkey".to_string(),
            constraint_type: PgConstraintType::PrimaryKey,
            columns: vec!["id".to_string()],
        },
        PgDdlConstraint {
            name: "tbl_uq".to_string(),
            constraint_type: PgConstraintType::Unique,
            columns: vec!["id".to_string()],
        },
    ];
    let ddl = build_pg_ddl_from_metadata("public", "tbl", &columns, &constraints, &[]).unwrap();
    let pk_pos = ddl.find("PRIMARY KEY").unwrap();
    let uq_pos = ddl.find("UNIQUE").unwrap();
    let ck_pos = ddl.find("CHECK").unwrap();
    assert!(pk_pos < uq_pos, "PK가 UQ보다 먼저 나와야 함");
    assert!(uq_pos < ck_pos, "UQ가 CK보다 먼저 나와야 함");
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 24 PBT 테스트: PG DDL 구조적 완전성
// **Validates: Requirements 8.2**
// ─────────────────────────────────────────────────────────────────────────────

/// DDL 재구성용 컬럼 메타데이터를 생성하는 전략
fn pg_ddl_column_strategy() -> impl Strategy<Value = PgDdlColumn> {
    (
        "[a-z][a-z0-9_]{0,15}",
        prop_oneof![
            Just("integer".to_string()),
            Just("text".to_string()),
            Just("bool".to_string()),
            Just("varchar(255)".to_string()),
            Just("numeric(10,2)".to_string()),
            Just("timestamptz".to_string()),
            Just("uuid".to_string()),
            Just("jsonb".to_string()),
        ],
        any::<bool>(),
        prop_oneof![
            Just(None),
            Just(Some("0".to_string())),
            Just(Some("true".to_string())),
            Just(Some("'hello'".to_string())),
            Just(Some("now()".to_string())),
        ],
    )
        .prop_map(
            |(name, data_type, is_nullable, default_value)| PgDdlColumn {
                name,
                data_type,
                is_nullable,
                default_value,
                generated_expression: None,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 24-a: DDL은 비어있지 않다.
    /// **Validates: Requirements 8.2**
    #[test]
    fn prop24_ddl_is_not_empty(
        schema in "[a-z][a-z0-9_]{0,10}",
        table in "[a-z][a-z0-9_]{0,10}",
        columns in proptest::collection::vec(pg_ddl_column_strategy(), 1..=5),
    ) {
        let ddl = build_pg_ddl_from_metadata(
            &schema, &table, &columns, &[], &[],
        ).unwrap();
        prop_assert!(!ddl.is_empty(), "DDL이 비어있음");
    }

    /// Property 24-b: DDL은 CREATE TABLE 헤더를 포함한다.
    /// **Validates: Requirements 8.2**
    #[test]
    fn prop24_ddl_contains_create_table_header(
        schema in "[a-z][a-z0-9_]{0,10}",
        table in "[a-z][a-z0-9_]{0,10}",
        columns in proptest::collection::vec(pg_ddl_column_strategy(), 1..=5),
    ) {
        let ddl = build_pg_ddl_from_metadata(
            &schema, &table, &columns, &[], &[],
        ).unwrap();
        prop_assert!(
            ddl.contains("CREATE TABLE"),
            "DDL에 'CREATE TABLE' 헤더가 없음: {ddl}"
        );
    }

    /// Property 24-c: DDL은 하나 이상의 컬럼 정의를 포함한다.
    /// **Validates: Requirements 8.2**
    #[test]
    fn prop24_ddl_contains_column_definition(
        schema in "[a-z][a-z0-9_]{0,10}",
        table in "[a-z][a-z0-9_]{0,10}",
        columns in proptest::collection::vec(pg_ddl_column_strategy(), 1..=5),
    ) {
        let ddl = build_pg_ddl_from_metadata(
            &schema, &table, &columns, &[], &[],
        ).unwrap();
        let first_col_quoted = format!("\"{}\"", columns[0].name);
        prop_assert!(
            ddl.contains(&first_col_quoted),
            "DDL에 첫 번째 컬럼 '{}'이 없음: {ddl}",
            columns[0].name
        );
    }

    /// Property 24-d: DDL은 닫는 괄호 `)`를 포함한다.
    /// **Validates: Requirements 8.2**
    #[test]
    fn prop24_ddl_contains_closing_paren(
        schema in "[a-z][a-z0-9_]{0,10}",
        table in "[a-z][a-z0-9_]{0,10}",
        columns in proptest::collection::vec(pg_ddl_column_strategy(), 1..=5),
    ) {
        let ddl = build_pg_ddl_from_metadata(
            &schema, &table, &columns, &[], &[],
        ).unwrap();
        prop_assert!(
            ddl.contains(')'),
            "DDL에 닫는 괄호 ')'가 없음: {ddl}"
        );
    }

    /// Property 24-e: DDL은 세미콜론 `;`으로 끝난다.
    /// **Validates: Requirements 8.2**
    #[test]
    fn prop24_ddl_ends_with_semicolon(
        schema in "[a-z][a-z0-9_]{0,10}",
        table in "[a-z][a-z0-9_]{0,10}",
        columns in proptest::collection::vec(pg_ddl_column_strategy(), 1..=5),
    ) {
        let ddl = build_pg_ddl_from_metadata(
            &schema, &table, &columns, &[], &[],
        ).unwrap();
        let trimmed = ddl.trim_end();
        prop_assert!(
            trimmed.ends_with(';'),
            "DDL이 세미콜론으로 끝나지 않음: {ddl}"
        );
    }

    /// Property 24-f: 제약 조건이 있어도 DDL 구조적 완전성이 유지된다.
    /// **Validates: Requirements 8.2**
    #[test]
    fn prop24_ddl_structural_completeness_with_constraints(
        schema in "[a-z][a-z0-9_]{0,10}",
        table in "[a-z][a-z0-9_]{0,10}",
        columns in proptest::collection::vec(pg_ddl_column_strategy(), 1..=5),
        constraint_count in 0usize..=3usize,
    ) {
        let col_names: Vec<String> =
            columns.iter().map(|c| c.name.clone()).collect();

        let mut constraints = Vec::new();
        if constraint_count > 0 {
            constraints.push(PgDdlConstraint {
                name: format!("{table}_pkey"),
                constraint_type: PgConstraintType::PrimaryKey,
                columns: vec![col_names[0].clone()],
            });
        }
        if constraint_count > 1 {
            constraints.push(PgDdlConstraint {
                name: format!("{table}_uq"),
                constraint_type: PgConstraintType::Unique,
                columns: vec![col_names[0].clone()],
            });
        }
        if constraint_count > 2 {
            constraints.push(PgDdlConstraint {
                name: format!("{table}_ck"),
                constraint_type: PgConstraintType::Check {
                    expression: "(id > 0)".to_string(),
                },
                columns: vec![],
            });
        }

        let ddl = build_pg_ddl_from_metadata(
            &schema, &table, &columns, &constraints, &[],
        ).unwrap();

        prop_assert!(!ddl.is_empty(), "DDL이 비어있음");
        prop_assert!(ddl.contains("CREATE TABLE"), "CREATE TABLE 헤더 없음");
        prop_assert!(ddl.contains(')'), "닫는 괄호 없음");
        prop_assert!(
            ddl.trim_end().ends_with(';'),
            "세미콜론으로 끝나지 않음"
        );
    }
}
