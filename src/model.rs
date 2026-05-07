use std::collections::HashMap;

/// 출력 포맷 열거형
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Excel,
    Markdown,
    Sql,
}

impl OutputFormat {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, crate::error::AppError> {
        match s.to_ascii_lowercase().as_str() {
            "excel" => Ok(Self::Excel),
            "markdown" => Ok(Self::Markdown),
            "sql" => Ok(Self::Sql),
            _ => Err(crate::error::AppError::InvalidOutputFormat(s.to_string())),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Excel => "excel",
            Self::Markdown => "markdown",
            Self::Sql => "sql",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Excel => "Excel",
            Self::Markdown => "Markdown",
            Self::Sql => "SQL",
        }
    }
}

/// 지원하는 DB 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbType {
    MySql,
    Postgres,
}

impl DbType {
    /// 문자열을 DbType으로 파싱한다. 대소문자 무관.
    /// - `"mysql"` → `MySql`
    /// - `"postgres"` | `"postgresql"` → `Postgres`
    /// - 그 외 → `AppError::InvalidDbType`
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, crate::error::AppError> {
        match s.to_ascii_lowercase().as_str() {
            "mysql" => Ok(Self::MySql),
            "postgres" | "postgresql" => Ok(Self::Postgres),
            _ => Err(crate::error::AppError::InvalidDbType(s.to_string())),
        }
    }

    /// 정규화된 문자열 표현 (CLI 기본값 매칭 용도)
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MySql => "mysql",
            Self::Postgres => "postgres",
        }
    }

    /// 사람이 읽기 좋은 표시 이름
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::MySql => "MySQL",
            Self::Postgres => "PostgreSQL",
        }
    }

    /// DB 종류별 기본 포트
    pub fn default_port(&self) -> u16 {
        match self {
            Self::MySql => 3306,
            Self::Postgres => 5432,
        }
    }
}

/// 한 번의 실행에 필요한 모든 설정값 (불변)
#[derive(Clone)]
pub struct RunConfig {
    pub endpoint: String,
    pub port: u16,
    pub user: String,
    pub password: String, // 로그 출력 금지
    pub target_db: Option<Vec<String>>,
    pub except_tables: Option<Vec<String>>,
    pub output_format: OutputFormat,
    pub db_type: DbType,
    /// PostgreSQL 전용: 접속할 데이터베이스 이름. MySQL에서는 사용하지 않음.
    pub database: Option<String>,
}

/// Debug 구현에서 password 필드를 [REDACTED]로 대체
impl std::fmt::Debug for RunConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunConfig")
            .field("endpoint", &self.endpoint)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"[REDACTED]")
            .field("target_db", &self.target_db)
            .field("except_tables", &self.except_tables)
            .field("output_format", &self.output_format)
            .field("db_type", &self.db_type)
            .field("database", &self.database)
            .finish()
    }
}

/// 스키마 → 테이블 목록 맵
pub type SchemaCatalog = HashMap<String, Vec<TableDef>>;

/// 한 테이블/뷰의 메타데이터 집합 (Go의 PerTable 대응)
#[derive(Debug, Clone, Default)]
pub struct TableDef {
    pub table_name: String,
    pub general: GeneralInfo,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
    pub constraints: Vec<ConstInfo>,
    pub view: Option<ViewInfo>,
    pub ddl: Option<String>,
}

/// 테이블 일반 정보
#[derive(Debug, Clone, Default)]
pub struct GeneralInfo {
    pub table_type: String, // "BASE TABLE" 또는 "VIEW"
    pub engine: Option<String>,
    pub row_format: Option<String>,
    pub collate: Option<String>,
    pub comment: Option<String>,
}

/// 컬럼 정보
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub column_name: String,
    pub default_value: Option<String>,
    pub nullable: String, // "YES" 또는 "NO"
    pub column_type: String,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub column_key: Option<String>,
    pub extra: Option<String>,
    pub comment: Option<String>,
}

/// 인덱스 정보
#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub index_name: String,
    pub non_unique: i32,       // 1 = Normal, 0 = Unique
    pub index_columns: String, // 쉼표 구분 컬럼 목록
}

/// 외래 키 제약 조건 정보
#[derive(Debug, Clone)]
pub struct ConstInfo {
    pub constraint_name: String,
    pub constraint_column: String,
    pub reference: String, // "{table}.{column}" 형식
    pub delete_action: String,
    pub update_action: String,
}

/// 뷰 정의 정보
#[derive(Debug, Clone)]
pub struct ViewInfo {
    pub view_query: String,
    pub charset: String,
    pub collate: String,
}
