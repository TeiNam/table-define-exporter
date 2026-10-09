//! PostgreSQL DDL 재구성 전용 모듈.
//!
//! `build_pg_ddl_from_metadata` — 순수 함수로 메타데이터 → CREATE TABLE DDL 문자열을 재구성.
//! `fetch_table_ddl` — `PgPool`에서 컬럼/제약/인덱스 메타데이터를 조회한 뒤
//! `build_pg_ddl_from_metadata`에 위임하는 async 헬퍼.

use crate::{error::AppError, identifier::quote_pg_identifier, model::TableDdl};

use super::parse::{extract_check_expression, quote_column_list, without_on_only};
use super::types::{
    PgConstraintType, PgDdlColumn, PgDdlConstraint, PgGenerated, PgIdentity, PgIdentitySequence,
};
use crate::db::try_get_or_warn;

/// 테이블 메타데이터로부터 PostgreSQL DDL 문자열을 재구성한다.
///
/// 순수 함수로 구현하여 PBT 테스트가 가능하다.
/// `quote_pg_identifier`를 사용하여 스키마/테이블/컬럼 이름을 안전하게 인용한다.
///
/// DDL 구조:
/// ```sql
/// CREATE TABLE "schema"."table" (
///     "col" type [NOT NULL] [GENERATED {ALWAYS|BY DEFAULT} AS IDENTITY [(...)]]
///         [DEFAULT default] [GENERATED ALWAYS AS (expr) STORED|VIRTUAL],
///     CONSTRAINT "pk" PRIMARY KEY (columns),
///     CONSTRAINT "uq" UNIQUE (columns),
///     CONSTRAINT "ck" CHECK (expression)
/// );
/// -- 인덱스
/// indexdef;
/// ```
/// FK 는 여기에 넣지 않는다 — [`build_pg_fk_ddl`] 로 따로 만든다.
pub fn build_pg_ddl_from_metadata(
    schema: &str,
    table: &str,
    columns: &[PgDdlColumn],
    constraints: &[PgDdlConstraint],
    index_defs: &[String],
) -> Result<String, AppError> {
    build_table_ddl(
        schema,
        table,
        columns,
        constraints,
        index_defs,
        &TableOptions::default(),
    )
}

/// 일반 테이블이 아닌 경우의 DDL 옵션
#[derive(Default)]
struct TableOptions<'a> {
    /// 파티션 부모의 `PARTITION BY {key}` 키
    partition_key: Option<&'a str>,
    /// 외부 테이블의 `SERVER .. OPTIONS (..)` 절
    foreign: Option<&'a str>,
}

/// [`build_pg_ddl_from_metadata`] + 파티션 부모(`PARTITION BY`) / 외부 테이블(`CREATE FOREIGN TABLE`).
fn build_table_ddl(
    schema: &str,
    table: &str,
    columns: &[PgDdlColumn],
    constraints: &[PgDdlConstraint],
    index_defs: &[String],
    options: &TableOptions,
) -> Result<String, AppError> {
    let partition_key = options.partition_key;
    let quoted_schema = quote_pg_identifier(schema)?;
    let quoted_table = quote_pg_identifier(table)?;

    let keyword = if options.foreign.is_some() {
        "CREATE FOREIGN TABLE"
    } else {
        "CREATE TABLE"
    };
    let mut ddl = format!("{keyword} {quoted_schema}.{quoted_table} (\n");

    // 컬럼 정의와 제약 조건을 모두 모아서 쉼표로 구분
    let mut entries: Vec<String> = Vec::new();

    // 컬럼 정의 추가
    for col in columns {
        let quoted_col = quote_pg_identifier(&col.name)?;
        let mut col_def = format!("    {quoted_col} {}", col.data_type);

        // NOT NULL — PG 18+ 은 이름 있는 제약(contype 'n')이라 이름·NO INHERIT 까지 살린다.
        // NOT VALID 면 빈 테이블에서 곧바로 검증되지 않게 build_pg_fk_ddl 이 ALTER TABLE 로 추가한다.
        let not_null = constraints.iter().find(|c| {
            matches!(c.constraint_type, PgConstraintType::NotNull)
                && c.columns == [col.name.as_str()]
        });
        match not_null {
            Some(c) if is_not_valid(c) => {}
            Some(c) => col_def.push_str(&not_null_clause(table, &col.name, c)?),
            None if !col.is_nullable => col_def.push_str(" NOT NULL"),
            None => {}
        }

        // identity 컬럼 (identity 와 DEFAULT 는 함께 올 수 없고 column_default 도 NULL)
        if let Some(identity) = &col.identity {
            col_def.push_str(&identity.to_sql()?);
        }

        // GENERATED ALWAYS AS (...) STORED|VIRTUAL (기본값보다 우선)
        match &col.generated {
            Some(PgGenerated::Stored(expr)) => {
                col_def.push_str(&format!(" GENERATED ALWAYS AS ({expr}) STORED"));
            }
            Some(PgGenerated::Virtual(expr)) => {
                col_def.push_str(&format!(" GENERATED ALWAYS AS ({expr}) VIRTUAL"));
            }
            // DEFAULT 값 (generated 컬럼이 아닌 경우에만)
            None => {
                if let Some(default) = &col.default_value {
                    col_def.push_str(&format!(" DEFAULT {default}"));
                }
            }
        }

        entries.push(col_def);
    }

    // 제약 조건 추가 (PK → UQ → CK → EXCLUDE 순서, FK 는 build_pg_fk_ddl)
    let order = |c: &PgDdlConstraint| match c.constraint_type {
        PgConstraintType::PrimaryKey => Some(0),
        PgConstraintType::Unique => Some(1),
        PgConstraintType::Check { .. } => Some(2),
        PgConstraintType::Exclude { .. } => Some(3),
        PgConstraintType::ForeignKey { .. } | PgConstraintType::NotNull => None,
    };
    // NOT VALID 제약은 CREATE TABLE 안에 두면 빈 테이블이라 곧바로 검증돼 상태가 바뀌므로
    // build_pg_fk_ddl 이 ALTER TABLE .. NOT VALID 로 뒤에서 추가한다 (pg_dump 와 동일)
    let mut inline: Vec<&PgDdlConstraint> = constraints
        .iter()
        .filter(|c| order(c).is_some() && !is_not_valid(c))
        .collect();
    inline.sort_by_key(|c| order(c));
    for c in inline {
        entries.push(format!("    {}", constraint_clause(c)?));
    }

    // 엔트리들을 쉼표+개행으로 결합
    ddl.push_str(&entries.join(",\n"));
    match (options.foreign, partition_key) {
        (Some(foreign), _) => ddl.push_str(&format!("\n) {foreign};\n")),
        (None, Some(key)) => ddl.push_str(&format!("\n) PARTITION BY {key};\n")),
        (None, None) => ddl.push_str("\n);\n"),
    }

    // 인덱스 정의 추가. 파티션 부모의 인덱스는 pg_get_indexdef 가 `ON ONLY` 로 돌려준다.
    for idx_def in index_defs {
        let idx_def = match partition_key {
            Some(_) => without_on_only(idx_def),
            None => idx_def.clone(),
        };
        ddl.push_str(&format!("{idx_def};\n"));
    }

    Ok(ddl)
}

/// 컬럼의 ` [CONSTRAINT "name"] NOT NULL[ NO INHERIT]` — 이름이 기본값(`{table}_{col}_not_null`)이면
/// 생략한다 (PG 17 이하에서도 실행되는 열 제약 문법).
fn not_null_clause(table: &str, column: &str, c: &PgDdlConstraint) -> Result<String, AppError> {
    let name = if c.name == format!("{table}_{column}_not_null") {
        String::new()
    } else {
        format!(" CONSTRAINT {}", quote_pg_identifier(&c.name)?)
    };
    let no_inherit = match &c.definition {
        Some(definition) if definition.ends_with(" NO INHERIT") => " NO INHERIT",
        _ => "",
    };
    Ok(format!("{name} NOT NULL{no_inherit}"))
}

/// `NOT VALID` 로 만든(아직 검증하지 않은) 제약인가
fn is_not_valid(c: &PgDdlConstraint) -> bool {
    c.definition
        .as_deref()
        .is_some_and(|d| d.ends_with(" NOT VALID"))
}

/// `CONSTRAINT "name" ...` 절 (PK/UNIQUE/CHECK/EXCLUDE). 카탈로그 원문이 있으면 그대로 쓰고,
/// 이름·컬럼은 원문을 쓸 때도 위험 식별자 정책대로 검증한다.
fn constraint_clause(c: &PgDdlConstraint) -> Result<String, AppError> {
    let quoted_name = quote_pg_identifier(&c.name)?;
    let cols = quote_column_list(&c.columns)?;
    let body = match (&c.definition, &c.constraint_type) {
        (Some(definition), _) => definition.clone(),
        (None, PgConstraintType::PrimaryKey) => format!("PRIMARY KEY ({cols})"),
        (None, PgConstraintType::Unique) => format!("UNIQUE ({cols})"),
        (None, PgConstraintType::Check { expression }) => format!("CHECK ({expression})"),
        (None, PgConstraintType::Exclude { definition }) => definition.clone(),
        (None, PgConstraintType::NotNull) => format!("NOT NULL {cols}"),
        (None, PgConstraintType::ForeignKey { .. }) => {
            unreachable!("FK 는 build_pg_fk_ddl 에서 ALTER TABLE 로 만든다")
        }
    };
    Ok(format!("CONSTRAINT {quoted_name} {body}"))
}

/// 인덱스 정의처럼 `;` 없이 온 문장에 종결자를 붙인다.
fn terminate(statements: &[String]) -> Vec<String> {
    statements.iter().map(|s| format!("{s};")).collect()
}

/// 머티리얼라이즈드 뷰 정의로 `CREATE MATERIALIZED VIEW .. AS .. WITH NO DATA;` 를 만든다.
/// pg_dump 의 스키마 전용 출력과 같이 데이터 없이 만들고, 채우려면 `REFRESH` 를 실행한다.
pub fn build_pg_materialized_view_ddl(
    schema: &str,
    view: &str,
    definition: &str,
    reloptions: &[String],
) -> Result<String, AppError> {
    let quoted_schema = quote_pg_identifier(schema)?;
    let quoted_view = quote_pg_identifier(view)?;
    let body = definition.trim_end().trim_end_matches(';');
    let options: Vec<&String> = reloptions.iter().collect();
    Ok(format!(
        "CREATE MATERIALIZED VIEW {quoted_schema}.{quoted_view}{} AS\n{body}\nWITH NO DATA;\n",
        with_options(&options)
    ))
}

/// DDL 뒤에 문장들을 한 줄씩 붙인다.
fn append_statements(ddl: &mut String, statements: &[String]) {
    for statement in statements {
        ddl.push_str(statement);
        ddl.push('\n');
    }
}

/// FK 와 NOT VALID CHECK 를 `ALTER TABLE ... ADD CONSTRAINT ...;` 문장으로 만든다.
///
/// FK 를 CREATE TABLE 안에 두면 참조 테이블이 먼저 있어야 해서, 테이블명 순으로 출력한
/// SQL 파일을 그대로 실행할 수 없다. pg_dump 처럼 모든 테이블을 만든 뒤 추가한다.
pub fn build_pg_fk_ddl(
    schema: &str,
    table: &str,
    constraints: &[PgDdlConstraint],
) -> Result<Vec<String>, AppError> {
    let quoted_schema = quote_pg_identifier(schema)?;
    let quoted_table = quote_pg_identifier(table)?;
    let mut statements = Vec::new();
    // 아직 검증하지 않은(NOT VALID) CHECK·NOT NULL 은 테이블 생성 뒤에 그 상태 그대로 추가한다
    for c in constraints.iter().filter(|c| {
        matches!(
            c.constraint_type,
            PgConstraintType::Check { .. } | PgConstraintType::NotNull
        ) && is_not_valid(c)
    }) {
        statements.push(format!(
            "ALTER TABLE {quoted_schema}.{quoted_table} ADD {};",
            constraint_clause(c)?
        ));
    }
    for c in constraints {
        let PgConstraintType::ForeignKey {
            ref_schema,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
            match_type,
            deferrable,
        } = &c.constraint_type
        else {
            continue;
        };
        let quoted_name = quote_pg_identifier(&c.name)?;
        let local_cols = quote_column_list(&c.columns)?;
        let quoted_ref_schema = quote_pg_identifier(ref_schema)?;
        let quoted_ref_table = quote_pg_identifier(ref_table)?;
        let ref_cols = quote_column_list(ref_columns)?;
        // 카탈로그 원문(검색 경로가 비어 있어 참조 테이블도 스키마로 한정됨)을 그대로 쓴다.
        // ALTER TABLE ADD CONSTRAINT 는 NOT VALID·NOT ENFORCED 도 받아들인다.
        let body = match &c.definition {
            Some(definition) => definition.clone(),
            None => {
                // 문법 순서: REFERENCES .. [MATCH x] [ON DELETE] [ON UPDATE] [DEFERRABLE ..]
                let match_clause = match_type
                    .as_deref()
                    .map(|m| format!(" MATCH {m}"))
                    .unwrap_or_default();
                let deferrable_clause = deferrable
                    .as_deref()
                    .map(|d| format!(" {d}"))
                    .unwrap_or_default();
                format!(
                    "FOREIGN KEY ({local_cols}) \
                     REFERENCES {quoted_ref_schema}.{quoted_ref_table} ({ref_cols}){match_clause} \
                     ON DELETE {on_delete} ON UPDATE {on_update}{deferrable_clause}"
                )
            }
        };
        statements.push(format!(
            "ALTER TABLE {quoted_schema}.{quoted_table} ADD CONSTRAINT {quoted_name} {body};"
        ));
    }
    Ok(statements)
}

/// 뷰 정의(`pg_get_viewdef` 결과)로 `CREATE VIEW "schema"."view" AS ...;` 를 만든다.
///
/// `reloptions` 는 `pg_class.reloptions` (`security_barrier=true`, `check_option=cascaded` 등) —
/// `pg_get_viewdef` 는 SELECT 만 돌려주므로 따로 붙여야 보안·갱신 제약이 유지된다.
pub fn build_pg_view_ddl(
    schema: &str,
    view: &str,
    definition: &str,
    reloptions: &[String],
) -> Result<String, AppError> {
    let quoted_schema = quote_pg_identifier(schema)?;
    let quoted_view = quote_pg_identifier(view)?;
    let body = definition.trim_end().trim_end_matches(';');
    let (check_option, options): (Vec<&String>, Vec<&String>) = reloptions
        .iter()
        .partition(|o| o.starts_with("check_option="));
    let check_option = match check_option
        .first()
        .map(|o| o.trim_start_matches("check_option="))
    {
        Some("local") => "\nWITH LOCAL CHECK OPTION",
        Some(_) => "\nWITH CASCADED CHECK OPTION",
        None => "",
    };
    Ok(format!(
        "CREATE VIEW {quoted_schema}.{quoted_view}{} AS\n{body}{check_option};\n",
        with_options(&options)
    ))
}

/// `pg_class.reloptions` 의 `key=value` 목록 → ` WITH (key=value, ..)` (값은 필요하면 리터럴로)
fn with_options(options: &[&String]) -> String {
    if options.is_empty() {
        return String::new();
    }
    let options: Vec<String> = options
        .iter()
        .map(|option| match option.split_once('=') {
            Some((key, value))
                if value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) =>
            {
                format!("{key}={value}")
            }
            Some((key, value)) => format!("{key}={}", super::schema_ddl::quote_literal(value)),
            None => option.to_string(),
        })
        .collect();
    format!(" WITH ({})", options.join(", "))
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
) -> Result<TableDdl, AppError> {
    // 0. 뷰/머티리얼라이즈드 뷰면 CREATE [MATERIALIZED] VIEW 로 출력 (컬럼으로 재구성하면
    //    빈 CREATE TABLE 이 된다). 파티션 부모면 PARTITION BY 키를, 외부 테이블이면 서버·옵션을 받는다.
    let relation = sqlx::query(
        "SELECT c.relkind::text AS relkind, \
                CASE WHEN c.relkind IN ('v', 'm') THEN pg_get_viewdef(c.oid, true) END AS view_def, \
                pg_get_partkeydef(c.oid) AS partition_key, \
                c.reloptions AS reloptions, \
                fs.srvname::text AS foreign_server, \
                ft.ftoptions AS foreign_options \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_catalog.pg_foreign_table ft ON ft.ftrelid = c.oid \
         LEFT JOIN pg_catalog.pg_foreign_server fs ON fs.oid = ft.ftserver \
         WHERE n.nspname = $1 AND c.relname = $2",
    )
    .bind(schema)
    .bind(table)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::MetadataQuery {
        schema: schema.to_string(),
        table: table.to_string(),
        source: e,
    })?;
    let get_text = |column: &str| -> Option<String> {
        relation
            .as_ref()
            .and_then(|row| try_get_or_warn(row, column, schema, table))
    };
    let relkind = get_text("relkind").unwrap_or_default();
    let view_def = get_text("view_def");
    let reloptions: Vec<String> = relation
        .as_ref()
        .and_then(|row| try_get_or_warn::<_, Option<Vec<String>>>(row, "reloptions", schema, table))
        .unwrap_or_default();
    let partition_key = get_text("partition_key");
    let foreign = match get_text("foreign_server") {
        Some(server) => {
            let options: Option<Vec<String>> = relation
                .as_ref()
                .and_then(|row| try_get_or_warn(row, "foreign_options", schema, table));
            Some(super::foreign::build_foreign_suffix(
                &server,
                &options.unwrap_or_default(),
            )?)
        }
        None => None,
    };
    let comments = super::comment::fetch_comment_ddl(pool, schema, table).await?;
    if let Some(definition) = view_def {
        let mut create = if relkind == "m" {
            let index_defs = fetch_index_defs(pool, schema, table).await?;
            let mut ddl = build_pg_materialized_view_ddl(schema, table, &definition, &reloptions)?;
            append_statements(&mut ddl, &terminate(&index_defs));
            ddl
        } else {
            build_pg_view_ddl(schema, table, &definition, &reloptions)?
        };
        append_statements(&mut create, &comments);
        return Ok(TableDdl {
            create,
            ..Default::default()
        });
    }

    // 1. 컬럼 정보 조회 (ordinal_position 순). 타입 기본값과 다른 COLLATE 는 타입 뒤에 붙이고,
    //    NOT NULL 은 information_schema(도메인의 NOT NULL 까지 NO)가 아닌 컬럼 자체 속성으로 본다.
    //    default_after: 다른 테이블의 identity 시퀀스를 쓰는 기본값 — 그 테이블이 먼저 있어야 한다.
    let col_rows = sqlx::query(
        "SELECT \
             c.column_name, \
             format_type(a.atttypid, a.atttypmod) || \
               CASE WHEN a.attcollation <> 0 AND a.attcollation <> t.typcollation \
                    THEN ' COLLATE ' || format('%I.%I', colln.nspname, coll.collname) \
                    ELSE '' END AS data_type, \
             NOT a.attnotnull AS is_nullable, \
             c.column_default, \
             EXISTS ( \
                 SELECT 1 FROM pg_catalog.pg_attrdef ad \
                 JOIN pg_catalog.pg_depend d \
                   ON d.classid = 'pg_catalog.pg_attrdef'::regclass AND d.objid = ad.oid \
                  AND d.refclassid = 'pg_catalog.pg_class'::regclass \
                 JOIN pg_catalog.pg_depend i \
                   ON i.classid = 'pg_catalog.pg_class'::regclass AND i.objid = d.refobjid \
                  AND i.deptype = 'i' AND i.refobjid <> a.attrelid \
                 JOIN pg_catalog.pg_class seq ON seq.oid = d.refobjid AND seq.relkind = 'S' \
                 WHERE ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
             ) AS default_after, \
             a.attgenerated::text AS attgenerated, \
             c.generation_expression, \
             a.attidentity::text AS attidentity, \
             sn.nspname::text AS identity_seq_schema, \
             sc.relname::text AS identity_seq_name, \
             s.seqstart AS identity_start, \
             s.seqincrement AS identity_increment, \
             s.seqmin AS identity_min, \
             s.seqmax AS identity_max, \
             s.seqcache AS identity_cache, \
             s.seqcycle AS identity_cycle \
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
         JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
         LEFT JOIN pg_catalog.pg_collation coll ON coll.oid = a.attcollation \
         LEFT JOIN pg_catalog.pg_namespace colln ON colln.oid = coll.collnamespace \
         LEFT JOIN pg_catalog.pg_sequence s \
           ON a.attidentity <> '' \
          AND s.seqrelid = pg_get_serial_sequence(format('%I.%I', $1, $2), a.attname)::regclass \
         LEFT JOIN pg_catalog.pg_class sc ON sc.oid = s.seqrelid \
         LEFT JOIN pg_catalog.pg_namespace sn ON sn.oid = sc.relnamespace \
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
    let quoted_table = format!(
        "{}.{}",
        quote_pg_identifier(schema)?,
        quote_pg_identifier(table)?
    );
    let mut ddl_columns: Vec<PgDdlColumn> = Vec::new();
    let mut deferred_defaults: Vec<String> = Vec::new();
    for row in &col_rows {
        // try_get 실패 시 경고 로그 + 기본값 반환 (Requirements 5.2)
        let column_name: String = try_get_or_warn(row, "column_name", schema, table);
        // format_type: 정밀도(timestamp(3))·도메인·사용자 타입(스키마 한정)까지 pg_dump 와 같은 표기
        let data_type: String = try_get_or_warn(row, "data_type", schema, table);
        let is_nullable: bool = try_get_or_warn(row, "is_nullable", schema, table);
        let column_default: Option<String> = try_get_or_warn(row, "column_default", schema, table);
        let default_after: bool = try_get_or_warn(row, "default_after", schema, table);
        let column_default = match column_default {
            Some(default) if default_after => {
                deferred_defaults.push(format!(
                    "ALTER TABLE {quoted_table} ALTER COLUMN {} SET DEFAULT {default};",
                    quote_pg_identifier(&column_name)?
                ));
                None
            }
            other => other,
        };
        let attgenerated: String = try_get_or_warn(row, "attgenerated", schema, table);
        let generation_expression: Option<String> =
            try_get_or_warn(row, "generation_expression", schema, table);
        let attidentity: String = try_get_or_warn(row, "attidentity", schema, table);
        let seq_name: Option<String> = try_get_or_warn(row, "identity_seq_name", schema, table);
        let identity_sequence = seq_name.map(|name| {
            let num = |column: &str| -> i64 { try_get_or_warn(row, column, schema, table) };
            PgIdentitySequence {
                schema: try_get_or_warn(row, "identity_seq_schema", schema, table),
                name,
                start: num("identity_start"),
                increment: num("identity_increment"),
                min: num("identity_min"),
                max: num("identity_max"),
                cache: num("identity_cache"),
                cycle: try_get_or_warn(row, "identity_cycle", schema, table),
            }
        });

        ddl_columns.push(PgDdlColumn {
            name: column_name,
            data_type,
            is_nullable,
            default_value: column_default,
            generated: PgGenerated::from_catalog(&attgenerated, generation_expression),
            identity: PgIdentity::from_catalog(&attidentity, identity_sequence),
        });
    }

    // 2. 제약 조건 조회 (pg_constraint)
    let ddl_constraints = fetch_constraints(pool, schema, table).await?;

    // 3. 인덱스 정의 조회 (PK/UQ/EXCLUDE 제약 조건 인덱스 제외)
    let index_defs = fetch_index_defs(pool, schema, table).await?;

    // 4. DDL 재구성 — serial 시퀀스(스키마 파일 앞에서 생성)의 소유 관계는 테이블 직후에 복원
    let options = TableOptions {
        partition_key: partition_key.as_deref(),
        foreign: foreign.as_deref(),
    };
    let mut create = build_table_ddl(
        schema,
        table,
        &ddl_columns,
        &ddl_constraints,
        &index_defs,
        &options,
    )?;
    if foreign.is_some() {
        let column_options = super::foreign::fetch_column_options_ddl(pool, schema, table).await?;
        append_statements(&mut create, &column_options);
    }
    // 다른 스키마를 참조하는 FK 는 별도 파일로 — 스키마끼리 서로 참조하면 어느 스키마 파일을
    // 먼저 실행해도 참조 대상이 없어 실패한다
    let (cross_fks, local): (Vec<PgDdlConstraint>, Vec<PgDdlConstraint>) =
        ddl_constraints.iter().cloned().partition(|c| {
            matches!(&c.constraint_type, PgConstraintType::ForeignKey { ref_schema, .. }
                if ref_schema != schema)
        });
    let mut after = deferred_defaults;
    after.extend(build_pg_fk_ddl(schema, table, &local)?);
    let mut cross_schema = build_pg_fk_ddl(schema, table, &cross_fks)?;
    if partition_key.is_some() {
        let partitions = super::partition::fetch_partitions_ddl(pool, schema, table).await?;
        append_statements(&mut create, &partitions.create);
        after.extend(partitions.after);
        cross_schema.extend(partitions.cross_schema);
    }
    let ownership = super::schema_ddl::fetch_sequence_ownership(pool, schema, table).await?;
    append_statements(&mut create, &ownership);
    append_statements(&mut create, &comments);
    Ok(TableDdl {
        create,
        after,
        cross_schema,
    })
}

/// 테이블의 PK/UNIQUE/FK/CHECK/EXCLUDE/NOT NULL(PG 18+) 제약 조건을 `pg_constraint` 에서 조회한다.
///
/// DDL 재구성과 정의서용 FK 목록(`PgClient::get_constraints`)이 함께 쓴다.
pub(super) async fn fetch_constraints(
    pool: &sqlx::PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<PgDdlConstraint>, AppError> {
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
             con.confdeltype::text AS delete_code, \
             con.confupdtype::text AS update_code, \
             con.confmatchtype::text AS match_code, \
             con.condeferrable, \
             con.condeferred, \
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
           AND con.contype IN ('p', 'u', 'f', 'c', 'x', 'n') \
         ORDER BY \
           CASE con.contype \
             WHEN 'p' THEN 1 \
             WHEN 'u' THEN 2 \
             WHEN 'f' THEN 3 \
             WHEN 'c' THEN 4 \
             WHEN 'x' THEN 5 \
             WHEN 'n' THEN 6 \
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
                // 정의서 표시용 동작은 문자열 파싱 대신 카탈로그 코드에서 바로 읽는다
                // (DDL 은 condef 원문을 쓰므로 여기 값은 표시에만 쓰인다)
                let code = |column: &str| -> String { try_get_or_warn(row, column, schema, table) };
                let deferrable: bool = try_get_or_warn(row, "condeferrable", schema, table);
                let deferred: bool = try_get_or_warn(row, "condeferred", schema, table);
                PgConstraintType::ForeignKey {
                    ref_schema: ref_schema.unwrap_or_default(),
                    ref_table: ref_table.unwrap_or_default(),
                    // 참조 컬럼 이름도 서브쿼리로 이미 해석됨 → 추가 쿼리 없음
                    ref_columns: ref_col_names.unwrap_or_default(),
                    on_delete: fk_action(&code("delete_code")),
                    on_update: fk_action(&code("update_code")),
                    match_type: match code("match_code").as_str() {
                        "f" => Some("FULL".to_string()),
                        "p" => Some("PARTIAL".to_string()),
                        _ => None,
                    },
                    deferrable: deferrable.then(|| {
                        if deferred {
                            "DEFERRABLE INITIALLY DEFERRED"
                        } else {
                            "DEFERRABLE"
                        }
                        .to_string()
                    }),
                }
            }
            "c" => {
                // NOT NULL 은 CHECK 가 아니라 contype 'n'(PG 18+)이거나 카탈로그에 없으므로
                // 이름으로 걸러내지 않는다 (예전엔 이름이 *_not_null 인 사용자 CHECK 가 빠졌다)
                let expression = extract_check_expression(&condef);
                PgConstraintType::Check { expression }
            }
            "x" => PgConstraintType::Exclude {
                definition: condef.clone(),
            },
            "n" => PgConstraintType::NotNull,
            _ => continue,
        };

        ddl_constraints.push(PgDdlConstraint {
            name: conname,
            constraint_type,
            columns: local_columns,
            definition: Some(condef),
        });
    }

    Ok(ddl_constraints)
}

/// 릴레이션의 인덱스 정의 (`pg_get_indexdef`). PK/UNIQUE/EXCLUDE 제약이 만든 인덱스는 제약으로
/// 출력되므로 제외한다. 테이블·머티리얼라이즈드 뷰 공용.
async fn fetch_index_defs(
    pool: &sqlx::PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, AppError> {
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
                 AND con.contype IN ('p', 'u', 'x') \
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

    Ok(index_rows
        .iter()
        .map(|row| try_get_or_warn::<_, String>(row, "indexdef", schema, table))
        .filter(|s| !s.is_empty())
        .collect())
}

/// `pg_constraint.confdeltype` / `confupdtype` 코드 → 정의서 표기
fn fk_action(code: &str) -> String {
    match code {
        "r" => "RESTRICT",
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        _ => "NO ACTION",
    }
    .to_string()
}
