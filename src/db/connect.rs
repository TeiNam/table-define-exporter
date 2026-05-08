//! DB 연결 옵션(`ConnectOptions`) 빌더 모듈.
//!
//! MySQL / PostgreSQL 커넥션을 만들 때 **URL 문자열 포매팅을 사용하지 않고**
//! sqlx의 타입 안전한 빌더(`MySqlConnectOptions`, `PgConnectOptions`)로 자격
//! 증명을 전달하기 위한 공용 함수를 제공한다.
//!
//! # 왜 URL 포매팅을 쓰지 않는가
//!
//! `mysql://user:pass@host:port/db` 같은 URL은 비밀번호에 `@`, `:`, `/`, `?`,
//! `#`, `%`, 공백 등 URL 예약 문자가 포함되면 파싱이 깨지거나, 사전에 퍼센트
//! 인코딩해 주지 않으면 잘못된 자격 증명이 전달된다. 빌더 API는 값을 그대로
//! 받기 때문에 이스케이프 실수를 원천적으로 차단하며, 로그·에러 메시지에
//! 전체 커넥션 문자열이 조립되어 유출될 위험도 줄인다.
//!
//! # 호환성
//!
//! - `MySqlPool::connect_with(options)` / `PgPool::connect_with(options)` 는 기존
//!   `connect(&url)` 과 동일한 풀을 만든다. 호출부에서는 한 줄만 교체하면 된다.
//! - 반환 값은 sqlx 타입이므로 이후 `.log_statements(...)` 등 sqlx 옵션 체이닝이
//!   그대로 가능하다.

use sqlx::mysql::MySqlConnectOptions;
use sqlx::postgres::PgConnectOptions;

use crate::model::RunConfig;

/// [`RunConfig`]로부터 MySQL 접속 옵션을 빌드한다.
///
/// 메타데이터 조회가 목적이므로 기본 데이터베이스는 `information_schema`로
/// 고정한다. 비밀번호는 [`crate::secret::Password::expose`]로 명시적으로 노출한
/// 뒤 sqlx 내부에 전달된다 — 이 경로 외에는 원문이 외부로 흘러가지 않는다.
///
/// # Examples
///
/// ```
/// use td_export::db::connect::mysql_options;
/// use td_export::model::{DbType, OutputFormat, RunConfig};
/// use td_export::secret::Password;
///
/// let cfg = RunConfig {
///     endpoint: "db.example.com".to_string(),
///     port: 3306,
///     user: "root".to_string(),
///     password: Password::new("p@ss:w/ord".to_string()),
///     target_db: None,
///     except_tables: None,
///     output_format: OutputFormat::Excel,
///     db_type: DbType::MySql,
///     database: None,
/// };
/// let _options = mysql_options(&cfg);
/// ```
pub fn mysql_options(config: &RunConfig) -> MySqlConnectOptions {
    MySqlConnectOptions::new()
        .host(&config.endpoint)
        .port(config.port)
        .username(&config.user)
        .password(config.password.expose())
        .database("information_schema")
}

/// [`RunConfig`]로부터 PostgreSQL 접속 옵션을 빌드한다.
///
/// `config.database`가 주어지면 그 값을, 없으면 PostgreSQL 기본 DB(`postgres`)를
/// 사용한다. `application_name`은 `td-export`로 고정하여 서버 측 로그·pg_stat_activity
/// 에서 본 도구의 세션을 쉽게 식별할 수 있게 한다.
///
/// # Examples
///
/// ```
/// use td_export::db::connect::pg_options;
/// use td_export::model::{DbType, OutputFormat, RunConfig};
/// use td_export::secret::Password;
///
/// let cfg = RunConfig {
///     endpoint: "db.example.com".to_string(),
///     port: 5432,
///     user: "postgres".to_string(),
///     password: Password::new("p@ss:w/ord".to_string()),
///     target_db: None,
///     except_tables: None,
///     output_format: OutputFormat::Sql,
///     db_type: DbType::Postgres,
///     database: Some("app".to_string()),
/// };
/// let _options = pg_options(&cfg);
/// ```
pub fn pg_options(config: &RunConfig) -> PgConnectOptions {
    // database가 None이면 PostgreSQL 기본 DB(`postgres`)로 접속해 카탈로그를 조회한다.
    let db = config.database.as_deref().unwrap_or("postgres");
    PgConnectOptions::new()
        .host(&config.endpoint)
        .port(config.port)
        .username(&config.user)
        .password(config.password.expose())
        .database(db)
        .application_name("td-export")
}
