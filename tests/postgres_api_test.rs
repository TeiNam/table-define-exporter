//! 공개 API 불변 회귀 테스트 (Task 7.6)
//!
//! postgres 모듈 분리(Task 7.1–7.4) 이후에도 기존 외부 경로
//! `td_export::db::postgres::*`가 그대로 유지됨을 컴파일 타임에 보장한다.
//! 경로가 깨지면 이 테스트는 컴파일되지 않으므로 `cargo test` 실행 전에
//! 빌드 단계에서 곧바로 감지된다.
//!
//! 검증 대상 경로(총 11종):
//! - 함수: `build_pg_column_type`, `build_pg_ddl_from_metadata`,
//!   `determine_pg_extra`, `filter_pg_schemas`, `is_pg_system_schema`,
//!   `parse_pg_indexdef`
//! - 타입: `ParsedIndex`, `PgClient`, `PgConstraintType`, `PgDdlColumn`,
//!   `PgDdlConstraint`

use td_export::db::postgres::{
    ParsedIndex, PgClient, PgConstraintType, PgDdlColumn, PgDdlConstraint, build_pg_column_type,
    build_pg_ddl_from_metadata, determine_pg_extra, filter_pg_schemas, is_pg_system_schema,
    parse_pg_indexdef,
};

#[test]
fn postgres_public_api_paths_compile() {
    // 함수 경로: 실제 호출로 시그니처까지 고정한다.
    let _ = build_pg_column_type("int4", None, None, None);
    let _ = determine_pg_extra("", "", None);
    // parse_pg_indexdef는 Task 10.2에서 `ParsedIndex` 반환 타입으로 변경됨.
    let _: ParsedIndex = parse_pg_indexdef("CREATE INDEX i ON t USING btree (c)");
    let _ = is_pg_system_schema("public");
    let _ = filter_pg_schemas(vec!["public".to_string()], None);
    let _ = build_pg_ddl_from_metadata("s", "t", &[], &[], &[]);

    // 타입 경로: 컴파일 타임에만 참조(런타임 생성자 실행 없음).
    // PgClient는 `pub struct`이지만 `::connect`가 async + 실제 DB를 요구하므로
    // 타입 마커(`Option<PgClient>`)로만 참조하여 경로 유지 여부를 증명한다.
    let _pgclient_marker: Option<PgClient> = None;
    let _col_marker: Option<PgDdlColumn> = None;
    let _cons_marker: Option<PgDdlConstraint> = None;
    let _ct_marker: Option<PgConstraintType> = None;
    let _parsed_marker: Option<ParsedIndex> = None;
}
