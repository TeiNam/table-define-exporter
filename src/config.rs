use std::io::{self, BufRead, Write};

use crate::{
    error::AppError,
    model::{OutputFormat, RunConfig},
};

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

/// 포트 문자열을 파싱합니다.
/// - 빈 문자열 → 기본값 3306
/// - 그 외 → u16 파싱 후 1..=65535 범위 검증
pub(crate) fn parse_port(input: &str) -> Result<u16, AppError> {
    if input.is_empty() {
        return Ok(3306);
    }
    let n: u32 = input
        .parse()
        .map_err(|_| AppError::InvalidPort(input.to_string()))?;
    if !(1..=65535).contains(&n) {
        return Err(AppError::InvalidPort(input.to_string()));
    }
    Ok(n as u16)
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

/// 대화식으로 사용자 입력을 받아 RunConfig를 생성합니다.
pub fn load_config(output_format: OutputFormat) -> Result<RunConfig, AppError> {
    // 1. Endpoint
    let endpoint = prompt_and_read("Endpoint : ")?;
    if endpoint.is_empty() {
        return Err(AppError::MissingInput("Endpoint".to_string()));
    }

    // 2. Port (기본값: 3306)
    let port_str = prompt_and_read("Port (default: 3306) : ")?;
    let port = parse_port(&port_str)?;

    // 3. User
    let user = prompt_and_read("User : ")?;
    if user.is_empty() {
        return Err(AppError::MissingInput("User".to_string()));
    }

    // 4. Password (에코 없이 읽기)
    print!("Password : ");
    io::stdout()
        .flush()
        .map_err(|e| AppError::InputRead { source: e })?;
    let password = rpassword::read_password().map_err(|e| AppError::InputRead { source: e })?;

    // 5. DB (쉼표 구분, 빈 입력 시 None)
    let db_str = prompt_and_read("DB(Seperator , or Space(All)) : ")?;
    let target_db = parse_comma_separated(&db_str);

    // 6. Exception Tables (쉼표 구분, 빈 입력 시 None)
    let except_str =
        prompt_and_read("Exception Tables(Seperator , or Space(none) / Use wildcard) : ")?;
    let except_tables = parse_comma_separated(&except_str);

    Ok(RunConfig {
        endpoint,
        port,
        user,
        password,
        target_db,
        except_tables,
        output_format,
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
