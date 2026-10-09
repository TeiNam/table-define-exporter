//! PostgreSQL 스키마 수준 객체(사용자 타입·시퀀스)의 생성문.
//!
//! 테이블 DDL 이 참조하는 enum / 도메인 / 복합 타입과 시퀀스(`DEFAULT nextval(..)`)는
//! 테이블보다 먼저 만들어져 있어야 SQL 파일을 빈 DB 에 그대로 실행할 수 있다.
//! pg_dump 처럼 스키마 파일 맨 앞에 출력한다.

use std::collections::{BTreeMap, BTreeSet, HashMap};

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

/// 스키마의 사용자 타입·시퀀스 생성문. 시퀀스를 먼저(도메인 기본값이 `nextval` 을 쓸 수 있음),
/// 그다음 enum / 도메인 / 복합 / range 타입을 `pg_depend` 의 의존 순서로 낸다 (같은 단계는 OID 순).
/// OID 순만으로는 `ALTER TYPE c ADD ATTRIBUTE .. e` 처럼 나중에 만든 타입을 참조하게 된 경우가 깨진다.
///
/// identity 컬럼의 내부 시퀀스는 `GENERATED ... AS IDENTITY` 가 만들므로 제외한다.
// ponytail: base(C) 타입·range 의 subtype_opclass/canonical/subtype_diff·타입 코멘트·권한은 생략.
pub(super) async fn fetch_schema_ddl(pool: &PgPool, schema: &str) -> Result<Vec<String>, AppError> {
    const LABEL: &str = "schema objects";
    let query_err = |e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: LABEL.to_string(),
        source: e,
    };
    // 객체 하나(예: 이름에 위험 문자가 든 타입)가 실패해도 나머지 타입·시퀀스는 내보낸다.
    let skip = |kind: &str, name: &str, e: AppError| {
        tracing::warn!("{schema}.{name} ({kind}) 생성문 생략: {e}");
    };
    let mut types: Vec<(i64, String)> = Vec::new();
    let mut add_type =
        |oid: i64, kind: &str, name: &str, result: Result<String, AppError>| match result {
            Ok(statement) => types.push((oid, statement)),
            Err(e) => skip(kind, name, e),
        };

    // enum — 라벨이 없는 `ENUM ()` 도 포함
    let enums = sqlx::query(
        "SELECT t.oid::int8 AS oid, t.typname::text AS name, \
                COALESCE(array_agg(e.enumlabel::text ORDER BY e.enumsortorder) \
                         FILTER (WHERE e.enumlabel IS NOT NULL), '{}') AS labels \
         FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
         LEFT JOIN pg_catalog.pg_enum e ON e.enumtypid = t.oid \
         WHERE n.nspname = $1 AND t.typtype = 'e' \
         GROUP BY t.oid, t.typname",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(query_err)?;
    for row in &enums {
        let oid: i64 = try_get_or_warn(row, "oid", schema, LABEL);
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let labels: Vec<String> = try_get_or_warn(row, "labels", schema, LABEL);
        add_type(oid, "enum", &name, build_enum_ddl(schema, &name, &labels));
    }

    // 도메인 — 기본값은 typdefault(이름 변경 등이 반영되지 않는 텍스트) 대신 typdefaultbin 을 역변환
    let domains = sqlx::query(
        "SELECT t.oid::int8 AS oid, t.typname::text AS name, \
                format_type(t.typbasetype, t.typtypmod) AS base_type, \
                CASE WHEN t.typcollation <> 0 AND t.typcollation <> bt.typcollation \
                     THEN format('%I.%I', cn.nspname, co.collname) END AS collation, \
                pg_get_expr(t.typdefaultbin, 'pg_catalog.pg_type'::regclass) AS default_value, \
                t.typnotnull AS not_null, \
                (SELECT array_agg(c.conname::text ORDER BY c.conname) \
                   FROM pg_catalog.pg_constraint c \
                  WHERE c.contypid = t.oid AND c.contype = 'c') AS check_names, \
                (SELECT array_agg(pg_get_constraintdef(c.oid) ORDER BY c.conname) \
                   FROM pg_catalog.pg_constraint c \
                  WHERE c.contypid = t.oid AND c.contype = 'c') AS check_defs \
         FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
         JOIN pg_catalog.pg_type bt ON bt.oid = t.typbasetype \
         LEFT JOIN pg_catalog.pg_collation co ON co.oid = t.typcollation \
         LEFT JOIN pg_catalog.pg_namespace cn ON cn.oid = co.collnamespace \
         WHERE n.nspname = $1 AND t.typtype = 'd'",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(query_err)?;
    for row in &domains {
        let oid: i64 = try_get_or_warn(row, "oid", schema, LABEL);
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let base_type: String = try_get_or_warn(row, "base_type", schema, LABEL);
        let collation: Option<String> = try_get_or_warn(row, "collation", schema, LABEL);
        let default_value: Option<String> = try_get_or_warn(row, "default_value", schema, LABEL);
        let not_null: bool = try_get_or_warn(row, "not_null", schema, LABEL);
        let check_names: Option<Vec<String>> = try_get_or_warn(row, "check_names", schema, LABEL);
        let check_defs: Option<Vec<String>> = try_get_or_warn(row, "check_defs", schema, LABEL);
        let checks: Vec<(String, String)> = check_names
            .unwrap_or_default()
            .into_iter()
            .zip(check_defs.unwrap_or_default())
            .collect();
        let domain = DomainDef {
            base_type: &base_type,
            collation: collation.as_deref(),
            default_value: default_value.as_deref(),
            not_null,
            checks: &checks,
        };
        add_type(
            oid,
            "domain",
            &name,
            build_domain_ddl(schema, &name, &domain),
        );
    }

    // 복합 타입 — 속성 타입에 기본값과 다른 COLLATE 를 붙인다
    let composites = sqlx::query(
        "SELECT t.oid::int8 AS oid, t.typname::text AS name, \
                (SELECT array_agg(a.attname::text ORDER BY a.attnum) \
                   FROM pg_catalog.pg_attribute a \
                  WHERE a.attrelid = t.typrelid AND a.attnum > 0 AND NOT a.attisdropped) \
                  AS attr_names, \
                (SELECT array_agg(format_type(a.atttypid, a.atttypmod) || \
                          CASE WHEN a.attcollation <> 0 AND a.attcollation <> at.typcollation \
                               THEN ' COLLATE ' || format('%I.%I', cn.nspname, co.collname) \
                               ELSE '' END \
                          ORDER BY a.attnum) \
                   FROM pg_catalog.pg_attribute a \
                   JOIN pg_catalog.pg_type at ON at.oid = a.atttypid \
                   LEFT JOIN pg_catalog.pg_collation co ON co.oid = a.attcollation \
                   LEFT JOIN pg_catalog.pg_namespace cn ON cn.oid = co.collnamespace \
                  WHERE a.attrelid = t.typrelid AND a.attnum > 0 AND NOT a.attisdropped) \
                  AS attr_types \
         FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
         JOIN pg_catalog.pg_class cl ON cl.oid = t.typrelid \
         WHERE n.nspname = $1 AND t.typtype = 'c' AND cl.relkind = 'c'",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(query_err)?;
    for row in &composites {
        let oid: i64 = try_get_or_warn(row, "oid", schema, LABEL);
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let names: Option<Vec<String>> = try_get_or_warn(row, "attr_names", schema, LABEL);
        let types_: Option<Vec<String>> = try_get_or_warn(row, "attr_types", schema, LABEL);
        let attrs: Vec<(String, String)> = names
            .unwrap_or_default()
            .into_iter()
            .zip(types_.unwrap_or_default())
            .collect();
        add_type(
            oid,
            "composite type",
            &name,
            build_composite_ddl(schema, &name, &attrs),
        );
    }

    // range 타입 — multirange 이름(PG 14+)은 컬럼이 없는 PG 13 에서도 쿼리가 깨지지 않게 jsonb 로 읽는다
    let ranges = sqlx::query(
        "SELECT t.oid::int8 AS oid, t.typname::text AS name, \
                format_type(r.rngsubtype, NULL) AS subtype, \
                mn.nspname::text AS multirange_schema, mt.typname::text AS multirange_name \
         FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
         JOIN pg_catalog.pg_range r ON r.rngtypid = t.oid \
         LEFT JOIN pg_catalog.pg_type mt ON mt.oid = (to_jsonb(r) ->> 'rngmultitypid')::oid \
         LEFT JOIN pg_catalog.pg_namespace mn ON mn.oid = mt.typnamespace \
         WHERE n.nspname = $1 AND t.typtype = 'r'",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .map_err(query_err)?;
    for row in &ranges {
        let oid: i64 = try_get_or_warn(row, "oid", schema, LABEL);
        let name: String = try_get_or_warn(row, "name", schema, LABEL);
        let subtype: String = try_get_or_warn(row, "subtype", schema, LABEL);
        let mr_schema: Option<String> = try_get_or_warn(row, "multirange_schema", schema, LABEL);
        let mr_name: Option<String> = try_get_or_warn(row, "multirange_name", schema, LABEL);
        let multirange = mr_schema.zip(mr_name);
        let ddl = build_range_ddl(schema, &name, &subtype, multirange.as_ref());
        add_type(oid, "range type", &name, ddl);
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
    let mut statements = Vec::new();
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
        match build_sequence_ddl(schema, &name, &sequence) {
            Ok(statement) => statements.push(statement),
            Err(e) => skip("sequence", &name, e),
        }
    }

    // 타입 간 의존 (도메인의 기반·기본값·CHECK, 복합 타입 속성, range 의 subtype). 배열 타입은 원소 타입으로.
    // 조회에 실패해도 타입은 내보낸다 — OID 순으로 대신한다.
    let deps = sqlx::query_as::<_, (i64, i64)>(
        "SELECT t.oid::int8, \
                (CASE WHEN rt.typcategory = 'A' AND rt.typelem <> 0 \
                      THEN rt.typelem ELSE rt.oid END)::int8 \
         FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
         JOIN pg_catalog.pg_depend d \
           ON (d.classid = 'pg_catalog.pg_type'::regclass AND d.objid = t.oid) \
           OR (d.classid = 'pg_catalog.pg_class'::regclass AND d.objid = t.typrelid) \
           OR (d.classid = 'pg_catalog.pg_constraint'::regclass AND d.objid IN ( \
                 SELECT c.oid FROM pg_catalog.pg_constraint c WHERE c.contypid = t.oid)) \
         JOIN pg_catalog.pg_type rt \
           ON d.refclassid = 'pg_catalog.pg_type'::regclass AND rt.oid = d.refobjid \
         WHERE n.nspname = $1 AND t.typtype IN ('e', 'd', 'c', 'r')",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("{schema} 타입 의존 관계 조회 실패 — 생성(OID) 순으로 출력: {e}");
        Vec::new()
    });
    statements.extend(order_by_dependency(types, &deps));
    Ok(statements)
}

/// `(oid, 생성문)` 을 의존 순서로 — `deps` 의 `(t, d)` 는 t 가 d 를 쓴다는 뜻. 먼저 만들 수 있는
/// 것 중 OID 가 작은 것부터 (Kahn). PostgreSQL 은 타입 순환을 막지만, 남으면 OID 순으로 붙인다.
fn order_by_dependency(types: Vec<(i64, String)>, deps: &[(i64, i64)]) -> Vec<String> {
    let mut statements: BTreeMap<i64, String> = types.into_iter().collect();
    let mut dependents: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut pending: HashMap<i64, usize> = HashMap::new();
    for &(t, d) in deps {
        if t != d && statements.contains_key(&t) && statements.contains_key(&d) {
            dependents.entry(d).or_default().push(t);
            *pending.entry(t).or_default() += 1;
        }
    }
    let mut ready: BTreeSet<i64> = statements
        .keys()
        .copied()
        .filter(|t| !pending.contains_key(t))
        .collect();
    let mut ordered = Vec::with_capacity(statements.len());
    while let Some(t) = ready.pop_first() {
        if let Some(statement) = statements.remove(&t) {
            ordered.push(statement);
        }
        for &u in dependents.get(&t).into_iter().flatten() {
            if let Some(n) = pending.get_mut(&u) {
                *n -= 1;
                if *n == 0 {
                    ready.insert(u);
                }
            }
        }
    }
    ordered.extend(statements.into_values());
    ordered
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

/// 도메인 정의 (`CREATE DOMAIN` 의 구성 요소)
struct DomainDef<'a> {
    base_type: &'a str,
    collation: Option<&'a str>,
    default_value: Option<&'a str>,
    not_null: bool,
    checks: &'a [(String, String)],
}

/// `CREATE DOMAIN ..;` — 아직 검증하지 않은(NOT VALID) CHECK 는 CREATE DOMAIN 에 쓸 수 없어
/// (구문 오류) 바로 뒤의 `ALTER DOMAIN .. ADD CONSTRAINT .. NOT VALID;` 로 그 상태 그대로 추가한다.
fn build_domain_ddl(schema: &str, name: &str, domain: &DomainDef) -> Result<String, AppError> {
    let target = qualified(schema, name)?;
    let mut ddl = format!("CREATE DOMAIN {target} AS {}", domain.base_type);
    if let Some(collation) = domain.collation {
        ddl.push_str(&format!(" COLLATE {collation}"));
    }
    if let Some(default) = domain.default_value {
        ddl.push_str(&format!(" DEFAULT {default}"));
    }
    if domain.not_null {
        ddl.push_str(" NOT NULL");
    }
    let mut not_valid = Vec::new();
    for (check_name, definition) in domain.checks {
        let constraint = format!(
            "CONSTRAINT {} {definition}",
            quote_pg_identifier(check_name)?
        );
        if definition.ends_with(" NOT VALID") {
            not_valid.push(format!("ALTER DOMAIN {target} ADD {constraint};"));
        } else {
            ddl.push_str(&format!(" {constraint}"));
        }
    }
    ddl.push(';');
    for statement in not_valid {
        ddl.push('\n');
        ddl.push_str(&statement);
    }
    Ok(ddl)
}

/// `CREATE TYPE "s"."r" AS RANGE (SUBTYPE = .. [, MULTIRANGE_TYPE_NAME = "s"."rm"]);`
fn build_range_ddl(
    schema: &str,
    name: &str,
    subtype: &str,
    multirange: Option<&(String, String)>,
) -> Result<String, AppError> {
    let multirange = match multirange {
        Some((mr_schema, mr_name)) => {
            format!(
                ", MULTIRANGE_TYPE_NAME = {}",
                qualified(mr_schema, mr_name)?
            )
        }
        None => String::new(),
    };
    Ok(format!(
        "CREATE TYPE {} AS RANGE (SUBTYPE = {subtype}{multirange});",
        qualified(schema, name)?
    ))
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
    fn types_follow_dependencies_then_oid() {
        // 10: 복합 c(속성 e), 20: enum e, 30: 도메인 d(기반 c) — OID 순이면 c 가 e 보다 먼저라 실패
        let types = vec![
            (10, "c".to_string()),
            (20, "e".to_string()),
            (30, "d".to_string()),
            (40, "free".to_string()),
        ];
        let deps = [(10, 20), (10, 20), (30, 10), (30, 999), (40, 40)];
        assert_eq!(order_by_dependency(types, &deps), ["e", "c", "d", "free"]);
    }

    #[test]
    fn builds_schema_object_ddl() {
        assert_eq!(
            build_enum_ddl("a", "mood", &["ok".into(), "it's".into()]).unwrap(),
            r#"CREATE TYPE "a"."mood" AS ENUM ('ok', 'it''s');"#
        );
        let checks = [(
            "pos_int_check".to_string(),
            "CHECK ((VALUE > 0))".to_string(),
        )];
        let domain = DomainDef {
            base_type: "integer",
            collation: None,
            default_value: Some("1"),
            not_null: true,
            checks: &checks,
        };
        assert_eq!(
            build_domain_ddl("a", "pos_int", &domain).unwrap(),
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
        let collated = DomainDef {
            base_type: "text",
            collation: Some("pg_catalog.\"C\""),
            default_value: None,
            not_null: false,
            checks: &[],
        };
        assert_eq!(
            build_domain_ddl("a", "code", &collated).unwrap(),
            r#"CREATE DOMAIN "a"."code" AS text COLLATE pg_catalog."C";"#
        );
        // NOT VALID CHECK 는 CREATE DOMAIN 뒤 ALTER DOMAIN 으로 (CREATE DOMAIN 에 쓰면 구문 오류)
        let not_valid = [
            ("pos".to_string(), "CHECK ((VALUE > 0))".to_string()),
            (
                "small".to_string(),
                "CHECK ((VALUE < 100)) NOT VALID".to_string(),
            ),
        ];
        let partly_valid = DomainDef {
            base_type: "integer",
            collation: None,
            default_value: None,
            not_null: false,
            checks: &not_valid,
        };
        assert_eq!(
            build_domain_ddl("a", "n", &partly_valid).unwrap(),
            "CREATE DOMAIN \"a\".\"n\" AS integer CONSTRAINT \"pos\" CHECK ((VALUE > 0));\n\
             ALTER DOMAIN \"a\".\"n\" ADD CONSTRAINT \"small\" CHECK ((VALUE < 100)) NOT VALID;"
        );
        assert_eq!(
            build_enum_ddl("a", "empty", &[]).unwrap(),
            r#"CREATE TYPE "a"."empty" AS ENUM ();"#
        );
        assert_eq!(
            build_range_ddl(
                "a",
                "price_range",
                "numeric",
                Some(&("a".into(), "price_multirange".into()))
            )
            .unwrap(),
            r#"CREATE TYPE "a"."price_range" AS RANGE (SUBTYPE = numeric, MULTIRANGE_TYPE_NAME = "a"."price_multirange");"#
        );
        // 위험 식별자는 거부
        assert!(build_enum_ddl("a;b", "mood", &[]).is_err());
    }
}
