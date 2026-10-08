//! PostgreSQL 외부 테이블(FDW)의 `SERVER .. OPTIONS (..)` 절.
//!
//! `CREATE SERVER` / `CREATE USER MAPPING` 은 출력하지 않는다 — 서버는 클러스터 수준
//! 설정이고, USER MAPPING 에는 원격 접속 비밀번호가 들어 있을 수 있다. SQL 파일을 실행하기
//! 전에 같은 이름의 서버가 있어야 한다.
// ponytail: 컬럼 수준 FDW 옵션(attfdwoptions)은 생략 — 쓰는 외부 테이블이 생기면 같은 방식으로 추가.

use super::schema_ddl::quote_literal;
use crate::{error::AppError, identifier::quote_pg_identifier};

/// `SERVER "srv" OPTIONS (schema_name 'public', table_name 'remote')` —
/// `options` 는 `pg_foreign_table.ftoptions` 의 `key=value` 목록.
pub(super) fn build_foreign_suffix(server: &str, options: &[String]) -> Result<String, AppError> {
    let mut suffix = format!("SERVER {}", quote_pg_identifier(server)?);
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
    if !options.is_empty() {
        suffix.push_str(&format!(" OPTIONS ({})", options.join(", ")));
    }
    Ok(suffix)
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
    }
}
