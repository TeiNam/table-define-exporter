//! PostgreSQL 선언적 파티션의 하위 파티션 DDL.
//!
//! 파티션 부모만 정의서·SQL 목록에 남기고(MySQL 처럼 테이블 하나), 하위 파티션은 부모 DDL
//! 뒤에 `CREATE TABLE child PARTITION OF parent FOR VALUES ...;` 로 붙인다. 컬럼·PK·FK·
//! 인덱스는 부모에서 상속되므로 다시 쓰지 않고, 하위 파티션에만 따로 만든 제약·인덱스·코멘트만
//! 덧붙인다. 외부 테이블 파티션은 `CREATE FOREIGN TABLE .. PARTITION OF .. SERVER ..` 로 쓴다.
// ponytail: 하위 파티션에만 다른 컬럼 기본값·NOT NULL 은 생략 — 필요해지면 ALTER COLUMN 으로 추가.

use sqlx::PgPool;

use super::foreign::build_foreign_suffix;
use crate::{db::try_get_or_warn, error::AppError, identifier::quote_pg_identifier};

/// 하위 파티션 DDL — 부모 DDL 바로 뒤에 둘 문장과, FK 처럼 모든 테이블 뒤에 둘 문장
#[derive(Debug, Default)]
pub(super) struct PartitionDdl {
    pub create: Vec<String>,
    pub after: Vec<String>,
}

/// 부모 테이블의 모든 하위 파티션 생성문 (단계 순 — 하위의 하위 파티션까지).
pub(super) async fn fetch_partitions_ddl(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<PartitionDdl, AppError> {
    let rows = sqlx::query(
        "SELECT cn.nspname::text AS child_schema, c.relname::text AS child_name, \
                pn.nspname::text AS parent_schema, p.relname::text AS parent_name, \
                pg_get_expr(c.relpartbound, c.oid) AS bound, \
                pg_get_partkeydef(c.oid) AS partition_key, \
                fs.srvname::text AS foreign_server, ft.ftoptions AS foreign_options \
         FROM pg_partition_tree(format('%I.%I', $1, $2)::regclass) t \
         JOIN pg_catalog.pg_class c ON c.oid = t.relid \
         JOIN pg_catalog.pg_namespace cn ON cn.oid = c.relnamespace \
         JOIN pg_catalog.pg_class p ON p.oid = t.parentrelid \
         JOIN pg_catalog.pg_namespace pn ON pn.oid = p.relnamespace \
         LEFT JOIN pg_catalog.pg_foreign_table ft ON ft.ftrelid = c.oid \
         LEFT JOIN pg_catalog.pg_foreign_server fs ON fs.oid = ft.ftserver \
         WHERE t.level > 0 \
         ORDER BY t.level, c.relname",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(query_error(schema, table))?;

    let mut ddl = PartitionDdl::default();
    for row in &rows {
        let get = |column: &str| -> String { try_get_or_warn(row, column, schema, table) };
        let (child_schema, child) = (get("child_schema"), get("child_name"));
        let partition_key: Option<String> = try_get_or_warn(row, "partition_key", schema, table);
        let server: Option<String> = try_get_or_warn(row, "foreign_server", schema, table);
        let foreign = match server {
            Some(server) => {
                let options: Option<Vec<String>> =
                    try_get_or_warn(row, "foreign_options", schema, table);
                Some(build_foreign_suffix(&server, &options.unwrap_or_default())?)
            }
            None => None,
        };
        ddl.create.push(build_partition_ddl(
            (&child_schema, &child),
            (&get("parent_schema"), &get("parent_name")),
            &get("bound"),
            partition_key.as_deref(),
            foreign.as_deref(),
        )?);
        let own = fetch_partition_own_objects(pool, &child_schema, &child).await?;
        ddl.create.extend(own.create);
        ddl.after.extend(own.after);
        let comments = super::comment::fetch_comment_ddl(pool, &child_schema, &child).await?;
        ddl.create.extend(comments);
    }
    Ok(ddl)
}

/// 하위 파티션에만 있는(부모에서 상속되지 않은) 제약과 인덱스.
async fn fetch_partition_own_objects(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<PartitionDdl, AppError> {
    let constraints = sqlx::query(
        "SELECT con.conname::text AS name, con.contype::text AS kind, \
                pg_get_constraintdef(con.oid) AS definition \
         FROM pg_catalog.pg_constraint con \
         WHERE con.conrelid = format('%I.%I', $1, $2)::regclass \
           AND con.coninhcount = 0 AND con.contype IN ('p', 'u', 'c', 'x', 'f') \
         ORDER BY con.contype, con.conname",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(query_error(schema, table))?;
    let indexes = sqlx::query(
        "SELECT pg_get_indexdef(i.indexrelid) AS definition \
         FROM pg_catalog.pg_index i \
         WHERE i.indrelid = format('%I.%I', $1, $2)::regclass \
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits h WHERE h.inhrelid = i.indexrelid) \
           AND NOT EXISTS ( \
               SELECT 1 FROM pg_catalog.pg_constraint con \
               WHERE con.conindid = i.indexrelid AND con.contype IN ('p', 'u', 'x') \
           ) \
         ORDER BY i.indexrelid",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(query_error(schema, table))?;

    let target = format!(
        "{}.{}",
        quote_pg_identifier(schema)?,
        quote_pg_identifier(table)?
    );
    let mut ddl = PartitionDdl::default();
    for row in &constraints {
        let name: String = try_get_or_warn(row, "name", schema, table);
        let kind: String = try_get_or_warn(row, "kind", schema, table);
        let definition: String = try_get_or_warn(row, "definition", schema, table);
        let statement = format!(
            "ALTER TABLE {target} ADD CONSTRAINT {} {definition};",
            quote_pg_identifier(&name)?
        );
        // FK 는 참조 대상이 먼저 있어야 하므로 다른 FK 와 함께 파일 끝으로
        if kind == "f" {
            ddl.after.push(statement);
        } else {
            ddl.create.push(statement);
        }
    }
    for row in &indexes {
        let definition: String = try_get_or_warn(row, "definition", schema, table);
        ddl.create.push(format!("{definition};"));
    }
    Ok(ddl)
}

fn query_error<'a>(schema: &'a str, table: &'a str) -> impl Fn(sqlx::Error) -> AppError + 'a {
    move |e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: table.to_string(),
        source: e,
    }
}

/// `CREATE [FOREIGN] TABLE "s"."child" PARTITION OF "s"."parent" {bound}[ PARTITION BY {key}][ SERVER ..];`
fn build_partition_ddl(
    (child_schema, child): (&str, &str),
    (parent_schema, parent): (&str, &str),
    bound: &str,
    partition_key: Option<&str>,
    foreign: Option<&str>,
) -> Result<String, AppError> {
    let keyword = if foreign.is_some() {
        "CREATE FOREIGN TABLE"
    } else {
        "CREATE TABLE"
    };
    let sub_partition = partition_key
        .map(|key| format!(" PARTITION BY {key}"))
        .unwrap_or_default();
    let server = foreign.map(|f| format!(" {f}")).unwrap_or_default();
    Ok(format!(
        "{keyword} {}.{} PARTITION OF {}.{} {bound}{sub_partition}{server};",
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
                None,
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
                Some("LIST (id)"),
                None
            )
            .unwrap(),
            r#"CREATE TABLE "a"."logs_2027" PARTITION OF "a"."logs" FOR VALUES FROM ('2027-01-01') TO ('2028-01-01') PARTITION BY LIST (id);"#
        );
        // 외부 테이블 파티션
        assert_eq!(
            build_partition_ddl(
                ("a", "logs_us"),
                ("a", "logs"),
                "FOR VALUES IN ('us')",
                None,
                Some(r#"SERVER "rs" OPTIONS ("table_name" 'logs_us')"#)
            )
            .unwrap(),
            r#"CREATE FOREIGN TABLE "a"."logs_us" PARTITION OF "a"."logs" FOR VALUES IN ('us') SERVER "rs" OPTIONS ("table_name" 'logs_us');"#
        );
        assert!(build_partition_ddl(("a", "x;y"), ("a", "logs"), "DEFAULT", None, None).is_err());
    }
}
