//! DB 클라이언트 추상화 모듈
//!
//! `DbClient` 트레이트를 정의하고, `DbClientEnum` 열거형으로
//! MySQL/PostgreSQL 클라이언트를 통합 디스패치한다.

pub mod mysql;
pub mod postgres;

use async_trait::async_trait;

use crate::{
    error::AppError,
    model::{
        ColumnInfo, ConstInfo, DbType, IndexInfo, RunConfig, SchemaCatalog, TableDef, ViewInfo,
    },
};

pub use mysql::MySqlClient;
pub use postgres::PgClient;

/// DB 메타데이터 조회를 추상화하는 트레이트 (MySQL/PostgreSQL 공통)
#[async_trait]
pub trait DbClient: Send + Sync {
    /// 스키마 목록 조회 (시스템 스키마 제외, target_db 필터링)
    async fn get_schemas(&self, config: &RunConfig) -> Result<SchemaCatalog, AppError>;

    /// 테이블 목록 + 일반 정보 조회 (except_tables 패턴 적용)
    async fn get_tables(&self, schema: &str, except: &[String]) -> Result<Vec<TableDef>, AppError>;

    /// 컬럼 정보 조회 (BASE TABLE 전용)
    async fn get_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, AppError>;

    /// 인덱스 정보 조회 (BASE TABLE 전용)
    async fn get_indexes(&self, schema: &str, table: &str) -> Result<Vec<IndexInfo>, AppError>;

    /// 외래 키 제약 조건 조회 (BASE TABLE 전용)
    async fn get_constraints(&self, schema: &str, table: &str) -> Result<Vec<ConstInfo>, AppError>;

    /// 뷰 정의 조회 (VIEW 전용)
    async fn get_view_info(&self, schema: &str, table: &str) -> Result<ViewInfo, AppError>;

    /// DDL 조회 (SQL 포맷 전용)
    async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, AppError>;
}

/// enum 디스패치로 MySQL/PostgreSQL 클라이언트를 통합
pub enum DbClientEnum {
    MySql(MySqlClient),
    Pg(PgClient),
}

impl DbClientEnum {
    /// RunConfig의 db_type에 따라 적절한 DB 클라이언트를 생성한다.
    pub async fn connect(config: &RunConfig) -> Result<Self, AppError> {
        match config.db_type {
            DbType::MySql => Ok(Self::MySql(MySqlClient::connect(config).await?)),
            DbType::Postgres => Ok(Self::Pg(PgClient::connect(config).await?)),
        }
    }
}

#[async_trait]
impl DbClient for DbClientEnum {
    async fn get_schemas(&self, config: &RunConfig) -> Result<SchemaCatalog, AppError> {
        match self {
            Self::MySql(c) => c.get_schemas(config).await,
            Self::Pg(c) => c.get_schemas(config).await,
        }
    }

    async fn get_tables(&self, schema: &str, except: &[String]) -> Result<Vec<TableDef>, AppError> {
        match self {
            Self::MySql(c) => c.get_tables(schema, except).await,
            Self::Pg(c) => c.get_tables(schema, except).await,
        }
    }

    async fn get_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, AppError> {
        match self {
            Self::MySql(c) => c.get_columns(schema, table).await,
            Self::Pg(c) => c.get_columns(schema, table).await,
        }
    }

    async fn get_indexes(&self, schema: &str, table: &str) -> Result<Vec<IndexInfo>, AppError> {
        match self {
            Self::MySql(c) => c.get_indexes(schema, table).await,
            Self::Pg(c) => c.get_indexes(schema, table).await,
        }
    }

    async fn get_constraints(&self, schema: &str, table: &str) -> Result<Vec<ConstInfo>, AppError> {
        match self {
            Self::MySql(c) => c.get_constraints(schema, table).await,
            Self::Pg(c) => c.get_constraints(schema, table).await,
        }
    }

    async fn get_view_info(&self, schema: &str, table: &str) -> Result<ViewInfo, AppError> {
        match self {
            Self::MySql(c) => c.get_view_info(schema, table).await,
            Self::Pg(c) => c.get_view_info(schema, table).await,
        }
    }

    async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, AppError> {
        match self {
            Self::MySql(c) => c.get_table_ddl(schema, table).await,
            Self::Pg(c) => c.get_table_ddl(schema, table).await,
        }
    }
}
