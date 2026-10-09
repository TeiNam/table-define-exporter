//! PostgreSQL 함수·프로시저와 확장(extension)의 생성문.
//!
//! 테이블 기본값·CHECK·인덱스 식·뷰가 쓰는 사용자 함수와, 확장 타입(`citext`)·연산자 클래스
//! (btree_gist 의 EXCLUDE)·함수(`uuid_generate_v4()`)가 먼저 있어야 SQL 파일을 빈 DB 에 실행할 수
//! 있다. 확장에 속한 객체는 `CREATE EXTENSION` 이 만들므로 따로 내보내지 않는다.
// ponytail: 집계 함수(CREATE AGGREGATE)·트리거·권한은 생략 — 필요해지면 pg_dump 처럼 추가.

use sqlx::PgPool;

use crate::{
    db::try_get_or_warn,
    error::AppError,
    identifier::{quote_pg_identifier, validate_identifier},
};

/// 사용자 함수·프로시저 하나 (`pg_get_functiondef` 원문)
#[derive(Debug)]
pub(super) struct PgFunction {
    pub oid: i64,
    pub name: String,
    pub definition: String,
    /// 인자·반환 타입이 테이블 행 타입이거나(`RETURNS SETOF s.t`) 본문이 테이블을 참조(BEGIN ATOMIC)
    /// — 테이블을 만든 뒤에 만들어야 한다
    pub uses_tables: bool,
}

impl PgFunction {
    /// 실행할 문장 (`pg_get_functiondef` 는 `;` 없이 끝난다)
    pub fn statement(&self) -> String {
        format!("{};", self.definition.trim_end())
    }
}

/// 스키마의 함수·프로시저·윈도 함수 (확장에 속한 것 제외, OID 순). 이름에 위험 문자가 있으면 경고 후 뺀다.
pub(super) async fn fetch_functions(
    pool: &PgPool,
    schema: &str,
) -> Result<Vec<PgFunction>, AppError> {
    const LABEL: &str = "functions";
    let rows = sqlx::query(
        "SELECT p.oid::int8 AS oid, p.proname::text AS name, \
                pg_get_functiondef(p.oid) AS definition, \
                EXISTS ( \
                    SELECT 1 FROM pg_catalog.pg_depend d \
                    LEFT JOIN pg_catalog.pg_type rt \
                      ON d.refclassid = 'pg_catalog.pg_type'::regclass AND rt.oid = d.refobjid \
                    LEFT JOIN pg_catalog.pg_type et ON et.oid = rt.typelem \
                    JOIN pg_catalog.pg_class rc ON rc.oid = CASE \
                        WHEN d.refclassid = 'pg_catalog.pg_class'::regclass THEN d.refobjid \
                        WHEN rt.typrelid <> 0 THEN rt.typrelid \
                        ELSE et.typrelid END \
                    WHERE d.classid = 'pg_catalog.pg_proc'::regclass AND d.objid = p.oid \
                      AND rc.relkind IN ('r', 'p', 'v', 'm', 'f') \
                ) AS uses_tables \
         FROM pg_catalog.pg_proc p \
         JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
         WHERE n.nspname = $1 AND p.prokind IN ('f', 'p', 'w') \
           AND NOT EXISTS ( \
               SELECT 1 FROM pg_catalog.pg_depend e \
               WHERE e.classid = 'pg_catalog.pg_proc'::regclass AND e.objid = p.oid \
                 AND e.deptype = 'e' \
           ) \
         ORDER BY p.oid",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: LABEL.to_string(),
        source: e,
    })?;

    Ok(rows
        .iter()
        .map(|row| PgFunction {
            oid: try_get_or_warn(row, "oid", schema, LABEL),
            name: try_get_or_warn(row, "name", schema, LABEL),
            definition: try_get_or_warn(row, "definition", schema, LABEL),
            uses_tables: try_get_or_warn(row, "uses_tables", schema, LABEL),
        })
        .filter(|f| match validate_identifier(&f.name) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("{schema}.{} (function) 생성문 생략: {e}", f.name);
                false
            }
        })
        .collect())
}

/// 스키마에 설치됐거나 스키마의 객체(컬럼 타입·기본값·제약·인덱스 연산자 클래스·뷰·함수)가 쓰는
/// 확장의 `CREATE EXTENSION IF NOT EXISTS .. WITH SCHEMA .. CASCADE;`. 다른 스키마에 설치된 확장도
/// 쓰면 낸다 — 그 스키마만 내보내도 실행되게. plpgsql 은 항상 있으므로 뺀다.
pub(super) async fn fetch_extension_ddl(
    pool: &PgPool,
    schema: &str,
) -> Result<Vec<String>, AppError> {
    const LABEL: &str = "extensions";
    let rows = sqlx::query(
        "WITH objs AS ( \
             SELECT 'pg_catalog.pg_class'::regclass AS classid, c.oid AS objid \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 \
             UNION ALL \
             SELECT 'pg_catalog.pg_type'::regclass, t.oid FROM pg_catalog.pg_type t \
             JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace WHERE n.nspname = $1 \
             UNION ALL \
             SELECT 'pg_catalog.pg_proc'::regclass, p.oid FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = $1 \
             UNION ALL \
             SELECT 'pg_catalog.pg_constraint'::regclass, con.oid FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_namespace n ON n.oid = con.connamespace WHERE n.nspname = $1 \
             UNION ALL \
             SELECT 'pg_catalog.pg_attrdef'::regclass, ad.oid FROM pg_catalog.pg_attrdef ad \
             JOIN pg_catalog.pg_class c ON c.oid = ad.adrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 \
             UNION ALL \
             SELECT 'pg_catalog.pg_rewrite'::regclass, r.oid FROM pg_catalog.pg_rewrite r \
             JOIN pg_catalog.pg_class c ON c.oid = r.ev_class \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 \
         ) \
         SELECT e.extname::text AS name, en.nspname::text AS ext_schema \
         FROM objs o \
         JOIN pg_catalog.pg_depend d ON d.classid = o.classid AND d.objid = o.objid \
         JOIN pg_catalog.pg_depend x \
           ON x.classid = d.refclassid AND x.objid = d.refobjid AND x.deptype = 'e' \
         JOIN pg_catalog.pg_extension e ON e.oid = x.refobjid \
         JOIN pg_catalog.pg_namespace en ON en.oid = e.extnamespace \
         WHERE e.extname <> 'plpgsql' \
         UNION \
         SELECT e.extname::text, n.nspname::text FROM pg_catalog.pg_extension e \
         JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace \
         WHERE n.nspname = $1 AND e.extname <> 'plpgsql' \
         ORDER BY 1",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: LABEL.to_string(),
        source: e,
    })?;

    let mut statements = Vec::new();
    for row in &rows {
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let ext_schema: String = try_get_or_warn(row, "ext_schema", schema, LABEL);
        match build_extension_ddl(schema, &name, &ext_schema) {
            Ok(ddl) => statements.extend(ddl),
            Err(e) => tracing::warn!("{schema}: 확장 {name} 생성문 생략: {e}"),
        }
    }
    Ok(statements)
}

/// `CREATE EXTENSION IF NOT EXISTS "x" WITH SCHEMA "s" CASCADE;` (이미 있으면 권한 없이도 통과).
/// 확장의 스키마가 이 파일의 스키마가 아니면 먼저 만든다 — `public`·`pg_*` 는 이미 있다.
fn build_extension_ddl(
    file_schema: &str,
    name: &str,
    ext_schema: &str,
) -> Result<Vec<String>, AppError> {
    let quoted_schema = quote_pg_identifier(ext_schema)?;
    let mut statements = Vec::new();
    if ext_schema != file_schema && ext_schema != "public" && !ext_schema.starts_with("pg_") {
        statements.push(format!("CREATE SCHEMA IF NOT EXISTS {quoted_schema};"));
    }
    statements.push(format!(
        "CREATE EXTENSION IF NOT EXISTS {} WITH SCHEMA {quoted_schema} CASCADE;",
        quote_pg_identifier(name)?
    ));
    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_extension_ddl() {
        assert_eq!(
            build_extension_ddl("app", "citext", "public").unwrap(),
            [r#"CREATE EXTENSION IF NOT EXISTS "citext" WITH SCHEMA "public" CASCADE;"#]
        );
        assert_eq!(
            build_extension_ddl("app", "postgis", "extensions").unwrap(),
            [
                r#"CREATE SCHEMA IF NOT EXISTS "extensions";"#,
                r#"CREATE EXTENSION IF NOT EXISTS "postgis" WITH SCHEMA "extensions" CASCADE;"#
            ]
        );
        assert_eq!(
            build_extension_ddl("app", "plpython3u", "pg_catalog").unwrap(),
            [r#"CREATE EXTENSION IF NOT EXISTS "plpython3u" WITH SCHEMA "pg_catalog" CASCADE;"#]
        );
        let function = PgFunction {
            oid: 1,
            name: "f".into(),
            definition: "CREATE OR REPLACE FUNCTION s.f()\n RETURNS integer\nAS $f$ SELECT 1 $f$\n"
                .into(),
            uses_tables: false,
        };
        assert!(function.statement().ends_with("$f$ SELECT 1 $f$;"));
    }
}
