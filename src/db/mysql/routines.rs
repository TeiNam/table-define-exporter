//! MySQL 함수·프로시저 생성문 (SQL 출력의 테이블 앞).
//!
//! 뷰가 저장 함수를 부르면 함수가 먼저 있어야 뷰를 만들 수 있다. 본문에 `;` 가 들어가므로 mysqldump
//! 처럼 `DELIMITER ;;` 로 감싸고, 루틴이 만들어질 때의 sql_mode(본문 실행 의미에 영향)를 되살린다.
// ponytail: 트리거·이벤트는 생략 — 필요해지면 같은 방식(SHOW CREATE TRIGGER/EVENT)으로 추가.

use super::{MySqlClient, ddl_column, quote_string_literal};
use crate::{error::AppError, identifier, model::SchemaDdl};

impl MySqlClient {
    /// 스키마의 함수·프로시저 (권한이 없어 정의를 못 읽으면 경고 후 생략)
    pub async fn get_schema_ddl(&self, schema: &str) -> Result<SchemaDdl, AppError> {
        let routines: Vec<(String, String)> = sqlx::query_as(
            "SELECT CAST(routine_type AS CHAR), CAST(routine_name AS CHAR) \
             FROM information_schema.ROUTINES \
             WHERE routine_schema = ? AND routine_type IN ('FUNCTION', 'PROCEDURE') \
             ORDER BY routine_type, routine_name",
        )
        .bind(schema)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::MetadataQuery {
            schema: schema.to_string(),
            table: "ROUTINES".to_string(),
            source: e,
        })?;

        let mut before = Vec::new();
        for (kind, name) in routines {
            match self.routine_ddl(schema, &kind, &name).await {
                Ok(Some(statement)) => before.push(statement),
                Ok(None) => tracing::warn!(
                    "{schema}.{name} ({kind}) 생성문을 읽을 권한이 없어 생략 (SHOW_ROUTINE 또는 정의자 필요)"
                ),
                Err(e) => tracing::warn!("{schema}.{name} ({kind}) 생성문 생략: {e}"),
            }
        }
        Ok(SchemaDdl {
            before,
            after_tables: Vec::new(),
        })
    }

    /// `SHOW CREATE FUNCTION|PROCEDURE` 를 원래 sql_mode 와 `DELIMITER ;;` 로 감싼 블록
    async fn routine_ddl(
        &self,
        schema: &str,
        kind: &str,
        name: &str,
    ) -> Result<Option<String>, AppError> {
        let (keyword, column) = match kind {
            "FUNCTION" => ("FUNCTION", "Create Function"),
            _ => ("PROCEDURE", "Create Procedure"),
        };
        // 식별자는 quote_identifier 로 인용·검증됨 → AssertSqlSafe 안전 (SHOW 는 text protocol)
        let sql = format!(
            "SHOW CREATE {keyword} {}.{}",
            identifier::quote_identifier(schema)?,
            identifier::quote_identifier(name)?
        );
        let row = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::MetadataQuery {
                schema: schema.to_string(),
                table: name.to_string(),
                source: e,
            })?;
        let Some(create) = ddl_column(&row, column) else {
            return Ok(None);
        };
        let sql_mode = ddl_column(&row, "sql_mode").unwrap_or_default();
        Ok(Some(routine_block(
            &self.definer_policy(schema, name, create),
            &sql_mode,
        )))
    }
}

/// 루틴 하나를 실행할 블록 — 파일의 SQL_MODE 를 잠시 루틴의 것으로 바꿨다가 되돌린다
fn routine_block(create: &str, sql_mode: &str) -> String {
    format!(
        "SET @OLD_ROUTINE_SQL_MODE = @@SQL_MODE, SQL_MODE = {};\n\
         DELIMITER ;;\n{create};;\nDELIMITER ;\n\
         SET SQL_MODE = @OLD_ROUTINE_SQL_MODE;",
        quote_string_literal(sql_mode)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_block_restores_sql_mode_and_uses_delimiter() {
        assert_eq!(
            routine_block(
                "CREATE FUNCTION `f`() RETURNS int\nBEGIN\n  RETURN 1;\nEND",
                "STRICT_TRANS_TABLES"
            ),
            "SET @OLD_ROUTINE_SQL_MODE = @@SQL_MODE, SQL_MODE = 'STRICT_TRANS_TABLES';\n\
             DELIMITER ;;\nCREATE FUNCTION `f`() RETURNS int\nBEGIN\n  RETURN 1;\nEND;;\n\
             DELIMITER ;\nSET SQL_MODE = @OLD_ROUTINE_SQL_MODE;"
        );
    }
}
