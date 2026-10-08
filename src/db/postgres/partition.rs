//! PostgreSQL 선언적 파티션의 하위 파티션 DDL.
//!
//! 파티션 부모만 정의서·SQL 목록에 남기고(MySQL 처럼 테이블 하나), 하위 파티션은 부모 DDL
//! 뒤에 `CREATE TABLE child PARTITION OF parent FOR VALUES ...;` 로 붙인다. 컬럼·PK·FK·
//! 인덱스는 부모에서 상속되므로 하위 파티션에는 다시 쓰지 않는다.
// ponytail: 하위 파티션에만 따로 만든 인덱스·CHECK·코멘트는 생략 — 필요해지면 같은 방식으로 추가.

use sqlx::PgPool;

use crate::{db::try_get_or_warn, error::AppError, identifier::quote_pg_identifier};

/// 부모 테이블의 모든 하위 파티션 생성문 (단계 순 — 하위의 하위 파티션까지).
pub(super) async fn fetch_partitions_ddl(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, AppError> {
    let rows = sqlx::query(
        "SELECT cn.nspname::text AS child_schema, c.relname::text AS child_name, \
                pn.nspname::text AS parent_schema, p.relname::text AS parent_name, \
                pg_get_expr(c.relpartbound, c.oid) AS bound, \
                pg_get_partkeydef(c.oid) AS partition_key \
         FROM pg_partition_tree(format('%I.%I', $1, $2)::regclass) t \
         JOIN pg_catalog.pg_class c ON c.oid = t.relid \
         JOIN pg_catalog.pg_namespace cn ON cn.oid = c.relnamespace \
         JOIN pg_catalog.pg_class p ON p.oid = t.parentrelid \
         JOIN pg_catalog.pg_namespace pn ON pn.oid = p.relnamespace \
         WHERE t.level > 0 \
         ORDER BY t.level, c.relname",
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

    rows.iter()
        .map(|row| {
            let get = |column: &str| -> String { try_get_or_warn(row, column, schema, table) };
            let partition_key: Option<String> =
                try_get_or_warn(row, "partition_key", schema, table);
            build_partition_ddl(
                (&get("child_schema"), &get("child_name")),
                (&get("parent_schema"), &get("parent_name")),
                &get("bound"),
                partition_key.as_deref(),
            )
        })
        .collect()
}

/// `CREATE TABLE "s"."child" PARTITION OF "s"."parent" {bound}[ PARTITION BY {key}];`
fn build_partition_ddl(
    (child_schema, child): (&str, &str),
    (parent_schema, parent): (&str, &str),
    bound: &str,
    partition_key: Option<&str>,
) -> Result<String, AppError> {
    let sub_partition = partition_key
        .map(|key| format!(" PARTITION BY {key}"))
        .unwrap_or_default();
    Ok(format!(
        "CREATE TABLE {}.{} PARTITION OF {}.{} {bound}{sub_partition};",
        quote_pg_identifier(child_schema)?,
        quote_pg_identifier(child)?,
        quote_pg_identifier(parent_schema)?,
        quote_pg_identifier(parent)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_partition_of_ddl() {
        assert_eq!(
            build_partition_ddl(
                ("a", "logs_2026"),
                ("a", "logs"),
                "FOR VALUES FROM ('2026-01-01') TO ('2027-01-01')",
                None
            )
            .unwrap(),
            r#"CREATE TABLE "a"."logs_2026" PARTITION OF "a"."logs" FOR VALUES FROM ('2026-01-01') TO ('2027-01-01');"#
        );
        // 하위 파티션이 다시 파티션된 경우
        assert_eq!(
            build_partition_ddl(
                ("a", "logs_2027"),
                ("a", "logs"),
                "FOR VALUES FROM ('2027-01-01') TO ('2028-01-01')",
                Some("LIST (id)")
            )
            .unwrap(),
            r#"CREATE TABLE "a"."logs_2027" PARTITION OF "a"."logs" FOR VALUES FROM ('2027-01-01') TO ('2028-01-01') PARTITION BY LIST (id);"#
        );
        assert!(build_partition_ddl(("a", "x;y"), ("a", "logs"), "DEFAULT", None).is_err());
    }
}
