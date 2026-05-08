use crate::error::AppError;

/// MySQL 식별자를 백틱으로 인용한다.
/// 내부 백틱은 이중 백틱으로 이스케이프한다.
/// 위험 문자(`;`, `/*`, `*/`, 개행) 포함 시 `AppError::UnsafeIdentifier` 반환.
pub fn quote_identifier(id: &str) -> Result<String, AppError> {
    validate_identifier(id)?;
    let escaped = id.replace('`', "``");
    Ok(format!("`{escaped}`"))
}

/// 인용된 식별자에서 원본을 복원한다.
/// 입력이 백틱으로 시작/끝나지 않으면 `AppError::UnsafeIdentifier` 반환.
pub fn unquote_identifier(quoted: &str) -> Result<String, AppError> {
    if !quoted.starts_with('`') || !quoted.ends_with('`') || quoted.len() < 2 {
        return Err(AppError::UnsafeIdentifier(format!(
            "백틱으로 감싸지지 않은 식별자: {quoted}"
        )));
    }
    // 앞뒤 백틱 제거 후 이중 백틱을 단일 백틱으로 복원
    let inner = &quoted[1..quoted.len() - 1];
    Ok(inner.replace("``", "`"))
}

/// PostgreSQL 식별자를 큰따옴표로 인용한다.
/// 내부 큰따옴표(`"`)는 이중 큰따옴표(`""`)로 이스케이프한다.
/// 위험 문자(`;`, `/*`, `*/`, 개행) 또는 null 바이트(`\0`) 포함 시
/// `AppError::UnsafeIdentifier`를 반환한다.
pub fn quote_pg_identifier(id: &str) -> Result<String, AppError> {
    validate_identifier(id)?;
    if id.contains('\0') {
        return Err(AppError::UnsafeIdentifier(
            "null 바이트를 포함하는 식별자".to_string(),
        ));
    }
    let escaped = id.replace('"', "\"\"");
    Ok(format!("\"{escaped}\""))
}

/// 큰따옴표로 인용된 PostgreSQL 식별자에서 원본을 복원한다.
/// 입력이 큰따옴표로 시작/끝나지 않으면 `AppError::UnsafeIdentifier` 반환.
pub fn unquote_pg_identifier(quoted: &str) -> Result<String, AppError> {
    if !quoted.starts_with('"') || !quoted.ends_with('"') || quoted.len() < 2 {
        return Err(AppError::UnsafeIdentifier(format!(
            "큰따옴표로 감싸지지 않은 식별자: {quoted}"
        )));
    }
    // 앞뒤 큰따옴표 제거 후 이중 큰따옴표를 단일 큰따옴표로 복원
    let inner = &quoted[1..quoted.len() - 1];
    Ok(inner.replace("\"\"", "\""))
}

/// 식별자에 위험 문자(`;`, `/*`, `*/`, 개행)가 포함되어 있는지 검사한다.
pub fn validate_identifier(id: &str) -> Result<(), AppError> {
    let dangerous = [";", "/*", "*/", "\n", "\r"];
    for pat in &dangerous {
        if id.contains(pat) {
            return Err(AppError::UnsafeIdentifier(format!(
                "위험 문자 포함: {id:?}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_simple_identifier() {
        let result = quote_identifier("table_name").unwrap();
        assert_eq!(result, "`table_name`");
    }

    #[test]
    fn quote_identifier_with_backtick() {
        let result = quote_identifier("tab`le").unwrap();
        assert_eq!(result, "`tab``le`");
    }

    #[test]
    fn quote_empty_identifier() {
        let result = quote_identifier("").unwrap();
        assert_eq!(result, "``");
    }

    #[test]
    fn quote_identifier_multiple_backticks() {
        let result = quote_identifier("a`b`c").unwrap();
        assert_eq!(result, "`a``b``c`");
    }

    #[test]
    fn unquote_simple_identifier() {
        let result = unquote_identifier("`table_name`").unwrap();
        assert_eq!(result, "table_name");
    }

    #[test]
    fn unquote_identifier_with_escaped_backtick() {
        let result = unquote_identifier("`tab``le`").unwrap();
        assert_eq!(result, "tab`le");
    }

    #[test]
    fn unquote_fails_without_backticks() {
        let result = unquote_identifier("table_name");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::UnsafeIdentifier(_)));
    }

    #[test]
    fn unquote_fails_single_backtick() {
        let result = unquote_identifier("`");
        assert!(result.is_err());
    }

    #[test]
    fn unquote_empty_quoted() {
        let result = unquote_identifier("``").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn validate_safe_identifier() {
        assert!(validate_identifier("table_name").is_ok());
        assert!(validate_identifier("my_schema").is_ok());
        assert!(validate_identifier("column123").is_ok());
    }

    #[test]
    fn validate_rejects_semicolon() {
        let result = validate_identifier("table;drop");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::UnsafeIdentifier(_)));
    }

    #[test]
    fn validate_rejects_block_comment_start() {
        let result = validate_identifier("table/*comment");
        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_block_comment_end() {
        let result = validate_identifier("table*/name");
        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_newline() {
        let result = validate_identifier("table\nname");
        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_carriage_return() {
        let result = validate_identifier("table\rname");
        assert!(result.is_err());
    }

    #[test]
    fn round_trip_simple() {
        let original = "my_table";
        let quoted = quote_identifier(original).unwrap();
        let unquoted = unquote_identifier(&quoted).unwrap();
        assert_eq!(unquoted, original);
    }

    #[test]
    fn round_trip_with_backtick() {
        let original = "tab`le";
        let quoted = quote_identifier(original).unwrap();
        let unquoted = unquote_identifier(&quoted).unwrap();
        assert_eq!(unquoted, original);
    }

    #[test]
    fn round_trip_korean_identifier() {
        let original = "테이블명";
        let quoted = quote_identifier(original).unwrap();
        let unquoted = unquote_identifier(&quoted).unwrap();
        assert_eq!(unquoted, original);
    }

    // --- PostgreSQL 식별자 인용 단위 테스트 ---

    #[test]
    fn pg_quote_simple_identifier() {
        let result = quote_pg_identifier("table_name").unwrap();
        assert_eq!(result, "\"table_name\"");
    }

    #[test]
    fn pg_quote_identifier_with_double_quote() {
        let result = quote_pg_identifier("tab\"le").unwrap();
        assert_eq!(result, "\"tab\"\"le\"");
    }

    #[test]
    fn pg_quote_empty_identifier() {
        let result = quote_pg_identifier("").unwrap();
        assert_eq!(result, "\"\"");
    }

    #[test]
    fn pg_quote_rejects_null_byte() {
        let result = quote_pg_identifier("table\0name");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::UnsafeIdentifier(_)));
    }

    #[test]
    fn pg_quote_no_backtick_in_output() {
        // 백틱이 없는 일반 식별자의 인용 결과에 백틱이 포함되지 않아야 한다
        let result = quote_pg_identifier("table_name").unwrap();
        assert!(!result.contains('`'));
        // 큰따옴표로 시작/끝나야 한다
        assert!(result.starts_with('"'));
        assert!(result.ends_with('"'));
    }

    #[test]
    fn pg_unquote_simple_identifier() {
        let result = unquote_pg_identifier("\"table_name\"").unwrap();
        assert_eq!(result, "table_name");
    }

    #[test]
    fn pg_unquote_identifier_with_escaped_double_quote() {
        let result = unquote_pg_identifier("\"tab\"\"le\"").unwrap();
        assert_eq!(result, "tab\"le");
    }

    #[test]
    fn pg_unquote_fails_without_double_quotes() {
        let result = unquote_pg_identifier("table_name");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::UnsafeIdentifier(_)));
    }

    #[test]
    fn pg_unquote_fails_single_double_quote() {
        let result = unquote_pg_identifier("\"");
        assert!(result.is_err());
    }

    #[test]
    fn pg_unquote_empty_quoted() {
        let result = unquote_pg_identifier("\"\"").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn pg_round_trip_simple() {
        let original = "my_table";
        let quoted = quote_pg_identifier(original).unwrap();
        let unquoted = unquote_pg_identifier(&quoted).unwrap();
        assert_eq!(unquoted, original);
    }

    #[test]
    fn pg_round_trip_with_double_quote() {
        let original = "tab\"le";
        let quoted = quote_pg_identifier(original).unwrap();
        let unquoted = unquote_pg_identifier(&quoted).unwrap();
        assert_eq!(unquoted, original);
    }

    #[test]
    fn pg_round_trip_korean_identifier() {
        let original = "테이블명";
        let quoted = quote_pg_identifier(original).unwrap();
        let unquoted = unquote_pg_identifier(&quoted).unwrap();
        assert_eq!(unquoted, original);
    }
}
