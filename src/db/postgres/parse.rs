//! PostgreSQL 메타데이터 문자열 파싱 헬퍼 (순수 함수).
//!
//! 이 모듈은 `pg_catalog.pg_indexes.indexdef`, `pg_get_constraintdef()`
//! 등의 PostgreSQL 시스템 함수 출력 문자열을 파싱하는 순수 함수만 포함한다.
//! 외부 I/O 의존성이 없어 단위 테스트와 속성 기반 테스트(PBT)가 용이하다.
//!
//! 상위 `postgres` 모듈에서 `pub use`로 `parse_pg_indexdef` / `ParsedIndex`를
//! 재노출하여 `td_export::db::postgres::parse_pg_indexdef`의 기존 공개 경로를 유지한다.

use crate::{error::AppError, identifier::quote_pg_identifier};

/// PostgreSQL indexdef 파싱 결과.
///
/// `pg_catalog.pg_indexes.indexdef` 컬럼의 값에서 인덱스의 유니크 여부,
/// 컬럼 목록, 파셜 인덱스 predicate(`WHERE ...`)를 추출한 결과를 담는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedIndex {
    /// `CREATE UNIQUE INDEX ...` 여부.
    pub is_unique: bool,
    /// 컬럼 목록을 쉼표로 결합한 문자열. 정렬 수식어(ASC/DESC/NULLS ...)는 제거된다.
    pub columns: String,
    /// 파셜 인덱스의 `WHERE ...` 절. 존재하지 않으면 `None`.
    pub predicate: Option<String>,
}

/// PostgreSQL indexdef 문자열을 파싱하여 [`ParsedIndex`]를 반환한다.
///
/// `pg_catalog.pg_indexes.indexdef` 컬럼의 값을 파싱하여 인덱스의 유니크 여부,
/// 컬럼 목록, 파셜 인덱스 predicate(`WHERE ...`)를 추출한다.
///
/// 예:
/// - `"CREATE UNIQUE INDEX idx ON public.t USING btree (col1, col2)"`
///   → `ParsedIndex { is_unique: true, columns: "col1, col2", predicate: None }`
/// - `"CREATE INDEX idx ON public.t USING btree (col) WHERE deleted_at IS NULL"`
///   → `ParsedIndex { is_unique: false, columns: "col", predicate: Some("deleted_at IS NULL") }`
pub fn parse_pg_indexdef(indexdef: &str) -> ParsedIndex {
    // 유니크 여부: "CREATE UNIQUE INDEX" 패턴 확인
    let is_unique = indexdef
        .to_ascii_uppercase()
        .starts_with("CREATE UNIQUE INDEX");

    // 컬럼 블록의 괄호 위치를 찾는다.
    // predicate는 컬럼 블록 닫는 괄호 이후에만 등장할 수 있으므로,
    // 먼저 컬럼 블록의 경계를 확정하여 predicate 내부 괄호와의 혼동을 차단한다.
    let (columns, predicate) = match find_column_block(indexdef) {
        Some((open, close)) => {
            let inner = &indexdef[open + 1..close];
            let cols = extract_columns_from_block(inner);
            let after_block = &indexdef[close + 1..];
            let pred = extract_where_clause(after_block);
            (cols, pred)
        }
        None => (String::new(), None),
    };

    ParsedIndex {
        is_unique,
        columns,
        predicate,
    }
}

/// indexdef 문자열에서 컬럼 블록 `(...)`의 여는/닫는 괄호 바이트 인덱스를 반환한다.
///
/// `USING` 키워드 이후의 첫 번째 `(`를 여는 괄호로 간주하고,
/// 없으면 indexdef 전체의 첫 `(`를 사용한다. 중첩 괄호는 깊이 카운팅으로 처리한다.
fn find_column_block(indexdef: &str) -> Option<(usize, usize)> {
    let upper = indexdef.to_ascii_uppercase();
    let open = if let Some(using_pos) = upper.find("USING") {
        indexdef[using_pos..].find('(').map(|p| using_pos + p)?
    } else {
        indexdef.find('(')?
    };

    let mut depth: i32 = 0;
    for (i, ch) in indexdef[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, open + i));
                }
            }
            _ => {}
        }
    }
    None
}

/// 컬럼 블록 내부(괄호 제외) 문자열을 받아 정규화된 컬럼 목록 문자열을 만든다.
///
/// 최상위 쉼표로 분리한 뒤 각 항목에서 ASC/DESC, NULLS FIRST/LAST 수식어를 제거한다.
fn extract_columns_from_block(inner: &str) -> String {
    let parts = split_top_level_commas(inner);
    let cleaned: Vec<String> = parts
        .iter()
        .map(|col| clean_index_column(col.trim()))
        .collect();
    cleaned.join(", ")
}

/// 컬럼 블록 닫는 괄호 이후 문자열에서 `WHERE ...` 절을 추출한다.
///
/// 대소문자를 구분하지 않으며, `WHERE` 다음에 공백이 반드시 따라와야 한다.
/// predicate 내부는 원문 그대로(공백만 trim) 유지한다.
fn extract_where_clause(after_block: &str) -> Option<String> {
    let trimmed = after_block.trim_start();
    // "WHERE "는 6바이트 ASCII이므로 `get(..6)`로 UTF-8 경계 안전 검사.
    let prefix = trimmed.get(..6)?;
    if !prefix.eq_ignore_ascii_case("WHERE ") {
        return None;
    }
    let pred = trimmed[6..].trim();
    if pred.is_empty() {
        None
    } else {
        Some(pred.to_string())
    }
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

/// `pg_get_constraintdef` 출력에서 FK 액션을 추출한다.
///
/// 예: "FOREIGN KEY (col) REFERENCES tbl(ref_col) ON DELETE CASCADE ON UPDATE SET NULL"
/// → ("CASCADE", "SET NULL")
pub(super) fn parse_fk_actions_from_condef(condef: &str) -> (String, String) {
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
pub(super) fn extract_check_expression(condef: &str) -> String {
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

/// 컬럼 이름 목록을 인용하여 쉼표로 결합한다.
pub(super) fn quote_column_list(columns: &[String]) -> Result<String, AppError> {
    let quoted: Result<Vec<String>, AppError> =
        columns.iter().map(|c| quote_pg_identifier(c)).collect();
    Ok(quoted?.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_pg_indexdef: 기존 동작(is_unique, columns) 회귀 방지 ---

    #[test]
    fn parse_indexdef_non_unique_basic() {
        let parsed = parse_pg_indexdef("CREATE INDEX idx ON t USING btree (col)");
        assert!(!parsed.is_unique);
        assert_eq!(parsed.columns, "col");
        assert_eq!(parsed.predicate, None);
    }

    #[test]
    fn parse_indexdef_unique_multi_column_strips_sort_modifiers() {
        let parsed = parse_pg_indexdef("CREATE UNIQUE INDEX idx ON t USING btree (a, b DESC)");
        assert!(parsed.is_unique);
        assert_eq!(parsed.columns, "a, b");
        assert_eq!(parsed.predicate, None);
    }

    // --- predicate(WHERE 절) 추출 ---

    #[test]
    fn parse_indexdef_partial_index_simple_where() {
        let parsed =
            parse_pg_indexdef("CREATE INDEX idx ON t USING btree (col) WHERE deleted_at IS NULL");
        assert!(!parsed.is_unique);
        assert_eq!(parsed.columns, "col");
        assert_eq!(parsed.predicate.as_deref(), Some("deleted_at IS NULL"));
    }

    #[test]
    fn parse_indexdef_partial_unique_index_parenthesized_predicate() {
        let parsed =
            parse_pg_indexdef("CREATE UNIQUE INDEX idx ON t (col) WHERE (a > 0 AND b < 10)");
        assert!(parsed.is_unique);
        assert_eq!(parsed.columns, "col");
        assert_eq!(parsed.predicate.as_deref(), Some("(a > 0 AND b < 10)"));
    }

    #[test]
    fn parse_indexdef_where_is_case_insensitive() {
        // PostgreSQL이 소문자 `where`를 내보내는 경우는 거의 없지만 robust 파싱을 위해 확인.
        let parsed =
            parse_pg_indexdef("CREATE INDEX idx ON t USING btree (col) where deleted_at is null");
        assert_eq!(parsed.predicate.as_deref(), Some("deleted_at is null"));
    }

    #[test]
    fn parse_indexdef_expression_column_with_where() {
        // 표현식 인덱스 컬럼 블록은 중첩 괄호를 포함하므로,
        // WHERE 절이 컬럼 블록 내부로 오인되지 않아야 한다.
        let parsed = parse_pg_indexdef(
            "CREATE INDEX idx ON t USING btree (lower(name)) WHERE name IS NOT NULL",
        );
        assert_eq!(parsed.columns, "lower(name)");
        assert_eq!(parsed.predicate.as_deref(), Some("name IS NOT NULL"));
    }

    #[test]
    fn parse_indexdef_trailing_whitespace_in_predicate_is_trimmed() {
        let parsed = parse_pg_indexdef("CREATE INDEX idx ON t USING btree (col) WHERE   x > 0   ");
        assert_eq!(parsed.predicate.as_deref(), Some("x > 0"));
    }
}
