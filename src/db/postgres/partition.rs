//! PostgreSQL 선언적 파티션의 하위 파티션 DDL.
//!
//! 파티션 부모만 정의서·SQL 목록에 남기고(MySQL 처럼 테이블 하나), 하위 파티션은 부모 DDL
//! 뒤에 `CREATE TABLE child PARTITION OF parent FOR VALUES ...;` 로 붙인다. 컬럼·PK·FK 는
//! 부모에서 상속되므로 다시 쓰지 않고, 하위 파티션에만 따로 만든 제약·코멘트만 덧붙인다.
//! 인덱스는 파티션을 모두 만든 뒤 [`order_tree_indexes`] 순서로 만들어 원본 이름을 그대로 쓴다.
//! 외부 테이블 파티션은 `CREATE FOREIGN TABLE .. PARTITION OF .. SERVER ..` 로 쓴다.
// ponytail: 하위 파티션에만 다른 컬럼 기본값과 PG 17 이하의 하위 파티션 전용 NOT NULL 은 생략
// (PG 18+ 은 NOT NULL 도 제약이라 함께 나온다) — 필요해지면 ALTER COLUMN 으로 추가.

use std::cmp::Reverse;
use std::collections::HashMap;

use sqlx::PgPool;

use super::foreign::{build_foreign_suffix, fetch_column_options_ddl};
use super::parse::without_on_only;
use crate::{db::try_get_or_warn, error::AppError, identifier::quote_pg_identifier};

/// 하위 파티션 DDL — 부모 DDL 바로 뒤에 둘 문장(`create`, 그다음 `indexes`)과, FK 처럼 모든
/// 테이블 뒤에 둘 문장 (다른 스키마를 참조하는 FK 는 `cross_schema` — [`crate::model::TableDdl::cross_schema`])
#[derive(Debug, Default)]
pub(super) struct PartitionDdl {
    pub create: Vec<String>,
    /// 부모·하위 파티션의 인덱스 전부 (파티션을 모두 만든 뒤 실행)
    pub indexes: Vec<String>,
    pub after: Vec<String>,
    pub cross_schema: Vec<String>,
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
        if foreign.is_some() {
            ddl.create
                .extend(fetch_column_options_ddl(pool, &child_schema, &child).await?);
        }
        let own = fetch_partition_own_objects(pool, schema, &child_schema, &child).await?;
        ddl.create.extend(own.create);
        ddl.after.extend(own.after);
        ddl.cross_schema.extend(own.cross_schema);
        let comments = super::comment::fetch_comment_ddl(pool, &child_schema, &child).await?;
        ddl.create.extend(comments);
    }
    ddl.indexes = fetch_tree_indexes(pool, schema, table).await?;
    Ok(ddl)
}

/// 파티션 트리(부모 포함)의 인덱스 하나 — PK/UNIQUE/EXCLUDE 제약 인덱스는 제약이 만든다
#[derive(Debug)]
struct TreeIndex {
    oid: i64,
    /// 인덱스가 걸린 테이블의 파티션 트리 단계 (부모 0)
    level: i32,
    /// 이 인덱스가 붙어 있는(pg_inherits) 상위 파티션의 인덱스
    parent: Option<i64>,
    definition: String,
}

async fn fetch_tree_indexes(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, AppError> {
    let rows = sqlx::query(
        "SELECT i.indexrelid::int8 AS oid, t.level::int4 AS level, \
                (SELECT h.inhparent::int8 FROM pg_catalog.pg_inherits h \
                  WHERE h.inhrelid = i.indexrelid LIMIT 1) AS parent, \
                pg_get_indexdef(i.indexrelid) AS definition \
         FROM pg_partition_tree(format('%I.%I', $1, $2)::regclass) t \
         JOIN pg_catalog.pg_index i ON i.indrelid = t.relid \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM pg_catalog.pg_constraint con \
             WHERE con.conindid = i.indexrelid AND con.contype IN ('p', 'u', 'x') \
         )",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(query_error(schema, table))?;
    let indexes = rows
        .iter()
        .map(|row| TreeIndex {
            oid: try_get_or_warn(row, "oid", schema, table),
            level: try_get_or_warn(row, "level", schema, table),
            parent: try_get_or_warn(row, "parent", schema, table),
            definition: try_get_or_warn(row, "definition", schema, table),
        })
        .collect();
    Ok(order_tree_indexes(indexes))
}

/// 파티션 트리 인덱스의 생성 순서와 문장.
///
/// 파티션된 테이블의 인덱스는 `ON ONLY` 없이(재귀로) 만든다 — 그러면 PostgreSQL 이 하위 파티션에
/// 정의가 같은 인덱스가 이미 있으면 새로 만들지 않고 붙이므로(attach), 하위 인덱스를 원래 이름으로
/// 먼저 만들어 두면 이름이 그대로 남는다. 외부 테이블 파티션은 건너뛰어 부모 인덱스도 원본처럼
/// 유효하다 (pg_dump 식 `ON ONLY` + `ATTACH PARTITION` 은 외부 파티션이 있으면 무효로 남는다).
/// - 인덱스 트리(pg_inherits)마다 깊은 단계부터 만든다
/// - 트리는 뿌리가 얕은 것부터 — 하위 파티션 자체 인덱스(더 깊은 뿌리)는 위 단계 인덱스를 다 만든
///   뒤에 만들어야 정의가 같은 위 단계 인덱스에 잘못 붙지 않는다
fn order_tree_indexes(indexes: Vec<TreeIndex>) -> Vec<String> {
    let by_oid: HashMap<i64, &TreeIndex> = indexes.iter().map(|i| (i.oid, i)).collect();
    // 뿌리 인덱스의 (단계, oid) — pg_inherits 는 순환하지 않지만 반복은 인덱스 수로 제한
    let root_of = |index: &TreeIndex| -> (i32, i64) {
        let mut root = index;
        for _ in 0..by_oid.len() {
            match root.parent.and_then(|p| by_oid.get(&p)) {
                Some(parent) => root = parent,
                None => break,
            }
        }
        (root.level, root.oid)
    };
    let mut ordered: Vec<&TreeIndex> = indexes.iter().collect();
    ordered.sort_by_cached_key(|index| {
        let (root_level, root_oid) = root_of(index);
        (root_level, root_oid, Reverse(index.level), index.oid)
    });
    ordered
        .into_iter()
        .map(|index| format!("{};", without_on_only(&index.definition)))
        .collect()
}

/// 하위 파티션에만 있는(부모에서 상속되지 않은) 제약. `file_schema` 는 이 DDL 을 쓸
/// 스키마 파일(부모 테이블의 스키마) — 다른 스키마를 참조하는 FK 를 가려낸다.
async fn fetch_partition_own_objects(
    pool: &PgPool,
    file_schema: &str,
    schema: &str,
    table: &str,
) -> Result<PartitionDdl, AppError> {
    let constraints = sqlx::query(
        "SELECT con.conname::text AS name, con.contype::text AS kind, \
                pg_get_constraintdef(con.oid) AS definition, \
                (SELECT n.nspname::text FROM pg_catalog.pg_class c \
                   JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                  WHERE c.oid = con.confrelid) AS ref_schema \
         FROM pg_catalog.pg_constraint con \
         WHERE con.conrelid = format('%I.%I', $1, $2)::regclass \
           AND con.coninhcount = 0 AND con.contype IN ('p', 'u', 'c', 'x', 'f', 'n') \
         ORDER BY con.contype, con.conname",
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
        let ref_schema: Option<String> = try_get_or_warn(row, "ref_schema", schema, table);
        let statement = format!(
            "ALTER TABLE {target} ADD CONSTRAINT {} {definition};",
            quote_pg_identifier(&name)?
        );
        // FK 는 참조 대상이 먼저 있어야 하므로 다른 FK 와 함께 파일 끝으로 (다른 스키마 참조는 별도 파일)
        match (kind.as_str(), ref_schema) {
            ("f", Some(ref_schema)) if ref_schema != file_schema => {
                ddl.cross_schema.push(statement)
            }
            ("f", _) => ddl.after.push(statement),
            _ => ddl.create.push(statement),
        }
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
    fn tree_indexes_attach_children_before_parents_and_own_indexes_last() {
        let index = |oid, level, parent, name: &str, on: &str| TreeIndex {
            oid,
            level,
            parent,
            definition: format!("CREATE INDEX {name} ON {on} USING btree (k)"),
        };
        let order = order_tree_indexes(vec![
            // 부모 p(0) 의 인덱스 1 ← 하위 c(1, 다시 파티션됨) 의 2 ← 말단 c_a(2) 의 3
            index(1, 0, None, "p_k", "ONLY s.p"),
            index(2, 1, Some(1), "renamed", "ONLY s.c"),
            index(3, 2, Some(2), "leaf_renamed", "s.c_a"),
            // c 자체 인덱스 4 ← c_a 의 5, 말단 c_a 자체 인덱스 6
            index(4, 1, None, "c_own", "ONLY s.c"),
            index(5, 2, Some(4), "c_a_from_own", "s.c_a"),
            index(6, 2, None, "c_a_own", "s.c_a"),
        ]);
        assert_eq!(
            order,
            [
                "CREATE INDEX leaf_renamed ON s.c_a USING btree (k);",
                "CREATE INDEX renamed ON s.c USING btree (k);",
                "CREATE INDEX p_k ON s.p USING btree (k);",
                "CREATE INDEX c_a_from_own ON s.c_a USING btree (k);",
                "CREATE INDEX c_own ON s.c USING btree (k);",
                "CREATE INDEX c_a_own ON s.c_a USING btree (k);",
            ]
        );
    }

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
