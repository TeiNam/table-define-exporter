//! PostgreSQL 코멘트 DDL (`COMMENT ON ...`).
//!
//! MySQL 은 `SHOW CREATE TABLE` 에 `COMMENT '...'` 가 들어 있지만 PostgreSQL 코멘트는 별도
//! 객체라, 재구성한 DDL 뒤에 `COMMENT ON` 문으로 붙여야 SQL 파일에서 보존된다.

use sqlx::PgPool;

use super::schema_ddl::quote_literal;
use crate::{db::try_get_or_warn, error::AppError, identifier::quote_pg_identifier};

/// 릴레이션(테이블/뷰/머티리얼라이즈드 뷰/외부 테이블)과 그 컬럼의 `COMMENT ON` 문.
pub(super) async fn fetch_comment_ddl(
    pool: &PgPool,
    schema: &str,
    relation: &str,
) -> Result<Vec<String>, AppError> {
    let rows = sqlx::query(
        "SELECT c.relkind::text AS relkind, NULL::text AS column_name, \
                obj_description(c.oid, 'pg_class') AS comment, 0 AS ord \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2 \
           AND obj_description(c.oid, 'pg_class') IS NOT NULL \
         UNION ALL \
         SELECT c.relkind::text, a.attname::text, col_description(c.oid, a.attnum), a.attnum \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_attribute a \
           ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         WHERE n.nspname = $1 AND c.relname = $2 \
           AND col_description(c.oid, a.attnum) IS NOT NULL \
         ORDER BY ord",
    )
    .bind(schema)
    .bind(relation)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: relation.to_string(),
        source: e,
    })?;

    rows.iter()
        .map(|row| {
            let relkind: String = try_get_or_warn(row, "relkind", schema, relation);
            let column: Option<String> = try_get_or_warn(row, "column_name", schema, relation);
            let comment: String = try_get_or_warn(row, "comment", schema, relation);
            build_comment_ddl(schema, relation, &relkind, column.as_deref(), &comment)
        })
        .collect()
}

/// `COMMENT ON {TABLE|VIEW|MATERIALIZED VIEW|FOREIGN TABLE} .. IS '..';` 또는
/// `COMMENT ON COLUMN "s"."t"."c" IS '..';`
fn build_comment_ddl(
    schema: &str,
    relation: &str,
    relkind: &str,
    column: Option<&str>,
    comment: &str,
) -> Result<String, AppError> {
    let target = format!(
        "{}.{}",
        quote_pg_identifier(schema)?,
        quote_pg_identifier(relation)?
    );
    let object = match column {
        Some(column) => format!("COLUMN {target}.{}", quote_pg_identifier(column)?),
        None => {
            let kind = match relkind {
                "v" => "VIEW",
                "m" => "MATERIALIZED VIEW",
                "f" => "FOREIGN TABLE",
                _ => "TABLE",
            };
            format!("{kind} {target}")
        }
    };
    Ok(format!(
        "COMMENT ON {object} IS {};",
        quote_literal(comment)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_comment_ddl() {
        assert_eq!(
            build_comment_ddl("a", "t", "r", None, "테이블 코멘트").unwrap(),
            r#"COMMENT ON TABLE "a"."t" IS '테이블 코멘트';"#
        );
        assert_eq!(
            build_comment_ddl("a", "t", "r", Some("name"), "it's").unwrap(),
            r#"COMMENT ON COLUMN "a"."t"."name" IS 'it''s';"#
        );
        assert_eq!(
            build_comment_ddl("a", "mv", "m", None, "x").unwrap(),
            r#"COMMENT ON MATERIALIZED VIEW "a"."mv" IS 'x';"#
        );
        assert!(build_comment_ddl("a", "t;x", "r", None, "x").is_err());
    }
}
