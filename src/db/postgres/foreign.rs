//! PostgreSQL 외부 테이블(FDW)의 `SERVER .. OPTIONS (..)` 절과 컬럼 옵션.
//!
//! `CREATE SERVER` / `CREATE USER MAPPING` 은 출력하지 않는다 — 서버는 클러스터 수준
//! 설정이고, USER MAPPING 에는 원격 접속 비밀번호가 들어 있을 수 있다. SQL 파일을 실행하기
//! 전에 같은 이름의 서버가 있어야 한다.

use sqlx::PgPool;

use super::schema_ddl::quote_literal;
use crate::{db::try_get_or_warn, error::AppError, identifier::quote_pg_identifier};

/// `SERVER "srv" OPTIONS (schema_name 'public', table_name 'remote')` —
/// `options` 는 `pg_foreign_table.ftoptions` 의 `key=value` 목록.
pub(super) fn build_foreign_suffix(server: &str, options: &[String]) -> Result<String, AppError> {
    let mut suffix = format!("SERVER {}", quote_pg_identifier(server)?);
    if !options.is_empty() {
        suffix.push_str(&format!(" {}", options_clause(options)?));
    }
    Ok(suffix)
}

/// `OPTIONS ("key" 'value', ..)` — `options` 는 `key=value` 목록 (`ftoptions`·`attfdwoptions`)
fn options_clause(options: &[String]) -> Result<String, AppError> {
    let options = options
        .iter()
        .map(|option| {
            let (key, value) = option.split_once('=').unwrap_or((option, ""));
            Ok(format!(
                "{} {}",
                quote_pg_identifier(key)?,
                quote_literal(value)
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(format!("OPTIONS ({})", options.join(", ")))
}

/// 외부 테이블 컬럼의 FDW 옵션(`attfdwoptions`) — pg_dump 처럼 테이블 생성 뒤
/// `ALTER FOREIGN TABLE .. ALTER COLUMN .. OPTIONS (..);` 로 쓴다 (컬럼을 나열하지 않는
/// `PARTITION OF` 외부 파티션에도 같은 방식이 통한다).
pub(super) async fn fetch_column_options_ddl(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<String>, AppError> {
    let rows = sqlx::query(
        "SELECT a.attname::text AS column_name, a.attfdwoptions AS options \
         FROM pg_catalog.pg_attribute a \
         WHERE a.attrelid = format('%I.%I', $1, $2)::regclass \
           AND a.attnum > 0 AND NOT a.attisdropped AND cardinality(a.attfdwoptions) > 0 \
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
    let target = format!(
        "{}.{}",
        quote_pg_identifier(schema)?,
        quote_pg_identifier(table)?
    );
    rows.iter()
        .map(|row| {
            let column: String = try_get_or_warn(row, "column_name", schema, table);
            let options: Vec<String> = try_get_or_warn(row, "options", schema, table);
            Ok(format!(
                "ALTER FOREIGN TABLE {target} ALTER COLUMN {} {};",
                quote_pg_identifier(&column)?,
                options_clause(&options)?
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_server_and_options() {
        assert_eq!(
            build_foreign_suffix(
                "rsrv",
                &["schema_name=public".into(), "table_name=it's".into()]
            )
            .unwrap(),
            r#"SERVER "rsrv" OPTIONS ("schema_name" 'public', "table_name" 'it''s')"#
        );
        assert_eq!(
            build_foreign_suffix("rsrv", &[]).unwrap(),
            r#"SERVER "rsrv""#
        );
        assert!(build_foreign_suffix("a;b", &[]).is_err());
        assert_eq!(
            options_clause(&["column_name=remote id".into()]).unwrap(),
            r#"OPTIONS ("column_name" 'remote id')"#
        );
    }
}
