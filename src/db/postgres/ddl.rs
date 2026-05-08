//! PostgreSQL DDL 재구성 전용 모듈.
//!
//! `build_pg_ddl_from_metadata` — 순수 함수로 메타데이터 → CREATE TABLE DDL 문자열을 재구성.
//! `fetch_table_ddl` — `PgPool`에서 컬럼/제약/인덱스 메타데이터를 조회한 뒤
//! `build_pg_ddl_from_metadata`에 위임하는 async 헬퍼.

use crate::{error::AppError, identifier::quote_pg_identifier};

use super::parse::{extract_check_expression, parse_fk_actions_from_condef, quote_column_list};
use super::types::{build_pg_column_type, PgConstraintType, PgDdlColumn, PgDdlConstraint};
use crate::db::try_get_or_warn;

/// 테이블 메타데이터로부터 PostgreSQL DDL 문자열을 재구성한다.
///
/// 순수 함수로 구현하여 PBT 테스트가 가능하다.
/// `quote_pg_identifier`를 사용하여 스키마/테이블/컬럼 이름을 안전하게 인용한다.
///
/// DDL 구조:
/// ```sql
/// CREATE TABLE "schema"."table" (
///     "col" type [NOT NULL] [DEFAULT default] [GENERATED ALWAYS AS (expr) STORED],
///     CONSTRAINT "pk" PRIMARY KEY (columns),
///     CONSTRAINT "uq" UNIQUE (columns),
///     CONSTRAINT "fk" FOREIGN KEY (cols) REFERENCES "ref" (ref_cols) ...
///     CONSTRAINT "ck" CHECK (expression)
/// );
/// -- 인덱스
/// indexdef;
/// ```
pub fn build_pg_ddl_from_metadata(
    schema: &str,
    table: &str,
    columns: &[PgDdlColumn],
    constraints: &[PgDdlConstraint],
    index_defs: &[String],
) -> Result<String, AppError> {
    let quoted_schema = quote_pg_identifier(schema)?;
    let quoted_table = quote_pg_identifier(table)?;

    let mut ddl = format!("CREATE TABLE {quoted_schema}.{quoted_table} (\n");

    // 컬럼 정의와 제약 조건을 모두 모아서 쉼표로 구분
    let mut entries: Vec<String> = Vec::new();

    // 컬럼 정의 추가
    for col in columns {
        let quoted_col = quote_pg_identifier(&col.name)?;
        let mut col_def = format!("    {quoted_col} {}", col.data_type);

        // NOT NULL 제약
        if !col.is_nullable {
            col_def.push_str(" NOT NULL");
        }

        // GENERATED ALWAYS AS (...) STORED (기본값보다 우선)
        if let Some(ref expr) = col.generated_expression {
            col_def.push_str(&format!(" GENERATED ALWAYS AS ({expr}) STORED"));
        } else if let Some(ref default) = col.default_value {
            // DEFAULT 값 (generated 컬럼이 아닌 경우에만)
            col_def.push_str(&format!(" DEFAULT {default}"));
        }

        entries.push(col_def);
    }

    // 제약 조건 추가 (PK → UQ → FK → CK 순서)
    for c in constraints
        .iter()
        .filter(|c| matches!(c.constraint_type, PgConstraintType::PrimaryKey))
    {
        let quoted_name = quote_pg_identifier(&c.name)?;
        let cols = quote_column_list(&c.columns)?;
        entries.push(format!("    CONSTRAINT {quoted_name} PRIMARY KEY ({cols})"));
    }

    for c in constraints
        .iter()
        .filter(|c| matches!(c.constraint_type, PgConstraintType::Unique))
    {
        let quoted_name = quote_pg_identifier(&c.name)?;
        let cols = quote_column_list(&c.columns)?;
        entries.push(format!("    CONSTRAINT {quoted_name} UNIQUE ({cols})"));
    }

    for c in constraints
        .iter()
        .filter(|c| matches!(c.constraint_type, PgConstraintType::ForeignKey { .. }))
    {
        if let PgConstraintType::ForeignKey {
            ref ref_schema,
            ref ref_table,
            ref ref_columns,
            ref on_delete,
            ref on_update,
        } = c.constraint_type
        {
            let quoted_name = quote_pg_identifier(&c.name)?;
            let local_cols = quote_column_list(&c.columns)?;
            let quoted_ref_schema = quote_pg_identifier(ref_schema)?;
            let quoted_ref_table = quote_pg_identifier(ref_table)?;
            let ref_cols = quote_column_list(ref_columns)?;
            entries.push(format!(
                "    CONSTRAINT {quoted_name} FOREIGN KEY ({local_cols}) \
                 REFERENCES {quoted_ref_schema}.{quoted_ref_table} ({ref_cols}) \
                 ON DELETE {on_delete} ON UPDATE {on_update}"
            ));
        }
    }

    for c in constraints
        .iter()
        .filter(|c| matches!(c.constraint_type, PgConstraintType::Check { .. }))
    {
        if let PgConstraintType::Check { ref expression } = c.constraint_type {
            let quoted_name = quote_pg_identifier(&c.name)?;
            entries.push(format!("    CONSTRAINT {quoted_name} CHECK ({expression})"));
        }
    }

    // 엔트리들을 쉼표+개행으로 결합
    ddl.push_str(&entries.join(",\n"));
    ddl.push_str("\n);\n");

    // 인덱스 정의 추가
    for idx_def in index_defs {
        ddl.push_str(&format!("{idx_def};\n"));
    }

    Ok(ddl)
}

/// PostgreSQL `PgPool`을 통해 테이블 메타데이터를 조회하고 DDL을 재구성한다.
///
/// `information_schema.columns` + `pg_catalog.pg_constraint` + `pg_get_indexdef()`를
/// 조합하여 CREATE TABLE DDL 문자열을 재구성한다.
/// PostgreSQL에는 `pg_get_tabledef()` 내장 함수가 없으므로 직접 재구성한다.
///
/// 제약 조건 쿼리는 FK의 참조 컬럼 이름을 서브쿼리(WITH ORDINALITY + pg_attribute JOIN)로
/// 한 번에 해석하여 FK 개수에 비례한 N+1 쿼리를 제거한다 (Req 10.1, 10.2).
pub(super) async fn fetch_table_ddl(
    pool: &sqlx::PgPool,
    schema: &str,
    table: &str,
) -> Result<String, AppError> {
    // 1. 컬럼 정보 조회 (ordinal_position 순)
    let col_rows = sqlx::query(
        "SELECT \
             c.column_name, \
             c.udt_name, \
             c.character_maximum_length::int4 AS char_max_length, \
             c.numeric_precision::int4 AS numeric_precision, \
             c.numeric_scale::int4 AS numeric_scale, \
             c.is_nullable, \
             c.column_default, \
             a.attgenerated::text AS attgenerated, \
             c.generation_expression \
         FROM information_schema.columns c \
         JOIN pg_catalog.pg_attribute a \
           ON a.attrelid = ( \
               SELECT cl.oid FROM pg_catalog.pg_class cl \
               JOIN pg_catalog.pg_namespace ns ON ns.oid = cl.relnamespace \
               WHERE ns.nspname = $1 AND cl.relname = $2 \
           ) \
           AND a.attname = c.column_name \
           AND a.attnum > 0 \
           AND NOT a.attisdropped \
         WHERE c.table_schema = $1 AND c.table_name = $2 \
         ORDER BY c.ordinal_position",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: table.to_string(),
        source: e,
    })?;

    // 컬럼 메타데이터 변환
    let mut ddl_columns: Vec<PgDdlColumn> = Vec::new();
    for row in &col_rows {
        // try_get 실패 시 경고 로그 + 기본값 반환 (Requirements 5.2)
        let column_name: String = try_get_or_warn(row, "column_name", schema, table);
        let udt_name: String = try_get_or_warn(row, "udt_name", schema, table);
        let char_max_length: Option<i32> = try_get_or_warn(row, "char_max_length", schema, table);
        let numeric_precision: Option<i32> =
            try_get_or_warn(row, "numeric_precision", schema, table);
        let numeric_scale: Option<i32> = try_get_or_warn(row, "numeric_scale", schema, table);
        let is_nullable: String = try_get_or_warn(row, "is_nullable", schema, table);
        let column_default: Option<String> = try_get_or_warn(row, "column_default", schema, table);
        let attgenerated: String = try_get_or_warn(row, "attgenerated", schema, table);
        let generation_expression: Option<String> =
            try_get_or_warn(row, "generation_expression", schema, table);

        // 컬럼 타입 구성
        let data_type =
            build_pg_column_type(&udt_name, char_max_length, numeric_precision, numeric_scale);

        // STORED generated 컬럼 감지
        let generated_expression = if attgenerated == "s" {
            generation_expression
        } else {
            None
        };

        ddl_columns.push(PgDdlColumn {
            name: column_name,
            data_type,
            is_nullable: is_nullable == "YES",
            default_value: column_default,
            generated_expression,
        });
    }

    // 2. 제약 조건 조회 (pg_constraint)
    //
    // FK의 참조 컬럼 이름을 해석하기 위해 과거에는 제약 조건마다 별도의
    // `pg_attribute` 쿼리를 발행했으나, 이는 테이블당 FK 개수에 비례한
    // N+1 쿼리를 유발했다 (Req 10.1).
    //
    // 해결: `unnest(...) WITH ORDINALITY` + `pg_attribute` JOIN을 서브쿼리로
    // 배치하여 다음을 한 번에 반환한다 (Req 10.2).
    //   - local_col_names : `con.conkey`  → 로컬 컬럼 이름 배열 (정의 순서 보존)
    //   - ref_col_names   : `con.confkey` → 참조 컬럼 이름 배열 (정의 순서 보존)
    //
    // 결과적으로 테이블당 제약 조건 쿼리는 1회로 고정되며,
    // 로컬 컬럼 매핑용 추가 `pg_attribute` 스캔도 함께 제거된다.
    let constraint_rows = sqlx::query(
        "SELECT \
             con.conname, \
             con.contype::text, \
             ( \
                 SELECT array_agg(a.attname ORDER BY k.ord) \
                 FROM unnest(con.conkey) WITH ORDINALITY AS k(attnum, ord) \
                 JOIN pg_catalog.pg_attribute a \
                   ON a.attrelid = con.conrelid \
                  AND a.attnum = k.attnum \
                  AND a.attnum > 0 \
                  AND NOT a.attisdropped \
             ) AS local_col_names, \
             ( \
                 SELECT array_agg(a.attname ORDER BY k.ord) \
                 FROM unnest(con.confkey) WITH ORDINALITY AS k(attnum, ord) \
                 JOIN pg_catalog.pg_attribute a \
                   ON a.attrelid = con.confrelid \
                  AND a.attnum = k.attnum \
                  AND a.attnum > 0 \
                  AND NOT a.attisdropped \
             ) AS ref_col_names, \
             pg_get_constraintdef(con.oid) AS condef, \
             ref_ns.nspname AS ref_schema, \
             ref_cl.relname AS ref_table \
         FROM pg_catalog.pg_constraint con \
         JOIN pg_catalog.pg_class cl ON cl.oid = con.conrelid \
         JOIN pg_catalog.pg_namespace ns ON ns.oid = cl.relnamespace \
         LEFT JOIN pg_catalog.pg_class ref_cl \
           ON ref_cl.oid = con.confrelid \
         LEFT JOIN pg_catalog.pg_namespace ref_ns \
           ON ref_ns.oid = ref_cl.relnamespace \
         WHERE ns.nspname = $1 AND cl.relname = $2 \
           AND con.contype IN ('p', 'u', 'f', 'c') \
         ORDER BY \
           CASE con.contype \
             WHEN 'p' THEN 1 \
             WHEN 'u' THEN 2 \
             WHEN 'f' THEN 3 \
             WHEN 'c' THEN 4 \
           END, \
           con.conname",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: table.to_string(),
        source: e,
    })?;

    // 제약 조건 메타데이터 변환
    let mut ddl_constraints: Vec<PgDdlConstraint> = Vec::new();
    for row in &constraint_rows {
        // try_get 실패 시 경고 로그 + 기본값 반환 (Requirements 5.2)
        let conname: String = try_get_or_warn(row, "conname", schema, table);
        let contype: String = try_get_or_warn(row, "contype", schema, table);
        // 서브쿼리가 반환한 컬럼 이름 배열(정의 순서 보존).
        // CHECK 제약처럼 conkey/confkey가 NULL이거나 관련 없는 경우 서브쿼리는
        // NULL을 반환하므로 Option으로 받는다.
        let local_col_names: Option<Vec<String>> =
            try_get_or_warn(row, "local_col_names", schema, table);
        let ref_col_names: Option<Vec<String>> =
            try_get_or_warn(row, "ref_col_names", schema, table);
        let condef: String = try_get_or_warn(row, "condef", schema, table);
        let ref_schema: Option<String> = try_get_or_warn(row, "ref_schema", schema, table);
        let ref_table: Option<String> = try_get_or_warn(row, "ref_table", schema, table);

        // 로컬 컬럼 이름 목록 (PK/UQ/FK에서 사용; CHECK에서는 빈 Vec로 남음)
        let local_columns = local_col_names.unwrap_or_default();

        let constraint_type = match contype.as_str() {
            "p" => PgConstraintType::PrimaryKey,
            "u" => PgConstraintType::Unique,
            "f" => {
                // ON DELETE / ON UPDATE 액션 추출
                let (on_delete, on_update) = parse_fk_actions_from_condef(&condef);

                PgConstraintType::ForeignKey {
                    ref_schema: ref_schema.unwrap_or_default(),
                    ref_table: ref_table.unwrap_or_default(),
                    // 참조 컬럼 이름도 서브쿼리로 이미 해석됨 → 추가 쿼리 없음
                    ref_columns: ref_col_names.unwrap_or_default(),
                    on_delete,
                    on_update,
                }
            }
            "c" => {
                // 시스템 생성 CHECK 제약 조건 제외 (NOT NULL 등)
                if conname.ends_with("_not_null") {
                    continue;
                }
                // pg_get_constraintdef에서 CHECK 표현식 추출
                let expression = extract_check_expression(&condef);
                PgConstraintType::Check { expression }
            }
            _ => continue,
        };

        ddl_constraints.push(PgDdlConstraint {
            name: conname,
            constraint_type,
            columns: local_columns,
        });
    }

    // 3. 인덱스 정의 조회 (PK/UQ 제약 조건 인덱스 제외)
    let index_rows = sqlx::query(
        "SELECT pg_get_indexdef(i.indexrelid) AS indexdef \
         FROM pg_catalog.pg_index i \
         JOIN pg_catalog.pg_class cl ON cl.oid = i.indrelid \
         JOIN pg_catalog.pg_namespace ns ON ns.oid = cl.relnamespace \
         WHERE ns.nspname = $1 AND cl.relname = $2 \
           AND NOT i.indisprimary \
           AND NOT EXISTS ( \
               SELECT 1 FROM pg_catalog.pg_constraint con \
               WHERE con.conindid = i.indexrelid \
                 AND con.contype IN ('p', 'u') \
           ) \
         ORDER BY i.indexrelid",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: table.to_string(),
        source: e,
    })?;

    let index_defs: Vec<String> = index_rows
        .iter()
        .map(|row| try_get_or_warn::<_, String>(row, "indexdef", schema, table))
        .filter(|s| !s.is_empty())
        .collect();

    // 4. DDL 재구성
    build_pg_ddl_from_metadata(schema, table, &ddl_columns, &ddl_constraints, &index_defs)
}
