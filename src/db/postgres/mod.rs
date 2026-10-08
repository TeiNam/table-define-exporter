use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;

use crate::{
    db::try_get_or_warn,
    error::AppError,
    model::{
        ColumnInfo, ConstInfo, GeneralInfo, IndexInfo, RunConfig, SchemaCatalog, TableDdl,
        TableDef, ViewInfo, fk_reference,
    },
};

mod comment;
mod ddl;
mod foreign;
mod parse;
mod partition;
mod schema_ddl;
mod types;

pub use ddl::{
    build_pg_ddl_from_metadata, build_pg_fk_ddl, build_pg_materialized_view_ddl, build_pg_view_ddl,
};
pub use parse::{ParsedIndex, parse_pg_indexdef};
pub use types::{
    PgConstraintType, PgDdlColumn, PgDdlConstraint, PgGenerated, PgIdentity, determine_pg_extra,
};

/// PostgreSQL 시스템 스키마 목록 (정적 매칭 대상)
const PG_STATIC_SYSTEM_SCHEMAS: &[&str] = &["pg_catalog", "information_schema", "pg_toast"];

/// PostgreSQL 시스템 스키마인지 확인한다.
/// `pg_catalog`, `information_schema`, `pg_toast` 및
/// `pg_temp_`, `pg_toast_temp_` 접두어를 가진 스키마를 시스템 스키마로 판별한다.
pub fn is_pg_system_schema(name: &str) -> bool {
    PG_STATIC_SYSTEM_SCHEMAS.contains(&name)
        || name.starts_with("pg_temp_")
        || name.starts_with("pg_toast_temp_")
}

/// 스키마 목록을 필터링한다 (PG 시스템 스키마 제외, target_db 필터링).
/// 순수 함수로 추출하여 PBT 테스트가 가능하다.
pub fn filter_pg_schemas(all_schemas: Vec<String>, target_db: Option<&[String]>) -> Vec<String> {
    all_schemas
        .into_iter()
        .filter(|name| !is_pg_system_schema(name))
        .filter(|name| {
            if let Some(targets) = target_db {
                targets.contains(name)
            } else {
                true
            }
        })
        .collect()
}

/// PostgreSQL 전용 DB 클라이언트
pub struct PgClient {
    /// PostgreSQL 커넥션 풀
    pool: sqlx::PgPool,
    /// DB 레벨 collation (모든 테이블에 동일하게 적용)
    db_collation: String,
}

impl PgClient {
    /// 커넥션 풀 생성 + `SELECT 1` 검증 + DB collation 캐시
    ///
    /// URL 문자열 포매팅 대신 `PgConnectOptions` 빌더로 자격 증명을 전달하여
    /// 비밀번호에 포함된 URL 예약 문자(`@`, `:`, `/`, `#`, `?`, `%` 등)에도
    /// 안전하게 연결한다. 에러 발생 시 비밀번호를 포함하지 않는
    /// `AppError::DbConnection`을 반환한다.
    pub async fn connect(config: &RunConfig) -> Result<Self, AppError> {
        // URL 포매팅 대신 타입 안전한 ConnectOptions 빌더를 사용해 비밀번호
        // 이스케이프 문제를 원천 차단한다.
        let options = crate::db::connect::pg_options(config);

        // 커넥션 풀 생성 (최대 4개 연결)
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
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

        // DB 레벨 collation 캐시
        let row: (String,) = sqlx::query_as(
            "SELECT datcollate FROM pg_catalog.pg_database \
             WHERE datname = current_database()",
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| AppError::DbConnection {
            endpoint: config.endpoint.clone(),
            port: config.port,
            source: e,
        })?;

        Ok(Self {
            pool,
            db_collation: row.0,
        })
    }

    /// 스키마 목록 조회 (PG 시스템 스키마 제외, target_db 필터링)
    ///
    /// `information_schema.schemata`에서 스키마 이름을 조회한 뒤,
    /// `is_pg_system_schema`로 시스템 스키마를 제외하고
    /// `target_db`가 지정된 경우 해당 스키마만 반환한다.
    pub async fn get_schemas(&self, config: &RunConfig) -> Result<SchemaCatalog, AppError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT schema_name FROM information_schema.schemata \
             ORDER BY schema_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::MetadataQuery {
            schema: "information_schema".to_string(),
            table: "schemata".to_string(),
            source: e,
        })?;

        let all_schemas: Vec<String> = rows.into_iter().map(|(name,)| name).collect();
        let filtered = filter_pg_schemas(all_schemas, config.target_db.as_deref());

        let mut catalog: SchemaCatalog = HashMap::new();
        for name in filtered {
            catalog.insert(name, Vec::new());
        }
        Ok(catalog)
    }

    /// 테이블 목록 + 일반 정보 조회 (except_tables LIKE 패턴 적용)
    ///
    /// `information_schema.tables` + `obj_description()` 조인으로
    /// 테이블 이름, 타입, 주석을 조회한다.
    /// PostgreSQL에는 engine/row_format 개념이 없으므로 항상 None.
    /// collation은 DB 레벨 collation을 사용한다.
    pub async fn get_tables(
        &self,
        schema: &str,
        except: &[String],
    ) -> Result<Vec<TableDef>, AppError> {
        // pg_class 는 반드시 스키마(namespace)까지 맞춰 조인한다. 이름만으로 조인하면
        // 다른 스키마의 같은 이름 테이블 수만큼 행이 늘고 코멘트도 섞인다.
        // 하위 파티션은 부모 테이블 DDL 에 PARTITION OF 로 붙으므로 목록에서 뺀다 (MySQL 처럼 테이블 하나).
        // 머티리얼라이즈드 뷰는 information_schema.tables 에 없어서 pg_class 에서 따로 더한다.
        // 동적 쿼리 구성: except_tables LIKE 패턴 추가
        let mut query_str = String::from(
            "SELECT * FROM ( \
                 SELECT t.table_name::text AS table_name, t.table_type::text AS table_type, \
                        obj_description(c.oid, 'pg_class') AS table_comment \
                 FROM information_schema.tables t \
                 LEFT JOIN pg_catalog.pg_namespace n \
                   ON n.nspname = t.table_schema \
                 LEFT JOIN pg_catalog.pg_class c \
                   ON c.relnamespace = n.oid AND c.relname = t.table_name \
                 WHERE t.table_schema = $1 \
                   AND NOT COALESCE(c.relispartition, false) \
                 UNION ALL \
                 SELECT c.relname::text, 'MATERIALIZED VIEW', obj_description(c.oid, 'pg_class') \
                 FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = $1 AND c.relkind = 'm' \
             ) relations WHERE true",
        );

        // except_tables LIKE 패턴 추가 (파라미터 바인딩)
        for i in 0..except.len() {
            query_str.push_str(&format!(" AND table_name NOT LIKE ${}", i + 2));
        }
        query_str.push_str(" ORDER BY table_name");

        // SQL 골격은 코드로만 생성하고 사용자 값(schema/except 패턴)은 전부 `$n` 바인딩한다.
        // 동적 문자열이지만 주입 위험이 없으므로 AssertSqlSafe로 감싼다 (sqlx 0.9 요구).
        let mut q = sqlx::query(sqlx::AssertSqlSafe(query_str)).bind(schema);
        for pat in except {
            q = q.bind(pat);
        }

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::MetadataQuery {
                schema: schema.to_string(),
                table: "tables".to_string(),
                source: e,
            })?;

        let mut tables = Vec::new();
        for row in rows {
            // try_get 실패 시 경고 로그 + 기본값 반환 (Requirements 5.2)
            let table_name: String = try_get_or_warn(&row, "table_name", schema, "tables");
            let table_type: String = try_get_or_warn(&row, "table_type", schema, "tables");
            let comment: Option<String> = try_get_or_warn(&row, "table_comment", schema, "tables");

            tables.push(TableDef {
                table_name,
                general: GeneralInfo {
                    table_type,
                    engine: None,     // PostgreSQL에는 engine 개념 없음
                    row_format: None, // PostgreSQL에는 row_format 개념 없음
                    collate: Some(self.db_collation.clone()), // DB 레벨 collation
                    comment,
                },
                ..Default::default()
            });
        }
        Ok(tables)
    }

    /// 컬럼 정보 조회 (BASE TABLE 전용)
    ///
    /// `information_schema.columns`와 `pg_catalog.pg_attribute`를 조인하여
    /// 컬럼 메타데이터를 수집한다. `ordinal_position` 순으로 정렬한다.
    /// - column_type: `format_type(atttypid, atttypmod)` — 정밀도·도메인·사용자 타입까지 원문 그대로
    /// - column_key: `pg_index` + `pg_attribute`로 PRI/UNI/MUL 결정
    /// - extra: `determine_pg_extra`로 identity/serial/generated 감지
    /// - charset: PostgreSQL에서는 항상 None
    /// - collation: `information_schema.columns.collation_name`
    /// - comment: `col_description(attrelid, attnum)`
    pub async fn get_columns(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, AppError> {
        // 컬럼 기본 정보 + pg_attribute 조인 쿼리
        let rows = sqlx::query(
            "SELECT \
                 c.column_name, \
                 c.column_default, \
                 c.is_nullable, \
                 format_type(a.atttypid, a.atttypmod) AS data_type, \
                 c.collation_name, \
                 a.attidentity::text AS attidentity, \
                 a.attgenerated::text AS attgenerated, \
                 c.generation_expression, \
                 col_description(a.attrelid, a.attnum) AS column_comment \
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
             WHERE c.table_schema = $1 AND c.table_name = $2 \
             ORDER BY c.ordinal_position",
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

        // column_key 결정을 위한 인덱스 키 컬럼 조회 (INCLUDE 컬럼은 키가 아니므로 제외)
        // MySQL COLUMN_KEY 와 같은 기준: PK 컬럼은 모두 PRI, 그 외 인덱스는 첫 키 컬럼만
        // 단일 컬럼 유니크면 UNI, 나머지는 MUL (복합 유니크의 첫 컬럼도 MUL)
        let key_rows = sqlx::query(
            "SELECT \
                 a.attname::text AS column_name, \
                 i.indisprimary, \
                 i.indisunique, \
                 i.indnkeyatts::int4 AS key_count, \
                 k.ord::int4 AS position \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class cl ON cl.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace ns ON ns.oid = cl.relnamespace \
             CROSS JOIN LATERAL unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
             JOIN pg_catalog.pg_attribute a \
               ON a.attrelid = i.indrelid \
               AND a.attnum = k.attnum \
               AND NOT a.attisdropped \
             WHERE ns.nspname = $1 AND cl.relname = $2 \
               AND k.attnum > 0 AND k.ord <= i.indnkeyatts",
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

        // 컬럼별 최우선 인덱스 키 결정: PRI > UNI > MUL
        use std::collections::HashMap;
        let mut column_keys: HashMap<String, String> = HashMap::new();
        for row in &key_rows {
            // try_get 실패 시 경고 로그 + 기본값 반환 (Requirements 5.2)
            let col_name: String = try_get_or_warn(row, "column_name", schema, table);
            let is_primary: bool = try_get_or_warn(row, "indisprimary", schema, table);
            let is_unique: bool = try_get_or_warn(row, "indisunique", schema, table);
            let key_count: i32 = try_get_or_warn(row, "key_count", schema, table);
            let position: i32 = try_get_or_warn(row, "position", schema, table);

            let new_key = match (is_primary, position) {
                (true, _) => "PRI",
                (false, 1) if is_unique && key_count == 1 => "UNI",
                (false, 1) => "MUL",
                // 첫 키 컬럼이 아니면 표시하지 않는다 (MySQL 과 동일)
                _ => continue,
            };

            // 우선순위: PRI > UNI > MUL (기존 값보다 높은 우선순위만 덮어씀)
            let should_update = match column_keys.get(&col_name) {
                None => true,
                Some(existing) => {
                    let priority = |k: &str| match k {
                        "PRI" => 3,
                        "UNI" => 2,
                        "MUL" => 1,
                        _ => 0,
                    };
                    priority(new_key) > priority(existing)
                }
            };
            if should_update {
                column_keys.insert(col_name, new_key.to_string());
            }
        }

        // 컬럼 정보 조립
        let mut columns = Vec::new();
        for row in &rows {
            // try_get 실패 시 경고 로그 + 기본값 반환 (Requirements 5.2)
            let column_name: String = try_get_or_warn(row, "column_name", schema, table);
            let column_default: Option<String> =
                try_get_or_warn(row, "column_default", schema, table);
            let is_nullable: String = try_get_or_warn(row, "is_nullable", schema, table);
            let column_type: String = try_get_or_warn(row, "data_type", schema, table);
            let collation_name: Option<String> =
                try_get_or_warn(row, "collation_name", schema, table);
            let attidentity: String = try_get_or_warn(row, "attidentity", schema, table);
            let attgenerated: String = try_get_or_warn(row, "attgenerated", schema, table);
            let generation_expression: Option<String> =
                try_get_or_warn(row, "generation_expression", schema, table);
            let comment: Option<String> = try_get_or_warn(row, "column_comment", schema, table);

            // extra 결정 (identity/serial/generated)
            // generated 컬럼은 MySQL 처럼 식도 함께 표시 (예: STORED GENERATED (id * 2))
            let extra = determine_pg_extra(&attidentity, &attgenerated, column_default.as_deref())
                .map(|e| match &generation_expression {
                    Some(expr) if e.ends_with("GENERATED") => format!("{e} {expr}"),
                    _ => e,
                });

            // column_key 결정
            let column_key = column_keys.get(&column_name).cloned();

            columns.push(ColumnInfo {
                column_name,
                default_value: column_default,
                nullable: is_nullable,
                column_type,
                charset: None, // PostgreSQL에서는 항상 None
                collation: collation_name,
                column_key,
                extra,
                comment,
            });
        }
        Ok(columns)
    }

    /// 인덱스 정보 조회 (BASE TABLE 전용)
    ///
    /// `pg_catalog.pg_indexes`에서 인덱스 목록을 조회하고,
    /// PRIMARY KEY 인덱스를 제외한 뒤 `indexdef`를 파싱하여
    /// 유니크 여부와 컬럼 목록을 추출한다.
    pub async fn get_indexes(&self, schema: &str, table: &str) -> Result<Vec<IndexInfo>, AppError> {
        // pg_indexes에서 인덱스 조회, PRIMARY KEY 제약 조건에 해당하는 인덱스 제외
        let rows = sqlx::query(
            "SELECT i.indexname, i.indexdef, \
                    (SELECT pg_get_constraintdef(c.oid) FROM pg_catalog.pg_constraint c \
                      WHERE c.conindid = format('%I.%I', i.schemaname, i.indexname)::regclass \
                        AND c.contype = 'x') AS exclusion_def \
             FROM pg_catalog.pg_indexes i \
             WHERE i.schemaname = $1 AND i.tablename = $2 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM pg_catalog.pg_constraint c \
                   JOIN pg_catalog.pg_namespace n ON n.oid = c.connamespace \
                   WHERE n.nspname = i.schemaname \
                     AND c.conname = i.indexname \
                     AND c.contype = 'p' \
               ) \
             ORDER BY i.indexname",
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
        for row in &rows {
            // try_get 실패 시 경고 로그 + 기본값 반환 (Requirements 5.2)
            let index_name: String = try_get_or_warn(row, "indexname", schema, table);
            let indexdef: String = try_get_or_warn(row, "indexdef", schema, table);
            // EXCLUDE 제약의 인덱스는 연산자(`during WITH &&`)가 제약 정의에만 있으므로 그쪽을 파싱
            let exclusion_def: Option<String> =
                try_get_or_warn(row, "exclusion_def", schema, table);

            // indexdef 파싱으로 유니크 여부, 컬럼 목록, 파셜 인덱스 predicate 추출
            let ParsedIndex {
                is_unique,
                columns,
                predicate,
                include,
                method,
            } = parse_pg_indexdef(exclusion_def.as_deref().unwrap_or(&indexdef));

            indexes.push(IndexInfo {
                index_name,
                non_unique: if is_unique { 0 } else { 1 },
                index_columns: columns,
                predicate,
                index_type: exclusion_def.map(|_| "EXCLUDE".to_string()).or(method),
                include_columns: include,
            });
        }
        Ok(indexes)
    }

    /// 외래 키 제약 조건 조회 (BASE TABLE 전용)
    ///
    /// DDL 재구성과 같은 `pg_constraint` 조회를 써서 다중 컬럼 FK 의 로컬·참조 컬럼을
    /// 정의 순서대로 짝지어 얻는다 (information_schema 의 constraint_column_usage 는
    /// 순서 정보가 없어 다중 컬럼 FK 의 참조 컬럼을 하나만 보여줬다).
    pub async fn get_constraints(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ConstInfo>, AppError> {
        let constraints = ddl::fetch_constraints(&self.pool, schema, table).await?;
        Ok(constraints
            .into_iter()
            .filter_map(|c| match c.constraint_type {
                PgConstraintType::ForeignKey {
                    ref_schema,
                    ref_table,
                    ref_columns,
                    on_delete,
                    on_update,
                    ..
                } => Some(ConstInfo {
                    constraint_name: c.name,
                    constraint_column: c.columns.join(", "),
                    reference: fk_reference(schema, &ref_schema, &ref_table, &ref_columns),
                    delete_action: on_delete,
                    update_action: on_update,
                }),
                _ => None,
            })
            .collect())
    }

    /// 뷰 정의 조회 (VIEW / MATERIALIZED VIEW)
    ///
    /// `pg_get_viewdef(oid, true)` OID 기반 조회로 뷰 정의 SQL을 가져온다.
    /// `pg_class` + `pg_namespace` 조인으로 뷰의 OID를 찾고 `pg_get_viewdef`를 호출한다.
    /// PostgreSQL에서는 뷰별 charset/collate 개념이 없으므로 빈 문자열로 설정한다.
    pub async fn get_view_info(&self, schema: &str, table: &str) -> Result<ViewInfo, AppError> {
        // pg_class + pg_namespace 조인으로 뷰의 OID를 찾고 pg_get_viewdef 호출
        let row = sqlx::query(
            "SELECT pg_get_viewdef(c.oid, true) AS view_def \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('v', 'm')",
        )
        .bind(schema)
        .bind(table)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::MetadataQuery {
            schema: schema.to_string(),
            table: table.to_string(),
            source: e,
        })?;

        Ok(ViewInfo {
            view_query: try_get_or_warn(&row, "view_def", schema, table),
            charset: String::new(), // PostgreSQL에서는 빈 문자열
            collate: String::new(), // PostgreSQL에서는 빈 문자열
        })
    }

    /// DDL 재구성 (SQL 포맷 전용)
    ///
    /// `information_schema.columns` + `pg_catalog.pg_constraint` + `pg_get_indexdef()`를
    /// 조합하여 CREATE TABLE DDL 문자열을 재구성한다.
    /// PostgreSQL에는 `pg_get_tabledef()` 내장 함수가 없으므로 직접 재구성한다.
    pub async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<TableDdl, AppError> {
        ddl::fetch_table_ddl(&self.pool, schema, table).await
    }

    /// 스키마 수준 객체(enum/도메인/복합 타입, 시퀀스) 생성문 — SQL 파일에서 테이블보다 먼저 실행
    pub async fn get_schema_ddl(&self, schema: &str) -> Result<Vec<String>, AppError> {
        schema_ddl::fetch_schema_ddl(&self.pool, schema).await
    }
}
