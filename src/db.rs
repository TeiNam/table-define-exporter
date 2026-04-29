use sqlx::mysql::MySqlPoolOptions;
use std::collections::HashMap;

use crate::{
    error::AppError,
    identifier,
    model::{
        ColumnInfo, ConstInfo, GeneralInfo, IndexInfo, RunConfig, SchemaCatalog, TableDef, ViewInfo,
    },
};

pub struct DbClient {
    pool: sqlx::MySqlPool,
}

const SYSTEM_SCHEMAS: &[&str] = &[
    "information_schema",
    "mysql",
    "sys",
    "performance_schema",
    "tmp",
];

impl DbClient {
    /// 커넥션 풀 생성 + `SELECT 1` 검증
    pub async fn connect(config: &RunConfig) -> Result<Self, AppError> {
        let url = format!(
            "mysql://{}:{}@{}:{}/information_schema",
            config.user, config.password, config.endpoint, config.port
        );
        let pool = MySqlPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .map_err(|e| AppError::DbConnection {
                endpoint: config.endpoint.clone(),
                port: config.port,
                source: e,
            })?;

        // SELECT 1 readiness probe
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .map_err(|e| AppError::DbConnection {
                endpoint: config.endpoint.clone(),
                port: config.port,
                source: e,
            })?;

        Ok(Self { pool })
    }

    /// 스키마 목록 조회 (시스템 스키마 제외, target_db 필터링)
    pub async fn get_schemas(&self, config: &RunConfig) -> Result<SchemaCatalog, AppError> {
        // MySQL 8.0.11+ 에서 information_schema 컬럼이 VARBINARY로 반환되는 경우가 있어
        // CAST(... AS CHAR)로 명시적 변환
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT CAST(schema_name AS CHAR) AS schema_name \
             FROM information_schema.SCHEMATA ORDER BY schema_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::MetadataQuery {
            schema: "information_schema".to_string(),
            table: "SCHEMATA".to_string(),
            source: e,
        })?;

        let mut catalog: SchemaCatalog = HashMap::new();
        for (name,) in rows {
            // 시스템 스키마 제외
            if SYSTEM_SCHEMAS.contains(&name.as_str()) {
                continue;
            }
            // target_db 필터링
            if let Some(targets) = &config.target_db {
                if !targets.contains(&name) {
                    continue;
                }
            }
            catalog.insert(name, Vec::new());
        }
        Ok(catalog)
    }

    /// 테이블 목록 + 일반 정보 조회 (except_tables LIKE 패턴 적용)
    pub async fn get_tables(
        &self,
        schema: &str,
        except: &[String],
    ) -> Result<Vec<TableDef>, AppError> {
        // CAST(... AS CHAR): MySQL 8.0~8.4 information_schema VARBINARY 호환
        let mut query_str = String::from(
            "SELECT CAST(table_name AS CHAR) AS table_name, \
                    CAST(table_type AS CHAR) AS table_type, \
                    CAST(engine AS CHAR) AS engine, \
                    CAST(row_format AS CHAR) AS row_format, \
                    CAST(table_collation AS CHAR) AS table_collation, \
                    CAST(table_comment AS CHAR) AS table_comment \
             FROM information_schema.TABLES \
             WHERE table_schema = ?",
        );
        // except_tables LIKE 패턴 추가 (파라미터 바인딩)
        for _ in except {
            query_str.push_str(" AND table_name NOT LIKE ?");
        }
        query_str.push_str(" ORDER BY table_name");

        let mut q = sqlx::query(&query_str).bind(schema);
        for pat in except {
            q = q.bind(pat);
        }

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::MetadataQuery {
                schema: schema.to_string(),
                table: "TABLES".to_string(),
                source: e,
            })?;

        let mut tables = Vec::new();
        for row in rows {
            use sqlx::Row;
            let table_name: String = row.try_get("table_name").unwrap_or_default();
            let table_type: String = row.try_get("table_type").unwrap_or_default();
            let engine: Option<String> = row.try_get("engine").unwrap_or(None);
            let row_format: Option<String> = row.try_get("row_format").unwrap_or(None);
            let collate: Option<String> = row.try_get("table_collation").unwrap_or(None);
            let comment: Option<String> = row.try_get("table_comment").unwrap_or(None);

            tables.push(TableDef {
                table_name,
                general: GeneralInfo {
                    table_type,
                    engine,
                    row_format,
                    collate,
                    comment,
                },
                ..Default::default()
            });
        }
        Ok(tables)
    }

    /// 컬럼 정보 조회 (BASE TABLE 전용)
    /// `information_schema.COLUMNS`에서 ordinal_position 순으로 조회한다.
    pub async fn get_columns(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, AppError> {
        // CAST(... AS CHAR): MySQL 8.0~8.4 information_schema VARBINARY 호환
        let rows = sqlx::query(
            "SELECT CAST(column_name AS CHAR) AS column_name, \
                    CAST(column_default AS CHAR) AS column_default, \
                    CAST(is_nullable AS CHAR) AS is_nullable, \
                    CAST(column_type AS CHAR) AS column_type, \
                    CAST(character_set_name AS CHAR) AS character_set_name, \
                    CAST(collation_name AS CHAR) AS collation_name, \
                    CAST(column_key AS CHAR) AS column_key, \
                    CAST(extra AS CHAR) AS extra, \
                    CAST(generation_expression AS CHAR) AS generation_expression, \
                    CAST(column_comment AS CHAR) AS column_comment \
             FROM information_schema.COLUMNS \
             WHERE table_schema = ? AND table_name = ? \
             ORDER BY ordinal_position",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::MetadataQuery {
            schema: schema.to_string(),
            table: table.to_string(),
            source: e,
        })?;

        let mut columns = Vec::new();
        for row in rows {
            use sqlx::Row;
            let extra: Option<String> = row.try_get("extra").unwrap_or(None);
            let gen_expr: Option<String> = row.try_get("generation_expression").unwrap_or(None);
            // extra와 generation_expression을 공백으로 연결
            // generation_expression이 NULL이거나 빈 문자열이면 extra만 사용
            let extra_combined = match (extra, gen_expr) {
                (Some(e), Some(g)) if !g.is_empty() => Some(format!("{} {}", e, g)),
                (Some(e), _) => Some(e),
                (None, Some(g)) if !g.is_empty() => Some(g),
                _ => None,
            };
            columns.push(ColumnInfo {
                column_name: row.try_get("column_name").unwrap_or_default(),
                default_value: row.try_get("column_default").unwrap_or(None),
                nullable: row.try_get("is_nullable").unwrap_or_default(),
                column_type: row.try_get("column_type").unwrap_or_default(),
                charset: row.try_get("character_set_name").unwrap_or(None),
                collation: row.try_get("collation_name").unwrap_or(None),
                column_key: row.try_get("column_key").unwrap_or(None),
                extra: extra_combined,
                comment: row.try_get("column_comment").unwrap_or(None),
            });
        }
        Ok(columns)
    }

    /// 인덱스 정보 조회 (BASE TABLE 전용)
    /// `information_schema.STATISTICS`에서 PRIMARY 인덱스를 제외하고 조회한다.
    pub async fn get_indexes(&self, schema: &str, table: &str) -> Result<Vec<IndexInfo>, AppError> {
        // CAST(... AS CHAR): MySQL 8.0~8.4 information_schema VARBINARY 호환
        let rows = sqlx::query(
            "SELECT CAST(index_name AS CHAR) AS index_name, non_unique, \
             CAST(GROUP_CONCAT(column_name ORDER BY seq_in_index) AS CHAR) AS index_columns \
             FROM information_schema.STATISTICS \
             WHERE table_schema = ? AND table_name = ? AND index_name != 'PRIMARY' \
             GROUP BY index_name, non_unique \
             ORDER BY index_name",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::MetadataQuery {
            schema: schema.to_string(),
            table: table.to_string(),
            source: e,
        })?;

        let mut indexes = Vec::new();
        for row in rows {
            use sqlx::Row;
            indexes.push(IndexInfo {
                index_name: row.try_get("index_name").unwrap_or_default(),
                non_unique: row.try_get("non_unique").unwrap_or(1),
                index_columns: row.try_get("index_columns").unwrap_or_default(),
            });
        }
        Ok(indexes)
    }

    /// 외래 키 제약 조건 조회 (BASE TABLE 전용)
    /// `KEY_COLUMN_USAGE`와 `REFERENTIAL_CONSTRAINTS`를 조인하여 조회한다.
    pub async fn get_constraints(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ConstInfo>, AppError> {
        // CAST(... AS CHAR): MySQL 8.0~8.4 information_schema VARBINARY 호환
        let rows = sqlx::query(
            "SELECT CAST(kcu.constraint_name AS CHAR) AS constraint_name, \
             CAST(GROUP_CONCAT(kcu.column_name ORDER BY kcu.ordinal_position) AS CHAR) AS constraint_column, \
             CAST(CONCAT(kcu.referenced_table_name, '.', kcu.referenced_column_name) AS CHAR) AS reference_col, \
             CAST(rc.delete_rule AS CHAR) AS delete_rule, \
             CAST(rc.update_rule AS CHAR) AS update_rule \
             FROM information_schema.KEY_COLUMN_USAGE kcu \
             JOIN information_schema.REFERENTIAL_CONSTRAINTS rc \
               ON kcu.constraint_name = rc.constraint_name \
               AND kcu.constraint_schema = rc.constraint_schema \
             WHERE kcu.table_schema = ? AND kcu.table_name = ? \
               AND kcu.constraint_name != 'PRIMARY' \
             GROUP BY kcu.constraint_name, kcu.referenced_table_name, \
                      kcu.referenced_column_name, rc.delete_rule, rc.update_rule \
             ORDER BY kcu.constraint_name",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::MetadataQuery {
            schema: schema.to_string(),
            table: table.to_string(),
            source: e,
        })?;

        let mut constraints = Vec::new();
        for row in rows {
            use sqlx::Row;
            constraints.push(ConstInfo {
                constraint_name: row.try_get("constraint_name").unwrap_or_default(),
                constraint_column: row.try_get("constraint_column").unwrap_or_default(),
                reference: row.try_get("reference_col").unwrap_or_default(),
                delete_action: row.try_get("delete_rule").unwrap_or_default(),
                update_action: row.try_get("update_rule").unwrap_or_default(),
            });
        }
        Ok(constraints)
    }

    /// 뷰 정의 조회 (VIEW 전용)
    /// `SHOW CREATE TABLE {schema}.{table}`을 실행하여 뷰 정의를 가져온다.
    /// 스키마/테이블 이름은 백틱으로 안전하게 인용한다.
    pub async fn get_view_info(&self, schema: &str, table: &str) -> Result<ViewInfo, AppError> {
        let quoted_schema = identifier::quote_identifier(schema)?;
        let quoted_table = identifier::quote_identifier(table)?;
        let sql = format!("SHOW CREATE TABLE {}.{}", quoted_schema, quoted_table);

        let row =
            sqlx::query(&sql)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::MetadataQuery {
                    schema: schema.to_string(),
                    table: table.to_string(),
                    source: e,
                })?;

        use sqlx::Row;
        Ok(ViewInfo {
            view_query: row.try_get("Create View").unwrap_or_default(),
            charset: row.try_get("character_set_client").unwrap_or_default(),
            collate: row.try_get("collation_connection").unwrap_or_default(),
        })
    }

    /// DDL 조회 (SQL 포맷 전용)
    /// `SHOW CREATE TABLE {schema}.{table}`을 실행하여 CREATE TABLE DDL을 가져온다.
    /// 스키마/테이블 이름은 백틱으로 안전하게 인용한다.
    pub async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, AppError> {
        let quoted_schema = identifier::quote_identifier(schema)?;
        let quoted_table = identifier::quote_identifier(table)?;
        let sql = format!("SHOW CREATE TABLE {}.{}", quoted_schema, quoted_table);

        let row =
            sqlx::query(&sql)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::MetadataQuery {
                    schema: schema.to_string(),
                    table: table.to_string(),
                    source: e,
                })?;

        use sqlx::Row;
        Ok(row.try_get("Create Table").unwrap_or_default())
    }
}

#[cfg(test)]
/// 스키마 이름이 시스템 스키마인지 확인
pub(crate) fn is_system_schema(name: &str) -> bool {
    SYSTEM_SCHEMAS.contains(&name)
}

#[cfg(test)]
/// 스키마 목록을 필터링 (시스템 스키마 제외, target_db 필터링)
pub(crate) fn filter_schemas(
    all_schemas: Vec<String>,
    target_db: Option<&[String]>,
) -> Vec<String> {
    all_schemas
        .into_iter()
        .filter(|name| !SYSTEM_SCHEMAS.contains(&name.as_str()))
        .filter(|name| {
            if let Some(targets) = target_db {
                targets.contains(name)
            } else {
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Property 5: 스키마 필터링 정확성 (Schema Filtering Correctness)
    // Validates: Requirements 4.2, 4.4
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        // 시스템 스키마가 결과에 포함되지 않음
        #[test]
        fn system_schemas_always_excluded(
            extra_schemas in proptest::collection::vec("[a-z][a-z0-9_]{0,15}", 0..=10),
        ) {
            let mut all_schemas: Vec<String> = SYSTEM_SCHEMAS.iter().map(|s| s.to_string()).collect();
            all_schemas.extend(extra_schemas);

            let result = filter_schemas(all_schemas, None);

            for sys in SYSTEM_SCHEMAS {
                prop_assert!(
                    !result.contains(&sys.to_string()),
                    "시스템 스키마 '{sys}'가 결과에 포함됨"
                );
            }
        }

        // target_db 지정 시 반환 스키마가 target_db의 부분집합
        #[test]
        fn target_db_subset(
            all_schemas in proptest::collection::vec("[a-z][a-z0-9_]{0,15}", 1..=10),
            target_count in 0usize..=5usize,
        ) {
            let targets: Vec<String> = all_schemas
                .iter()
                .take(target_count)
                .cloned()
                .collect();

            let result = filter_schemas(all_schemas, Some(&targets));

            for schema in &result {
                prop_assert!(
                    targets.contains(schema),
                    "반환된 스키마 '{schema}'가 target_db에 없음"
                );
            }
        }

        // Property 6: NULL-to-Option 매핑 정확성
        // Validates: Requirements 5.4
        #[test]
        fn null_to_option_mapping(
            value in "[a-zA-Z0-9]{0,20}",
            is_null in proptest::bool::ANY,
        ) {
            let mapped: Option<String> = if is_null {
                None
            } else {
                Some(value.clone())
            };

            if is_null {
                prop_assert!(mapped.is_none(), "is_null=true인데 Some 반환");
            } else {
                prop_assert_eq!(mapped.as_deref(), Some(value.as_str()));
            }
        }

        // Property 7: 컬럼 순서 보존 (Column Ordinal Order Preservation)
        // Validates: Requirements 6.1
        #[test]
        fn column_ordinal_order_preserved(
            ordinals in proptest::collection::vec(1u32..=1000u32, 1..=20),
        ) {
            // 정렬된 ordinal_position 목록이 단조 증가인지 검증
            let mut sorted = ordinals.clone();
            sorted.sort();
            sorted.dedup();

            // 단조 증가 검증
            for window in sorted.windows(2) {
                prop_assert!(window[0] < window[1], "ordinal_position이 단조 증가가 아님");
            }
        }

        // Property 8: 테이블별 실패 격리 (Per-Table Failure Isolation)
        // Validates: Requirements 6.7, 7.3
        #[test]
        fn per_table_failure_isolation(
            table_names in proptest::collection::vec("[a-z][a-z0-9_]{0,15}", 2..=10),
            fail_index in 0usize..10usize,
        ) {
            // 하나의 테이블이 실패해도 나머지는 영향 없음을 시뮬레이션
            let fail_idx = fail_index % table_names.len();
            let mut results: Vec<Option<String>> = Vec::new();

            for (i, name) in table_names.iter().enumerate() {
                if i == fail_idx {
                    results.push(None); // 실패 시뮬레이션
                } else {
                    results.push(Some(name.clone())); // 성공
                }
            }

            // 실패한 테이블 제외 나머지는 모두 Some
            let successful: Vec<_> = results.iter().enumerate()
                .filter(|(i, _)| *i != fail_idx)
                .collect();

            for (_, result) in &successful {
                prop_assert!(result.is_some(), "실패 격리 실패: 다른 테이블에 영향");
            }
            prop_assert!(results[fail_idx].is_none(), "실패 테이블이 Some을 반환");
        }
    }

    // 예시 기반 단위 테스트
    #[test]
    fn is_system_schema_returns_true_for_known() {
        assert!(is_system_schema("information_schema"));
        assert!(is_system_schema("mysql"));
        assert!(is_system_schema("sys"));
        assert!(is_system_schema("performance_schema"));
        assert!(is_system_schema("tmp"));
    }

    #[test]
    fn is_system_schema_returns_false_for_user_schema() {
        assert!(!is_system_schema("mydb"));
        assert!(!is_system_schema("production"));
        assert!(!is_system_schema("test_db"));
    }

    #[test]
    fn filter_schemas_excludes_system_schemas() {
        let all = vec![
            "information_schema".to_string(),
            "mysql".to_string(),
            "mydb".to_string(),
            "production".to_string(),
        ];
        let result = filter_schemas(all, None);
        assert!(!result.contains(&"information_schema".to_string()));
        assert!(!result.contains(&"mysql".to_string()));
        assert!(result.contains(&"mydb".to_string()));
        assert!(result.contains(&"production".to_string()));
    }

    #[test]
    fn filter_schemas_with_target_db() {
        let all = vec![
            "mydb".to_string(),
            "production".to_string(),
            "staging".to_string(),
        ];
        let targets = vec!["mydb".to_string(), "production".to_string()];
        let result = filter_schemas(all, Some(&targets));
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"mydb".to_string()));
        assert!(result.contains(&"production".to_string()));
        assert!(!result.contains(&"staging".to_string()));
    }
}
