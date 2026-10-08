//! PostgreSQL 스키마 수준 객체(사용자 타입·시퀀스)의 생성문.
//!
//! 테이블 DDL 이 참조하는 enum / 도메인 / 복합 타입과 시퀀스(`DEFAULT nextval(..)`)는
//! 테이블보다 먼저 만들어져 있어야 SQL 파일을 빈 DB 에 그대로 실행할 수 있다.
//! pg_dump 처럼 스키마 파일 맨 앞에 출력한다.

use sqlx::PgPool;

use crate::{db::try_get_or_warn, error::AppError, identifier::quote_pg_identifier};

/// 시퀀스 옵션 (`pg_sequence`)
#[derive(Debug, Clone, PartialEq)]
pub struct PgSequence {
    pub data_type: String,
    pub start: i64,
    pub increment: i64,
    pub min: i64,
    pub max: i64,
    pub cache: i64,
    pub cycle: bool,
}

/// 스키마의 사용자 타입·시퀀스 생성문 (enum → 도메인 → 복합 타입 → 시퀀스, 각각 생성 순).
///
/// identity 컬럼의 내부 시퀀스는 `GENERATED ... AS IDENTITY` 가 만들므로 제외한다.
// ponytail: range/base(C) 타입, 타입 코멘트·권한은 생략 — 쓰는 스키마가 생기면 같은 방식으로 추가.
pub(super) async fn fetch_schema_ddl(pool: &PgPool, schema: &str) -> Result<Vec<String>, AppError> {
    const LABEL: &str = "schema objects";
    let query_err = |e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: LABEL.to_string(),
        source: e,
    };
    let mut statements = Vec::new();
    // 객체 하나(예: 이름에 위험 문자가 든 타입)가 실패해도 나머지 타입·시퀀스는 내보낸다.
    // 예전엔 `?` 로 누적 결과 전체를 버려, 정상 enum 을 쓰는 테이블까지 실행할 수 없었다.
    let mut push = |kind: &str, name: &str, result: Result<String, AppError>| match result {
        Ok(statement) => statements.push(statement),
        Err(e) => tracing::warn!("{schema}.{name} ({kind}) 생성문 생략: {e}"),
    };

    let enums = sqlx::query(
        "SELECT t.typname::text AS name, \
                array_agg(e.enumlabel::text ORDER BY e.enumsortorder) AS labels \
         FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
         JOIN pg_catalog.pg_enum e ON e.enumtypid = t.oid \
         WHERE n.nspname = $1 AND t.typtype = 'e' \
         GROUP BY t.oid, t.typname \
         ORDER BY t.oid",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(query_err)?;
    for row in &enums {
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let labels: Vec<String> = try_get_or_warn(row, "labels", schema, LABEL);
        push("enum", &name, build_enum_ddl(schema, &name, &labels));
    }

    let domains = sqlx::query(
        "SELECT t.typname::text AS name, \
                format_type(t.typbasetype, t.typtypmod) AS base_type, \
                t.typdefault AS default_value, \
                t.typnotnull AS not_null, \
                (SELECT array_agg(c.conname::text ORDER BY c.conname) \
                   FROM pg_catalog.pg_constraint c \
                  WHERE c.contypid = t.oid AND c.contype = 'c') AS check_names, \
                (SELECT array_agg(pg_get_constraintdef(c.oid) ORDER BY c.conname) \
                   FROM pg_catalog.pg_constraint c \
                  WHERE c.contypid = t.oid AND c.contype = 'c') AS check_defs \
         FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
         WHERE n.nspname = $1 AND t.typtype = 'd' \
         ORDER BY t.oid",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(query_err)?;
    for row in &domains {
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let base_type: String = try_get_or_warn(row, "base_type", schema, LABEL);
        let default_value: Option<String> = try_get_or_warn(row, "default_value", schema, LABEL);
        let not_null: bool = try_get_or_warn(row, "not_null", schema, LABEL);
        let check_names: Option<Vec<String>> = try_get_or_warn(row, "check_names", schema, LABEL);
        let check_defs: Option<Vec<String>> = try_get_or_warn(row, "check_defs", schema, LABEL);
        let checks: Vec<(String, String)> = check_names
            .unwrap_or_default()
            .into_iter()
            .zip(check_defs.unwrap_or_default())
            .collect();
        let ddl = build_domain_ddl(
            schema,
            &name,
            &base_type,
            default_value.as_deref(),
            not_null,
            &checks,
        );
        push("domain", &name, ddl);
    }

    let composites = sqlx::query(
        "SELECT t.typname::text AS name, \
                (SELECT array_agg(a.attname::text ORDER BY a.attnum) \
                   FROM pg_catalog.pg_attribute a \
                  WHERE a.attrelid = t.typrelid AND a.attnum > 0 AND NOT a.attisdropped) \
                  AS attr_names, \
                (SELECT array_agg(format_type(a.atttypid, a.atttypmod) ORDER BY a.attnum) \
                   FROM pg_catalog.pg_attribute a \
                  WHERE a.attrelid = t.typrelid AND a.attnum > 0 AND NOT a.attisdropped) \
                  AS attr_types \
         FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
         JOIN pg_catalog.pg_class cl ON cl.oid = t.typrelid \
         WHERE n.nspname = $1 AND t.typtype = 'c' AND cl.relkind = 'c' \
         ORDER BY t.oid",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(query_err)?;
    for row in &composites {
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let names: Option<Vec<String>> = try_get_or_warn(row, "attr_names", schema, LABEL);
        let types: Option<Vec<String>> = try_get_or_warn(row, "attr_types", schema, LABEL);
        let attrs: Vec<(String, String)> = names
            .unwrap_or_default()
            .into_iter()
            .zip(types.unwrap_or_default())
            .collect();
        push(
            "composite type",
            &name,
            build_composite_ddl(schema, &name, &attrs),
        );
    }

    let sequences = sqlx::query(
        "SELECT c.relname::text AS name, \
                format_type(s.seqtypid, NULL) AS data_type, \
                s.seqstart, s.seqincrement, s.seqmin, s.seqmax, s.seqcache, s.seqcycle \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_sequence s ON s.seqrelid = c.oid \
         WHERE n.nspname = $1 AND c.relkind = 'S' \
           AND NOT EXISTS ( \
               SELECT 1 FROM pg_catalog.pg_depend d \
               WHERE d.classid = 'pg_catalog.pg_class'::regclass \
                 AND d.objid = c.oid AND d.deptype = 'i' \
           ) \
         ORDER BY c.relname",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(query_err)?;
    for row in &sequences {
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let sequence = PgSequence {
            data_type: try_get_or_warn(row, "data_type", schema, LABEL),
            start: try_get_or_warn(row, "seqstart", schema, LABEL),
            increment: try_get_or_warn(row, "seqincrement", schema, LABEL),
            min: try_get_or_warn(row, "seqmin", schema, LABEL),
            max: try_get_or_warn(row, "seqmax", schema, LABEL),
            cache: try_get_or_warn(row, "seqcache", schema, LABEL),
            cycle: try_get_or_warn(row, "seqcycle", schema, LABEL),
        };
        push(
            "sequence",
            &name,
            build_sequence_ddl(schema, &name, &sequence),
        );
    }

    Ok(statements)
}

/// 테이블 컬럼이 소유한(serial) 시퀀스의 `ALTER SEQUENCE ... OWNED BY ...;`.
///
/// 시퀀스는 스키마 파일 앞에서 먼저 만들어지므로, 테이블 생성 직후 소유 관계를 복원한다
/// (테이블을 DROP 하면 시퀀스도 함께 지워지는 serial 동작 유지).
pub(super) async fn fetch_sequence_ownership(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, AppError> {
    let rows = sqlx::query(
        "SELECT seq_ns.nspname::text AS seq_schema, seq.relname::text AS seq_name, \
                a.attname::text AS column_name \
         FROM pg_catalog.pg_depend d \
         JOIN pg_catalog.pg_class seq ON seq.oid = d.objid AND seq.relkind = 'S' \
         JOIN pg_catalog.pg_namespace seq_ns ON seq_ns.oid = seq.relnamespace \
         JOIN pg_catalog.pg_class tbl ON tbl.oid = d.refobjid \
         JOIN pg_catalog.pg_namespace tbl_ns ON tbl_ns.oid = tbl.relnamespace \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = tbl.oid AND a.attnum = d.refobjsubid \
         WHERE d.classid = 'pg_catalog.pg_class'::regclass \
           AND d.refclassid = 'pg_catalog.pg_class'::regclass \
           AND d.deptype = 'a' \
           AND tbl_ns.nspname = $1 AND tbl.relname = $2 \
         ORDER BY a.attnum",
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

    let quoted_table = format!(
        "{}.{}",
        quote_pg_identifier(schema)?,
        quote_pg_identifier(table)?
    );
    rows.iter()
        .map(|row| {
            let seq_schema: String = try_get_or_warn(row, "seq_schema", schema, table);
            let seq_name: String = try_get_or_warn(row, "seq_name", schema, table);
            let column: String = try_get_or_warn(row, "column_name", schema, table);
            Ok(format!(
                "ALTER SEQUENCE {}.{} OWNED BY {quoted_table}.{};",
                quote_pg_identifier(&seq_schema)?,
                quote_pg_identifier(&seq_name)?,
                quote_pg_identifier(&column)?
            ))
        })
        .collect()
}

/// SQL 문자열 리터럴 (`'` → `''`). standard_conforming_strings(기본 on) 기준.
pub(super) fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn qualified(schema: &str, name: &str) -> Result<String, AppError> {
    Ok(format!(
        "{}.{}",
        quote_pg_identifier(schema)?,
        quote_pg_identifier(name)?
    ))
}

fn build_enum_ddl(schema: &str, name: &str, labels: &[String]) -> Result<String, AppError> {
    let labels: Vec<String> = labels.iter().map(|l| quote_literal(l)).collect();
    Ok(format!(
        "CREATE TYPE {} AS ENUM ({});",
        qualified(schema, name)?,
        labels.join(", ")
    ))
}

fn build_domain_ddl(
    schema: &str,
    name: &str,
    base_type: &str,
    default_value: Option<&str>,
    not_null: bool,
    checks: &[(String, String)],
) -> Result<String, AppError> {
    let mut ddl = format!("CREATE DOMAIN {} AS {base_type}", qualified(schema, name)?);
    if let Some(default) = default_value {
        ddl.push_str(&format!(" DEFAULT {default}"));
    }
    if not_null {
        ddl.push_str(" NOT NULL");
    }
    for (check_name, definition) in checks {
        ddl.push_str(&format!(
            " CONSTRAINT {} {definition}",
            quote_pg_identifier(check_name)?
        ));
    }
    ddl.push(';');
    Ok(ddl)
}

fn build_composite_ddl(
    schema: &str,
    name: &str,
    attrs: &[(String, String)],
) -> Result<String, AppError> {
    let attrs = attrs
        .iter()
        .map(|(attr, data_type)| Ok(format!("{} {data_type}", quote_pg_identifier(attr)?)))
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(format!(
        "CREATE TYPE {} AS ({});",
        qualified(schema, name)?,
        attrs.join(", ")
    ))
}

fn build_sequence_ddl(schema: &str, name: &str, seq: &PgSequence) -> Result<String, AppError> {
    let cycle = if seq.cycle { " CYCLE" } else { "" };
    Ok(format!(
        "CREATE SEQUENCE {} AS {} START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {} CACHE {}{cycle};",
        qualified(schema, name)?,
        seq.data_type,
        seq.start,
        seq.increment,
        seq.min,
        seq.max,
        seq.cache
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_schema_object_ddl() {
        assert_eq!(
            build_enum_ddl("a", "mood", &["ok".into(), "it's".into()]).unwrap(),
            r#"CREATE TYPE "a"."mood" AS ENUM ('ok', 'it''s');"#
        );
        assert_eq!(
            build_domain_ddl(
                "a",
                "pos_int",
                "integer",
                Some("1"),
                true,
                &[("pos_int_check".into(), "CHECK ((VALUE > 0))".into())]
            )
            .unwrap(),
            r#"CREATE DOMAIN "a"."pos_int" AS integer DEFAULT 1 NOT NULL CONSTRAINT "pos_int_check" CHECK ((VALUE > 0));"#
        );
        assert_eq!(
            build_composite_ddl(
                "a",
                "addr",
                &[
                    ("street".into(), "text".into()),
                    ("zip".into(), "character varying(10)".into())
                ]
            )
            .unwrap(),
            r#"CREATE TYPE "a"."addr" AS ("street" text, "zip" character varying(10));"#
        );
        let seq = PgSequence {
            data_type: "integer".into(),
            start: 1000,
            increment: 10,
            min: 1,
            max: 2147483647,
            cache: 1,
            cycle: true,
        };
        assert_eq!(
            build_sequence_ddl("a", "s", &seq).unwrap(),
            r#"CREATE SEQUENCE "a"."s" AS integer START WITH 1000 INCREMENT BY 10 MINVALUE 1 MAXVALUE 2147483647 CACHE 1 CYCLE;"#
        );
        // 위험 식별자는 거부
        assert!(build_enum_ddl("a;b", "mood", &[]).is_err());
    }
}
