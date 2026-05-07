use std::io::{self, BufRead, Write};

use crate::{
    error::AppError,
    model::{DbType, OutputFormat, RunConfig},
};

/// CLI에서 전달된 선택적 오버라이드 값.
///
/// 각 필드가 `Some`이면 대화형 프롬프트를 건너뛰고 해당 값을 그대로 사용한다.
/// `None`이면 기존과 동일하게 대화형 프롬프트로 사용자에게 묻는다.
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub output_format: Option<OutputFormat>,
    pub db_type: Option<DbType>,
    pub endpoint: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub database: Option<String>,
    pub target_db: Option<Vec<String>>,
    pub except_tables: Option<Vec<String>>,
}

/// 프롬프트를 출력하고 stdin에서 한 줄을 읽어 반환합니다.
/// 줄 끝의 개행 문자(\n, \r)는 제거됩니다.
fn prompt_and_read(prompt: &str) -> Result<String, AppError> {
    print!("{}", prompt);
    io::stdout()
        .flush()
        .map_err(|e| AppError::InputRead { source: e })?;
    let stdin = io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .map_err(|e| AppError::InputRead { source: e })?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

/// 포트 문자열을 지정된 기본값으로 파싱한다.
/// - 빈 문자열 → `default`
/// - 그 외 → u16 파싱 후 1..=65535 범위 검증
pub(crate) fn parse_port_with_default(input: &str, default: u16) -> Result<u16, AppError> {
    if input.is_empty() {
        return Ok(default);
    }
    let n: u32 = input
        .parse()
        .map_err(|_| AppError::InvalidPort(input.to_string()))?;
    if !(1..=65535).contains(&n) {
        return Err(AppError::InvalidPort(input.to_string()));
    }
    Ok(n as u16)
}

/// 기존 호환: MySQL 기본 포트(3306)를 기준으로 파싱한다. (테스트 전용)
#[cfg(test)]
pub(crate) fn parse_port(input: &str) -> Result<u16, AppError> {
    parse_port_with_default(input, DbType::MySql.default_port())
}

/// 쉼표 구분 문자열을 파싱합니다.
/// - 빈 문자열(공백 포함) → None
/// - 그 외 → 쉼표로 분리 후 각 항목 trim → Some(Vec<String>)
pub(crate) fn parse_comma_separated(input: &str) -> Option<Vec<String>> {
    if input.trim().is_empty() {
        None
    } else {
        Some(input.split(',').map(|s| s.trim().to_string()).collect())
    }
}

/// CLI 오버라이드가 없으면 대화형 프롬프트로 출력 포맷을 묻는다.
fn resolve_output_format(override_val: Option<OutputFormat>) -> Result<OutputFormat, AppError> {
    if let Some(fmt) = override_val {
        return Ok(fmt);
    }
    let input = prompt_and_read("Output Format (excel/markdown/sql, default: markdown) : ")?;
    if input.is_empty() {
        Ok(OutputFormat::Markdown)
    } else {
        OutputFormat::from_str(&input)
    }
}

/// CLI 오버라이드가 없으면 대화형 프롬프트로 DB 종류를 묻는다.
fn resolve_db_type(override_val: Option<DbType>) -> Result<DbType, AppError> {
    if let Some(db) = override_val {
        return Ok(db);
    }
    let input = prompt_and_read("DB Type (mysql/postgres, default: mysql) : ")?;
    if input.is_empty() {
        Ok(DbType::MySql)
    } else {
        DbType::from_str(&input)
    }
}

/// 대화식으로 사용자 입력을 받아 RunConfig를 생성합니다.
/// CLI 플래그로 지정된 값은 그대로 사용하고, 지정되지 않은 값만 대화형으로 묻습니다.
pub fn load_config(overrides: CliOverrides) -> Result<RunConfig, AppError> {
    // 1. 출력 포맷
    let output_format = resolve_output_format(overrides.output_format)?;

    // 2. DB 종류 (포트 기본값 결정에 필요하므로 먼저 확정)
    let db_type = resolve_db_type(overrides.db_type)?;

    // 3. Endpoint
    let endpoint = match overrides.endpoint {
        Some(v) if !v.is_empty() => v,
        _ => {
            let v = prompt_and_read("Endpoint : ")?;
            if v.is_empty() {
                return Err(AppError::MissingInput("Endpoint".to_string()));
            }
            v
        }
    };

    // 4. Port (DB 종류별 기본값 사용)
    let default_port = db_type.default_port();
    let port = match overrides.port {
        Some(p) => p,
        None => {
            let port_str = prompt_and_read(&format!("Port (default: {}) : ", default_port))?;
            parse_port_with_default(&port_str, default_port)?
        }
    };

    // 5. User
    let user = match overrides.user {
        Some(v) if !v.is_empty() => v,
        _ => {
            let v = prompt_and_read("User : ")?;
            if v.is_empty() {
                return Err(AppError::MissingInput("User".to_string()));
            }
            v
        }
    };

    // 6. Password (에코 없이 읽기 - CLI 오버라이드 없음: 보안상 항상 프롬프트)
    print!("Password : ");
    io::stdout()
        .flush()
        .map_err(|e| AppError::InputRead { source: e })?;
    let password = rpassword::read_password().map_err(|e| AppError::InputRead { source: e })?;

    // 7. Database (PostgreSQL 전용 필수 입력)
    let database = match db_type {
        DbType::Postgres => {
            let v = match overrides.database {
                Some(v) if !v.is_empty() => v,
                _ => {
                    let v = prompt_and_read("Database : ")?;
                    if v.is_empty() {
                        return Err(AppError::MissingInput("Database".to_string()));
                    }
                    v
                }
            };
            Some(v)
        }
        DbType::MySql => None,
    };

    // 8. DB (쉼표 구분, 빈 입력 시 None)
    let target_db = match overrides.target_db {
        Some(v) => Some(v),
        None => {
            let db_str = prompt_and_read("DB(Seperator , or Space(All)) : ")?;
            parse_comma_separated(&db_str)
        }
    };

    // 9. Exception Tables (쉼표 구분, 빈 입력 시 None)
    let except_tables = match overrides.except_tables {
        Some(v) => Some(v),
        None => {
            let except_str =
                prompt_and_read("Exception Tables(Seperator , or Space(none) / Use wildcard) : ")?;
            parse_comma_separated(&except_str)
        }
    };

    Ok(RunConfig {
        endpoint,
        port,
        user,
        password,
        target_db,
        except_tables,
        output_format,
        db_type,
        database,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parse_port_empty_returns_default_3306() {
        assert_eq!(parse_port("").unwrap(), 3306);
    }

    #[test]
    fn parse_port_with_default_empty_returns_custom_default() {
        assert_eq!(parse_port_with_default("", 5432).unwrap(), 5432);
        assert_eq!(parse_port_with_default("", 3306).unwrap(), 3306);
    }

    #[test]
    fn parse_port_with_default_valid_number_ignores_default() {
        assert_eq!(parse_port_with_default("8080", 5432).unwrap(), 8080);
    }

    #[test]
    fn parse_port_valid_number_returns_u16() {
        assert_eq!(parse_port("3306").unwrap(), 3306);
        assert_eq!(parse_port("1").unwrap(), 1);
        assert_eq!(parse_port("65535").unwrap(), 65535);
        assert_eq!(parse_port("8080").unwrap(), 8080);
    }

    #[test]
    fn parse_port_zero_returns_error() {
        let err = parse_port("0").unwrap_err();
        assert!(err.to_string().contains("0"));
    }

    #[test]
    fn parse_port_above_max_returns_error() {
        let err = parse_port("65536").unwrap_err();
        assert!(err.to_string().contains("65536"));
    }

    #[test]
    fn parse_port_non_numeric_returns_error() {
        let err = parse_port("abc").unwrap_err();
        assert!(err.to_string().contains("abc"));
    }

    #[test]
    fn parse_port_negative_string_returns_error() {
        let err = parse_port("-1").unwrap_err();
        assert!(err.to_string().contains("-1"));
    }

    #[test]
    fn parse_comma_separated_empty_returns_none() {
        assert!(parse_comma_separated("").is_none());
    }

    #[test]
    fn parse_comma_separated_whitespace_only_returns_none() {
        assert!(parse_comma_separated("   ").is_none());
    }

    #[test]
    fn parse_comma_separated_single_item_returns_vec() {
        let result = parse_comma_separated("mydb").unwrap();
        assert_eq!(result, vec!["mydb"]);
    }

    #[test]
    fn parse_comma_separated_multiple_items_splits_correctly() {
        let result = parse_comma_separated("db1,db2,db3").unwrap();
        assert_eq!(result, vec!["db1", "db2", "db3"]);
    }

    #[test]
    fn parse_comma_separated_trims_whitespace_around_items() {
        let result = parse_comma_separated(" db1 , db2 , db3 ").unwrap();
        assert_eq!(result, vec!["db1", "db2", "db3"]);
    }

    #[test]
    fn parse_comma_separated_wildcard_pattern_preserved() {
        let result = parse_comma_separated("tmp_*,test_%").unwrap();
        assert_eq!(result, vec!["tmp_*", "test_%"]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn port_parse_totality(s in ".*") {
            let _ = parse_port(&s);
        }

        #[test]
        fn port_parse_valid_range(n in 1u16..=65535u16) {
            let s = n.to_string();
            let result = parse_port(&s).unwrap();
            prop_assert_eq!(result, n);
        }

        #[test]
        fn port_parse_empty_returns_3306(
            _dummy in 0u8..=255u8,
        ) {
            prop_assert_eq!(parse_port("").unwrap(), 3306);
        }

        #[test]
        fn port_parse_with_default_empty_returns_default(
            default in 1u16..=65535u16,
        ) {
            prop_assert_eq!(parse_port_with_default("", default).unwrap(), default);
        }

        #[test]
        fn comma_separated_empty_is_none(
            spaces in " {0,10}",
        ) {
            prop_assert!(parse_comma_separated(&spaces).is_none());
        }

        #[test]
        fn comma_separated_nonempty_is_some(
            items in proptest::collection::vec("[a-zA-Z0-9_]{1,20}", 1..=5),
        ) {
            let input = items.join(",");
            let result = parse_comma_separated(&input);
            prop_assert!(result.is_some());
            let vec = result.unwrap();
            prop_assert_eq!(vec.len(), items.len());
            for (parsed, original) in vec.iter().zip(items.iter()) {
                prop_assert_eq!(parsed, original);
            }
        }
    }
}
