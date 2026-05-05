use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;

use crate::{
    error::AppError,
    identifier::quote_pg_identifier,
    model::{
        ColumnInfo, ConstInfo, GeneralInfo, IndexInfo, RunConfig, SchemaCatalog, TableDef, ViewInfo,
    },
};

// ─────────────────────────────────────────────────────────────────────────────
// DDL 재구성용 메타데이터 구조체
// ─────────────────────────────────────────────────────────────────────────────

/// DDL 재구성용 컬럼 메타데이터
#[derive(Debug, Clone)]
pub struct PgDdlColumn {
    /// 컬럼 이름
    pub name: String,
    /// 데이터 타입 (예: "integer", "varchar(255)")
    pub data_type: String,
    /// NULL 허용 여부 (true = NULL 허용)
    pub is_nullable: bool,
    /// 기본값 (예: "0", "'hello'", "nextval('seq'::regclass)")
    pub default_value: Option<String>,
    /// STORED generated 컬럼의 표현식 (예: "col1 + col2")
    pub generated_expression: Option<String>,
}

/// DDL 재구성용 제약 조건 종류
#[derive(Debug, Clone)]
pub enum PgConstraintType {
    /// PRIMARY KEY 제약 조건
    PrimaryKey,
    /// UNIQUE 제약 조건
    Unique,
    /// FOREIGN KEY 제약 조건
    ForeignKey {
        ref_schema: String,
        ref_table: String,
        ref_columns: Vec<String>,
        on_delete: String,
        on_update: String,
    },
    /// CHECK 제약 조건
    Check { expression: String },
}

/// DDL 재구성용 제약 조건 메타데이터
#[derive(Debug, Clone)]
pub struct PgDdlConstraint {
    /// 제약 조건 이름
    pub name: String,
    /// 제약 조건 종류
    pub constraint_type: PgConstraintType,
    /// 로컬 컬럼 목록 (CHECK 제약 조건에서는 비어있을 수 있음)
    pub columns: Vec<String>,
}

/// 테이블 메타데이터로부터 PostgreSQL DDL 문자열을 재구성한다.
///
/// 순수 함수로 구현하여 PBT 테스트가 가능하다.
/// `quote_pg_identifier`를 사용하여 스키마/테이블/컬럼 이름을 안전하게 인용한다.
///
/// DDL 구조:
/// ```sql
/// CREATE TABLE "schema"."table" (
///     "col" type [NOT NULL] [DEFAULT default] [GENERATED ALWAYS AS (expr) STORED],
///     CONSTRAINT "pk" PRIMARY KEY (columns),
///     CONSTRAINT "uq" UNIQUE (columns),
///     CONSTRAINT "fk" FOREIGN KEY (cols) REFERENCES "ref" (ref_cols) ...
///     CONSTRAINT "ck" CHECK (expression)
/// );
/// -- 인덱스
/// indexdef;
/// ```
pub fn build_pg_ddl_from_metadata(
    schema: &str,
    table: &str,
    columns: &[PgDdlColumn],
    constraints: &[PgDdlConstraint],
    index_defs: &[String],
) -> Result<String, AppError> {
    let quoted_schema = quote_pg_identifier(schema)?;
    let quoted_table = quote_pg_identifier(table)?;

    let mut ddl = format!("CREATE TABLE {quoted_schema}.{quoted_table} (\n");

    // 컬럼 정의와 제약 조건을 모두 모아서 쉼표로 구분
    let mut entries: Vec<String> = Vec::new();

    // 컬럼 정의 추가
    for col in columns {
        let quoted_col = quote_pg_identifier(&col.name)?;
        let mut col_def = format!("    {quoted_col} {}", col.data_type);

        // NOT NULL 제약
        if !col.is_nullable {
            col_def.push_str(" NOT NULL");
        }

        // GENERATED ALWAYS AS (...) STORED (기본값보다 우선)
        if let Some(ref expr) = col.generated_expression {
            col_def.push_str(&format!(" GENERATED ALWAYS AS ({expr}) STORED"));
        } else if let Some(ref default) = col.default_value {
            // DEFAULT 값 (generated 컬럼이 아닌 경우에만)
            col_def.push_str(&format!(" DEFAULT {default}"));
        }

        entries.push(col_def);
    }

    // 제약 조건 추가 (PK → UQ → FK → CK 순서)
    for c in constraints
        .iter()
        .filter(|c| matches!(c.constraint_type, PgConstraintType::PrimaryKey))
    {
        let quoted_name = quote_pg_identifier(&c.name)?;
        let cols = quote_column_list(&c.columns)?;
        entries.push(format!("    CONSTRAINT {quoted_name} PRIMARY KEY ({cols})"));
    }

    for c in constraints
        .iter()
        .filter(|c| matches!(c.constraint_type, PgConstraintType::Unique))
    {
        let quoted_name = quote_pg_identifier(&c.name)?;
        let cols = quote_column_list(&c.columns)?;
        entries.push(format!("    CONSTRAINT {quoted_name} UNIQUE ({cols})"));
    }

    for c in constraints
        .iter()
        .filter(|c| matches!(c.constraint_type, PgConstraintType::ForeignKey { .. }))
    {
        if let PgConstraintType::ForeignKey {
            ref ref_schema,
            ref ref_table,
            ref ref_columns,
            ref on_delete,
            ref on_update,
        } = c.constraint_type
        {
            let quoted_name = quote_pg_identifier(&c.name)?;
            let local_cols = quote_column_list(&c.columns)?;
            let quoted_ref_schema = quote_pg_identifier(ref_schema)?;
            let quoted_ref_table = quote_pg_identifier(ref_table)?;
            let ref_cols = quote_column_list(ref_columns)?;
            entries.push(format!(
                "    CONSTRAINT {quoted_name} FOREIGN KEY ({local_cols}) \
                 REFERENCES {quoted_ref_schema}.{quoted_ref_table} ({ref_cols}) \
                 ON DELETE {on_delete} ON UPDATE {on_update}"
            ));
        }
    }

    for c in constraints
        .iter()
        .filter(|c| matches!(c.constraint_type, PgConstraintType::Check { .. }))
    {
        if let PgConstraintType::Check { ref expression } = c.constraint_type {
            let quoted_name = quote_pg_identifier(&c.name)?;
            entries.push(format!("    CONSTRAINT {quoted_name} CHECK ({expression})"));
        }
    }

    // 엔트리들을 쉼표+개행으로 결합
    ddl.push_str(&entries.join(",\n"));
    ddl.push_str("\n);\n");

    // 인덱스 정의 추가
    for idx_def in index_defs {
        ddl.push_str(&format!("{idx_def};\n"));
    }

    Ok(ddl)
}

/// 컬럼 이름 목록을 인용하여 쉼표로 결합한다.
fn quote_column_list(columns: &[String]) -> Result<String, AppError> {
    let quoted: Result<Vec<String>, AppError> =
        columns.iter().map(|c| quote_pg_identifier(c)).collect();
    Ok(quoted?.join(", "))
}

/// PostgreSQL 시스템 스키마 목록 (정적 매칭 대상)
const PG_STATIC_SYSTEM_SCHEMAS: &[&str] = &["pg_catalog", "information_schema", "pg_toast"];

/// PostgreSQL indexdef 문자열을 파싱하여 (is_unique, columns) 튜플을 반환한다.
///
/// `pg_catalog.pg_indexes.indexdef` 컬럼의 값을 파싱하여
/// 인덱스의 유니크 여부와 컬럼 목록을 추출한다.
///
/// 예:
/// - `"CREATE UNIQUE INDEX idx ON public.table USING btree (col1, col2)"`
///   → `(true, "col1, col2")`
/// - `"CREATE INDEX idx ON public.table USING btree (col1)"`
///   → `(false, "col1")`
pub fn parse_pg_indexdef(indexdef: &str) -> (bool, String) {
    // 유니크 여부: "CREATE UNIQUE INDEX" 패턴 확인
    let is_unique = indexdef
        .to_ascii_uppercase()
        .starts_with("CREATE UNIQUE INDEX");

    // 컬럼 목록: 마지막 괄호 쌍 내부 문자열 추출
    // indexdef 형식: CREATE [UNIQUE] INDEX ... ON ... USING ... (col1, col2 [DESC], ...)
    let columns = extract_columns_from_indexdef(indexdef);

    (is_unique, columns)
}

/// indexdef 문자열에서 USING 절 이후 괄호 내부의 컬럼 목록을 추출한다.
///
/// 각 컬럼에서 ASC/DESC, NULLS FIRST/LAST 등의 수식어를 제거하고
/// 순수 컬럼 이름만 반환한다.
fn extract_columns_from_indexdef(indexdef: &str) -> String {
    // "USING {method} (" 패턴 이후의 괄호 내부를 추출
    // USING이 없는 경우 마지막 괄호 쌍을 사용
    let upper = indexdef.to_ascii_uppercase();
    let start = if let Some(using_pos) = upper.find("USING") {
        // USING 이후 첫 번째 '(' 위치
        indexdef[using_pos..].find('(').map(|p| using_pos + p)
    } else {
        indexdef.find('(')
    };

    let Some(open) = start else {
        return String::new();
    };

    // 대응하는 닫는 괄호를 찾는다 (중첩 괄호 고려)
    let mut depth = 0;
    let mut close = None;
    for (i, ch) in indexdef[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }

    let Some(close) = close else {
        return String::new();
    };

    let inner = &indexdef[open + 1..close];
    // 각 컬럼 항목을 최상위 쉼표로 분리하고 수식어 제거
    // 중첩 괄호 내부의 쉼표는 분리하지 않는다
    let parts = split_top_level_commas(inner);
    let cleaned: Vec<String> = parts
        .iter()
        .map(|col| clean_index_column(col.trim()))
        .collect();
    cleaned.join(", ")
}

/// 최상위 레벨의 쉼표로만 분리한다 (괄호 내부의 쉼표는 무시).
fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(current.clone());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// 인덱스 컬럼 표현에서 ASC/DESC, NULLS FIRST/LAST 수식어를 제거한다.
///
/// 예: `"col1 DESC NULLS FIRST"` → `"col1"`
/// 예: `"lower(name)"` → `"lower(name)"` (표현식은 그대로 유지)
fn clean_index_column(col_expr: &str) -> String {
    // 표현식(함수 호출 등)이 포함된 경우 괄호가 있으므로 그대로 반환
    if col_expr.contains('(') {
        return col_expr.to_string();
    }

    // 공백으로 분리하여 첫 번째 토큰(컬럼 이름)만 추출
    // 나머지는 ASC/DESC/NULLS FIRST/NULLS LAST 등의 수식어
    col_expr.split_whitespace().next().unwrap_or("").to_string()
}

/// PostgreSQL 컬럼 타입 문자열을 구성한다.
///
/// `udt_name`과 길이/정밀도 정보를 조합하여 사람이 읽기 쉬운 타입 문자열을 만든다.
/// - 배열 타입(`_` 접두어): `_int4` → `int4[]`
/// - 문자 길이 지정: `varchar(255)`, `bpchar` → `char({length})`
/// - 숫자 정밀도/스케일: `numeric(10,2)`
/// - 그 외: `udt_name` 그대로 반환
pub fn build_pg_column_type(
    udt_name: &str,
    char_max_length: Option<i32>,
    numeric_precision: Option<i32>,
    numeric_scale: Option<i32>,
) -> String {
    // 배열 타입: `_` 접두어를 제거하고 `[]` 접미어 추가
    if let Some(base) = udt_name.strip_prefix('_') {
        return format!("{base}[]");
    }

    // 문자 길이가 지정된 경우: `{type}({length})`
    if let Some(length) = char_max_length {
        // `bpchar`는 PostgreSQL 내부 이름이므로 `char`로 표시
        let display_name = if udt_name == "bpchar" {
            "char"
        } else {
            udt_name
        };
        return format!("{display_name}({length})");
    }

    // numeric 타입에 정밀도/스케일이 모두 지정된 경우: `numeric({p},{s})`
    if udt_name == "numeric" {
        if let (Some(precision), Some(scale)) = (numeric_precision, numeric_scale) {
            return format!("numeric({precision},{scale})");
        }
    }

    // 그 외: udt_name 그대로 반환
    udt_name.to_string()
}

/// PostgreSQL 컬럼의 extra 정보를 결정한다.
///
/// 우선순위:
/// 1. `attidentity`가 `'a'`(ALWAYS) 또는 `'d'`(BY DEFAULT) → `auto_increment`
/// 2. `column_default`에 `nextval(` 포함 (serial/bigserial) → `auto_increment`
/// 3. `attgenerated`가 `'s'`(STORED) → `STORED GENERATED`
/// 4. 그 외 → `None`
pub fn determine_pg_extra(
    attidentity: &str,
    attgenerated: &str,
    column_default: Option<&str>,
) -> Option<String> {
    // 1. identity 컬럼 감지 (ALWAYS 또는 BY DEFAULT)
    if attidentity == "a" || attidentity == "d" {
        return Some("auto_increment".to_string());
    }

    // 2. serial/bigserial 감지 (nextval 패턴)
    if let Some(default) = column_default {
        if default.contains("nextval(") {
            return Some("auto_increment".to_string());
        }
    }

    // 3. generated 컬럼 감지 (STORED만 지원, PG 13~17)
    if attgenerated == "s" {
        return Some("STORED GENERATED".to_string());
    }

    // 4. 해당 없음
    None
}

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
    /// URL 형식: `postgres://{user}:{password}@{endpoint}:{port}/{database}`
    /// 에러 발생 시 비밀번호를 포함하지 않는 `AppError::DbConnection`을 반환한다.
    pub async fn connect(config: &RunConfig) -> Result<Self, AppError> {
        let database = config.database.as_deref().unwrap_or("postgres");
        let url = format!(
            "postgres://{}:{}@{}:{}/{}",
            config.user, config.password, config.endpoint, config.port, database
        );

        // 커넥션 풀 생성 (최대 4개 연결)
        let pool = PgPoolOptions::new()
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
        // 동적 쿼리 구성: except_tables LIKE 패턴 추가
        let mut query_str = String::from(
            "SELECT t.table_name, t.table_type, \
                    obj_description(c.oid, 'pg_class') AS table_comment \
             FROM information_schema.tables t \
             LEFT JOIN pg_catalog.pg_class c \
               ON c.relname = t.table_name \
             LEFT JOIN pg_catalog.pg_namespace n \
               ON n.oid = c.relnamespace AND n.nspname = t.table_schema \
             WHERE t.table_schema = $1",
        );

        // except_tables LIKE 패턴 추가 (파라미터 바인딩)
        for i in 0..except.len() {
            query_str.push_str(&format!(" AND t.table_name NOT LIKE ${}", i + 2));
        }
        query_str.push_str(" ORDER BY t.table_name");

        let mut q = sqlx::query(&query_str).bind(schema);
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
            use sqlx::Row;
            let table_name: String = row.try_get("table_name").unwrap_or_default();
            let table_type: String = row.try_get("table_type").unwrap_or_default();
            let comment: Option<String> = row.try_get("table_comment").unwrap_or(None);

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
    /// - column_type: `build_pg_column_type`으로 구성
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
                 c.udt_name, \
                 c.character_maximum_length::int4 AS char_max_length, \
                 c.numeric_precision::int4 AS numeric_precision, \
                 c.numeric_scale::int4 AS numeric_scale, \
                 c.collation_name, \
                 a.attidentity::text AS attidentity, \
                 a.attgenerated::text AS attgenerated, \
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

        // column_key 결정을 위한 인덱스 정보 조회
        // pg_index + pg_attribute 조인으로 각 컬럼의 인덱스 참여 여부를 확인
        let key_rows = sqlx::query(
            "SELECT \
                 a.attname AS column_name, \
                 i.indisprimary, \
                 i.indisunique \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class cl ON cl.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace ns ON ns.oid = cl.relnamespace \
             JOIN pg_catalog.pg_attribute a \
               ON a.attrelid = i.indrelid \
               AND a.attnum = ANY(i.indkey) \
               AND a.attnum > 0 \
               AND NOT a.attisdropped \
             WHERE ns.nspname = $1 AND cl.relname = $2",
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
            use sqlx::Row;
            let col_name: String = row.try_get("column_name").unwrap_or_default();
            let is_primary: bool = row.try_get("indisprimary").unwrap_or(false);
            let is_unique: bool = row.try_get("indisunique").unwrap_or(false);

            let new_key = if is_primary {
                "PRI"
            } else if is_unique {
                "UNI"
            } else {
                "MUL"
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
            use sqlx::Row;
            let column_name: String = row.try_get("column_name").unwrap_or_default();
            let column_default: Option<String> = row.try_get("column_default").unwrap_or(None);
            let is_nullable: String = row.try_get("is_nullable").unwrap_or_default();
            let udt_name: String = row.try_get("udt_name").unwrap_or_default();
            let char_max_length: Option<i32> = row.try_get("char_max_length").unwrap_or(None);
            let numeric_precision: Option<i32> = row.try_get("numeric_precision").unwrap_or(None);
            let numeric_scale: Option<i32> = row.try_get("numeric_scale").unwrap_or(None);
            let collation_name: Option<String> = row.try_get("collation_name").unwrap_or(None);
            let attidentity: String = row.try_get("attidentity").unwrap_or_default();
            let attgenerated: String = row.try_get("attgenerated").unwrap_or_default();
            let comment: Option<String> = row.try_get("column_comment").unwrap_or(None);

            // 컬럼 타입 구성
            let column_type =
                build_pg_column_type(&udt_name, char_max_length, numeric_precision, numeric_scale);

            // extra 결정 (identity/serial/generated)
            let extra = determine_pg_extra(&attidentity, &attgenerated, column_default.as_deref());

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
            "SELECT i.indexname, i.indexdef \
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
            use sqlx::Row;
            let index_name: String = row.try_get("indexname").unwrap_or_default();
            let indexdef: String = row.try_get("indexdef").unwrap_or_default();

            // indexdef 파싱으로 유니크 여부와 컬럼 목록 추출
            let (is_unique, columns) = parse_pg_indexdef(&indexdef);

            indexes.push(IndexInfo {
                index_name,
                non_unique: if is_unique { 0 } else { 1 },
                index_columns: columns,
            });
        }
        Ok(indexes)
    }

    /// 외래 키 제약 조건 조회 (BASE TABLE 전용)
    ///
    /// `information_schema.table_constraints` + `key_column_usage` +
    /// `referential_constraints`를 조인하여 FOREIGN KEY 제약 조건만 수집한다.
    /// CHECK/UNIQUE 등 다른 제약 조건은 수집하지 않는다.
    pub async fn get_constraints(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ConstInfo>, AppError> {
        // FK 제약 조건 조회: table_constraints + key_column_usage +
        // constraint_column_usage + referential_constraints 조인
        let rows = sqlx::query(
            "SELECT \
                 tc.constraint_name, \
                 kcu.column_name, \
                 ccu.table_name AS ref_table, \
                 ccu.column_name AS ref_column, \
                 rc.delete_rule, \
                 rc.update_rule \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON kcu.constraint_name = tc.constraint_name \
               AND kcu.constraint_schema = tc.constraint_schema \
             JOIN information_schema.constraint_column_usage ccu \
               ON ccu.constraint_name = tc.constraint_name \
               AND ccu.constraint_schema = tc.constraint_schema \
             JOIN information_schema.referential_constraints rc \
               ON rc.constraint_name = tc.constraint_name \
               AND rc.constraint_schema = tc.constraint_schema \
             WHERE tc.table_schema = $1 \
               AND tc.table_name = $2 \
               AND tc.constraint_type = 'FOREIGN KEY' \
             ORDER BY tc.constraint_name, kcu.ordinal_position",
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

        // 동일 제약 조건의 여러 컬럼을 그룹화
        let mut constraint_map: Vec<(String, Vec<String>, String, String, String)> = Vec::new();

        for row in &rows {
            use sqlx::Row;
            let constraint_name: String = row.try_get("constraint_name").unwrap_or_default();
            let column_name: String = row.try_get("column_name").unwrap_or_default();
            let ref_table: String = row.try_get("ref_table").unwrap_or_default();
            let ref_column: String = row.try_get("ref_column").unwrap_or_default();
            let delete_rule: String = row.try_get("delete_rule").unwrap_or_default();
            let update_rule: String = row.try_get("update_rule").unwrap_or_default();

            // 이미 같은 제약 조건이 있으면 컬럼만 추가
            if let Some(existing) = constraint_map
                .iter_mut()
                .find(|(name, _, _, _, _)| name == &constraint_name)
            {
                if !existing.1.contains(&column_name) {
                    existing.1.push(column_name);
                }
            } else {
                let reference = format!("{ref_table}.{ref_column}");
                constraint_map.push((
                    constraint_name,
                    vec![column_name],
                    reference,
                    delete_rule,
                    update_rule,
                ));
            }
        }

        // ConstInfo로 변환
        let constraints = constraint_map
            .into_iter()
            .map(
                |(name, columns, reference, delete_action, update_action)| ConstInfo {
                    constraint_name: name,
                    constraint_column: columns.join(", "),
                    reference,
                    delete_action,
                    update_action,
                },
            )
            .collect();

        Ok(constraints)
    }

    /// 뷰 정의 조회 (VIEW 전용)
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
             WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind = 'v'",
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

        use sqlx::Row;
        Ok(ViewInfo {
            view_query: row.try_get("view_def").unwrap_or_default(),
            charset: String::new(), // PostgreSQL에서는 빈 문자열
            collate: String::new(), // PostgreSQL에서는 빈 문자열
        })
    }

    /// DDL 재구성 (SQL 포맷 전용)
    ///
    /// `information_schema.columns` + `pg_catalog.pg_constraint` + `pg_get_indexdef()`를
    /// 조합하여 CREATE TABLE DDL 문자열을 재구성한다.
    /// PostgreSQL에는 `pg_get_tabledef()` 내장 함수가 없으므로 직접 재구성한다.
    pub async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, AppError> {
        // 1. 컬럼 정보 조회 (ordinal_position 순)
        let col_rows = sqlx::query(
            "SELECT \
                 c.column_name, \
                 c.udt_name, \
                 c.character_maximum_length::int4 AS char_max_length, \
                 c.numeric_precision::int4 AS numeric_precision, \
                 c.numeric_scale::int4 AS numeric_scale, \
                 c.is_nullable, \
                 c.column_default, \
                 a.attgenerated::text AS attgenerated, \
                 c.generation_expression \
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

        // 컬럼 메타데이터 변환
        let mut ddl_columns: Vec<PgDdlColumn> = Vec::new();
        for row in &col_rows {
            use sqlx::Row;
            let column_name: String = row.try_get("column_name").unwrap_or_default();
            let udt_name: String = row.try_get("udt_name").unwrap_or_default();
            let char_max_length: Option<i32> = row.try_get("char_max_length").unwrap_or(None);
            let numeric_precision: Option<i32> = row.try_get("numeric_precision").unwrap_or(None);
            let numeric_scale: Option<i32> = row.try_get("numeric_scale").unwrap_or(None);
            let is_nullable: String = row.try_get("is_nullable").unwrap_or_default();
            let column_default: Option<String> = row.try_get("column_default").unwrap_or(None);
            let attgenerated: String = row.try_get("attgenerated").unwrap_or_default();
            let generation_expression: Option<String> =
                row.try_get("generation_expression").unwrap_or(None);

            // 컬럼 타입 구성
            let data_type =
                build_pg_column_type(&udt_name, char_max_length, numeric_precision, numeric_scale);

            // STORED generated 컬럼 감지
            let generated_expression = if attgenerated == "s" {
                generation_expression
            } else {
                None
            };

            ddl_columns.push(PgDdlColumn {
                name: column_name,
                data_type,
                is_nullable: is_nullable == "YES",
                default_value: column_default,
                generated_expression,
            });
        }

        // 2. 제약 조건 조회 (pg_constraint)
        let constraint_rows = sqlx::query(
            "SELECT \
                 con.conname, \
                 con.contype::text, \
                 con.conkey, \
                 con.confrelid, \
                 con.confkey, \
                 pg_get_constraintdef(con.oid) AS condef, \
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
               AND con.contype IN ('p', 'u', 'f', 'c') \
             ORDER BY \
               CASE con.contype \
                 WHEN 'p' THEN 1 \
                 WHEN 'u' THEN 2 \
                 WHEN 'f' THEN 3 \
                 WHEN 'c' THEN 4 \
               END, \
               con.conname",
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

        // 컬럼 attnum → 이름 매핑 구축
        let attnum_rows = sqlx::query(
            "SELECT a.attnum, a.attname \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class cl ON cl.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace ns ON ns.oid = cl.relnamespace \
             WHERE ns.nspname = $1 AND cl.relname = $2 \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
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

        let mut attnum_map: HashMap<i16, String> = HashMap::new();
        for row in &attnum_rows {
            use sqlx::Row;
            let attnum: i16 = row.try_get("attnum").unwrap_or(0);
            let attname: String = row.try_get("attname").unwrap_or_default();
            attnum_map.insert(attnum, attname);
        }

        // 제약 조건 메타데이터 변환
        let mut ddl_constraints: Vec<PgDdlConstraint> = Vec::new();
        for row in &constraint_rows {
            use sqlx::Row;
            let conname: String = row.try_get("conname").unwrap_or_default();
            let contype: String = row.try_get("contype").unwrap_or_default();
            let conkey: Option<Vec<i16>> = row.try_get("conkey").unwrap_or(None);
            let confkey: Option<Vec<i16>> = row.try_get("confkey").unwrap_or(None);
            let condef: String = row.try_get("condef").unwrap_or_default();
            let ref_schema: Option<String> = row.try_get("ref_schema").unwrap_or(None);
            let ref_table: Option<String> = row.try_get("ref_table").unwrap_or(None);

            // 로컬 컬럼 이름 목록 구성
            let local_columns: Vec<String> = conkey
                .unwrap_or_default()
                .iter()
                .filter_map(|num| attnum_map.get(num).cloned())
                .collect();

            let constraint_type = match contype.as_str() {
                "p" => PgConstraintType::PrimaryKey,
                "u" => PgConstraintType::Unique,
                "f" => {
                    // FK 참조 컬럼 이름 조회
                    let ref_col_names = self
                        .resolve_fk_ref_columns(
                            confkey.as_deref().unwrap_or(&[]),
                            ref_schema.as_deref().unwrap_or("public"),
                            ref_table.as_deref().unwrap_or(""),
                        )
                        .await?;

                    // ON DELETE / ON UPDATE 액션 추출
                    let (on_delete, on_update) = parse_fk_actions_from_condef(&condef);

                    PgConstraintType::ForeignKey {
                        ref_schema: ref_schema.unwrap_or_default(),
                        ref_table: ref_table.unwrap_or_default(),
                        ref_columns: ref_col_names,
                        on_delete,
                        on_update,
                    }
                }
                "c" => {
                    // 시스템 생성 CHECK 제약 조건 제외 (NOT NULL 등)
                    if conname.ends_with("_not_null") {
                        continue;
                    }
                    // pg_get_constraintdef에서 CHECK 표현식 추출
                    let expression = extract_check_expression(&condef);
                    PgConstraintType::Check { expression }
                }
                _ => continue,
            };

            ddl_constraints.push(PgDdlConstraint {
                name: conname,
                constraint_type,
                columns: local_columns,
            });
        }

        // 3. 인덱스 정의 조회 (PK/UQ 제약 조건 인덱스 제외)
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
                     AND con.contype IN ('p', 'u') \
               ) \
             ORDER BY i.indexrelid",
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

        let index_defs: Vec<String> = index_rows
            .iter()
            .map(|row| {
                use sqlx::Row;
                row.try_get::<String, _>("indexdef").unwrap_or_default()
            })
            .filter(|s| !s.is_empty())
            .collect();

        // 4. DDL 재구성
        build_pg_ddl_from_metadata(schema, table, &ddl_columns, &ddl_constraints, &index_defs)
    }

    /// FK 참조 컬럼의 attnum 배열을 컬럼 이름으로 변환한다.
    async fn resolve_fk_ref_columns(
        &self,
        confkey: &[i16],
        ref_schema: &str,
        ref_table: &str,
    ) -> Result<Vec<String>, AppError> {
        if confkey.is_empty() || ref_table.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            "SELECT a.attnum, a.attname \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class cl ON cl.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace ns ON ns.oid = cl.relnamespace \
             WHERE ns.nspname = $1 AND cl.relname = $2 \
               AND a.attnum > 0 AND NOT a.attisdropped",
        )
        .bind(ref_schema)
        .bind(ref_table)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::MetadataQuery {
            schema: ref_schema.to_string(),
            table: ref_table.to_string(),
            source: e,
        })?;

        let mut ref_map: HashMap<i16, String> = HashMap::new();
        for row in &rows {
            use sqlx::Row;
            let attnum: i16 = row.try_get("attnum").unwrap_or(0);
            let attname: String = row.try_get("attname").unwrap_or_default();
            ref_map.insert(attnum, attname);
        }

        Ok(confkey
            .iter()
            .filter_map(|num| ref_map.get(num).cloned())
            .collect())
    }
}

/// `pg_get_constraintdef` 출력에서 FK 액션을 추출한다.
///
/// 예: "FOREIGN KEY (col) REFERENCES tbl(ref_col) ON DELETE CASCADE ON UPDATE SET NULL"
/// → ("CASCADE", "SET NULL")
fn parse_fk_actions_from_condef(condef: &str) -> (String, String) {
    let upper = condef.to_ascii_uppercase();

    let on_delete = if let Some(pos) = upper.find("ON DELETE ") {
        let rest = &condef[pos + 10..];
        // 다음 "ON UPDATE" 또는 문자열 끝까지
        let end = rest
            .to_ascii_uppercase()
            .find("ON UPDATE")
            .unwrap_or(rest.len());
        rest[..end].trim().to_string()
    } else {
        "NO ACTION".to_string()
    };

    let on_update = if let Some(pos) = upper.find("ON UPDATE ") {
        condef[pos + 10..].trim().to_string()
    } else {
        "NO ACTION".to_string()
    };

    (on_delete, on_update)
}

/// `pg_get_constraintdef` 출력에서 CHECK 표현식을 추출한다.
///
/// 예: "CHECK ((age > 0))" → "(age > 0)"
fn extract_check_expression(condef: &str) -> String {
    let upper = condef.to_ascii_uppercase();
    if let Some(pos) = upper.find("CHECK (") {
        let rest = &condef[pos + 7..];
        // 마지막 닫는 괄호 제거
        if let Some(stripped) = rest.strip_suffix(')') {
            return stripped.to_string();
        }
        return rest.to_string();
    }
    condef.to_string()
}
