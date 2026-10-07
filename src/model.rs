use std::collections::HashMap;
use std::str::FromStr;

/// 출력 포맷 열거형
///
/// `clap::ValueEnum`을 구현하여 CLI 인자로 직접 파싱할 수 있다.
/// `#[value(name = "...")]`로 기본 kebab-case 변환을 무시하고
/// 기존 소문자 이름(`excel`, `markdown`, `sql`)을 유지한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    #[value(name = "excel")]
    Excel,
    #[value(name = "markdown")]
    Markdown,
    #[value(name = "sql")]
    Sql,
}

impl FromStr for OutputFormat {
    type Err = crate::error::AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "excel" => Ok(Self::Excel),
            "markdown" => Ok(Self::Markdown),
            "sql" => Ok(Self::Sql),
            _ => Err(crate::error::AppError::InvalidOutputFormat(s.to_string())),
        }
    }
}

impl OutputFormat {
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
///
/// `clap::ValueEnum`을 구현하여 CLI 인자로 직접 파싱할 수 있다.
/// - `MySql`은 기본 kebab-case 변환이 `my-sql`이 되므로 `#[value(name = "mysql")]`로
///   기존 CLI 이름(`mysql`)을 유지한다.
/// - `Postgres`는 `postgres`가 기본이고, `postgresql` 별칭도 허용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum DbType {
    #[value(name = "mysql")]
    MySql,
    #[value(name = "postgres", alias = "postgresql")]
    Postgres,
}

impl FromStr for DbType {
    type Err = crate::error::AppError;

    /// 문자열을 DbType으로 파싱한다. 대소문자 무관.
    /// - `"mysql"` → `MySql`
    /// - `"postgres"` | `"postgresql"` → `Postgres`
    /// - 그 외 → `AppError::InvalidDbType`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "mysql" => Ok(Self::MySql),
            "postgres" | "postgresql" => Ok(Self::Postgres),
            _ => Err(crate::error::AppError::InvalidDbType(s.to_string())),
        }
    }
}

impl DbType {
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
    /// 비밀번호. `Password` 래퍼가 Debug/Display 시점에 자동 마스킹하므로
    /// 로그·에러 경로에서 원문이 노출되지 않는다. 원문 접근은
    /// `password.expose()`로 의도적으로만 가능하다.
    pub password: crate::secret::Password,
    pub target_db: Option<Vec<String>>,
    pub except_tables: Option<Vec<String>>,
    pub output_format: OutputFormat,
    pub db_type: DbType,
    /// PostgreSQL 전용: 접속할 데이터베이스 이름. MySQL에서는 사용하지 않음.
    pub database: Option<String>,
}

/// Debug 구현. password 필드는 `Password` 타입 자체의 `Debug`가
/// `Password([REDACTED])`로 마스킹하므로 그대로 위임한다.
impl std::fmt::Debug for RunConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunConfig")
            .field("endpoint", &self.endpoint)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password)
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
    /// 모든 테이블의 CREATE 뒤에 실행할 문장 (PostgreSQL FK 의 `ALTER TABLE ... ADD CONSTRAINT`)
    pub ddl_after: Vec<String>,
}

/// SQL 출력용 테이블 DDL
#[derive(Debug, Clone, Default)]
pub struct TableDdl {
    /// `CREATE TABLE` / `CREATE VIEW` 문
    pub create: String,
    /// 모든 테이블을 만든 뒤 실행할 문장 (참조 대상이 먼저 있어야 하는 FK 등)
    pub after: Vec<String>,
}

impl TableDef {
    /// 이 TableDef가 차지하는 대략적인 힙 메모리(바이트).
    /// ponytail: 정확한 할당량이 아니라 문자열 길이 합산 기반 추정치 — 진행 표시용.
    pub fn estimated_size(&self) -> usize {
        let opt = |o: &Option<String>| o.as_ref().map_or(0, String::len);
        let mut total = std::mem::size_of::<TableDef>() + self.table_name.len();
        total += self.general.table_type.len()
            + opt(&self.general.engine)
            + opt(&self.general.row_format)
            + opt(&self.general.collate)
            + opt(&self.general.comment);
        for c in &self.columns {
            total += std::mem::size_of::<ColumnInfo>()
                + c.column_name.len()
                + c.nullable.len()
                + c.column_type.len()
                + opt(&c.default_value)
                + opt(&c.charset)
                + opt(&c.collation)
                + opt(&c.column_key)
                + opt(&c.extra)
                + opt(&c.comment);
        }
        for i in &self.indexes {
            total += std::mem::size_of::<IndexInfo>() + i.index_name.len() + i.index_columns.len();
        }
        for k in &self.constraints {
            total += std::mem::size_of::<ConstInfo>()
                + k.constraint_name.len()
                + k.constraint_column.len()
                + k.reference.len()
                + k.delete_action.len()
                + k.update_action.len();
        }
        if let Some(v) = &self.view {
            total += v.view_query.len() + v.charset.len() + v.collate.len();
        }
        total + opt(&self.ddl) + self.ddl_after.iter().map(String::len).sum::<usize>()
    }
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

impl ColumnInfo {
    /// 정의서에 표시할 기본값.
    /// - 기본값 없음 + nullable → `NULL` (DB 가 암묵적으로 DEFAULT NULL 적용)
    /// - 기본값 없음 + NOT NULL, 또는 generated 컬럼 → 빈칸 (기본값 자체가 없음)
    /// - 빈 문자열 기본값 → `''` (NULL 과 구분)
    pub fn display_default(&self) -> &str {
        match self.default_value.as_deref() {
            None if self.nullable == "YES" && !self.is_generated() => "NULL",
            None => "",
            Some("") => "''",
            Some(v) => v,
        }
    }
    /// generated 컬럼 여부. extra 표기는 MySQL/PG 공통으로 `VIRTUAL GENERATED` /
    /// `STORED GENERATED` (MySQL 은 뒤에 표현식이 붙음). 표현식 기본값을 뜻하는
    /// MySQL `DEFAULT_GENERATED` 와는 구분한다.
    fn is_generated(&self) -> bool {
        self.extra
            .as_deref()
            .is_some_and(|e| e.contains("VIRTUAL GENERATED") || e.contains("STORED GENERATED"))
    }
}

/// 인덱스 정보
#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub index_name: String,
    pub non_unique: i32,       // 1 = Normal, 0 = Unique
    pub index_columns: String, // 쉼표 구분 컬럼 목록 (DESC·prefix 길이·함수식 포함)
    /// 파셜 인덱스의 WHERE 절 predicate (없으면 None)
    pub predicate: Option<String>,
    /// MySQL `INDEX_TYPE` (BTREE/HASH/FULLTEXT/SPATIAL). PostgreSQL 은 None
    pub index_type: Option<String>,
}

impl IndexInfo {
    /// 정의서에 표시할 인덱스 종류: Fulltext / Spatial / Unique / Normal
    pub fn kind_label(&self) -> &'static str {
        match self.index_type.as_deref() {
            Some("FULLTEXT") => "Fulltext",
            Some("SPATIAL") => "Spatial",
            _ if self.non_unique == 0 => "Unique",
            _ => "Normal",
        }
    }
}

/// 외래 키 제약 조건 정보
#[derive(Debug, Clone)]
pub struct ConstInfo {
    pub constraint_name: String,
    pub constraint_column: String,
    pub reference: String, // [`fk_reference`] 형식
    pub delete_action: String,
    pub update_action: String,
}

/// FK 참조 대상 표기: 단일 컬럼 `table.col`, 다중 컬럼 `table(c1, c2)`.
/// 다른 스키마의 테이블을 참조하면 `schema.` 를 앞에 붙인다.
pub fn fk_reference(
    schema: &str,
    ref_schema: &str,
    ref_table: &str,
    ref_columns: &[String],
) -> String {
    let table = if ref_schema.is_empty() || ref_schema == schema {
        ref_table.to_string()
    } else {
        format!("{ref_schema}.{ref_table}")
    };
    match ref_columns {
        [single] => format!("{table}.{single}"),
        columns => format!("{table}({})", columns.join(", ")),
    }
}

/// 뷰 정의 정보
#[derive(Debug, Clone)]
pub struct ViewInfo {
    pub view_query: String,
    pub charset: String,
    pub collate: String,
}
