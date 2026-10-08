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
    /// 컬럼 목록을 쉼표로 결합한 문자열. DESC·NULLS·opclass 등은 유지하고 기본값 ASC 만 제거한다.
    pub columns: String,
    /// 파셜 인덱스의 `WHERE ...` 절. 존재하지 않으면 `None`.
    pub predicate: Option<String>,
    /// 커버링 인덱스의 `INCLUDE (...)` 컬럼 목록. 없으면 `None`.
    pub include: Option<String>,
    /// 인덱스 방식 (`USING btree` 의 btree, 대문자). 없으면 `None`.
    pub method: Option<String>,
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
    let (columns, include, predicate) = match find_column_block(indexdef) {
        Some((open, close)) => {
            let inner = &indexdef[open + 1..close];
            let cols = extract_columns_from_block(inner);
            // pg_get_indexdef 순서: (cols) [INCLUDE (..)] [NULLS NOT DISTINCT] [WITH (..)]
            // [TABLESPACE ..] [WHERE ..] — INCLUDE 뒤에 WHERE 가 와도 놓치지 않도록 따로 찾는다
            let (include, rest) = split_include(&indexdef[close + 1..]);
            (cols, include, extract_where_clause(rest))
        }
        None => (String::new(), None, None),
    };
    let method = indexdef
        .to_ascii_uppercase()
        .split_once("USING ")
        .and_then(|(_, rest)| rest.split_whitespace().next().map(str::to_string));

    ParsedIndex {
        is_unique,
        columns,
        predicate,
        include,
        method,
    }
}

/// 컬럼 블록 뒤가 `INCLUDE (...)` 로 시작하면 그 목록과 나머지를 나눈다.
fn split_include(after_block: &str) -> (Option<String>, &str) {
    let trimmed = after_block.trim_start();
    if !trimmed.to_ascii_uppercase().starts_with("INCLUDE (") {
        return (None, after_block);
    }
    let open = "INCLUDE ".len();
    match closing_paren(trimmed, open) {
        Some(close) => (
            Some(trimmed[open + 1..close].trim().to_string()),
            &trimmed[close + 1..],
        ),
        None => (None, after_block),
    }
}

/// `s[open]` 의 `(` 와 짝이 맞는 `)` 위치 (따옴표 식별자·문자열 안의 괄호는 무시).
fn closing_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (i, c) in s[open..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
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
/// 최상위 쉼표로 분리한 뒤 각 항목에서 기본값인 ASC 수식어만 제거한다.
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
    // WHERE 앞에 NULLS NOT DISTINCT / WITH (..) / TABLESPACE 가 올 수 있으므로
    // 괄호 밖에서 공백 뒤에 오는 첫 `WHERE ` 를 찾는다 (WHERE 는 항상 마지막 절).
    let mut depth = 0;
    let mut prev_is_space = true;
    for (i, c) in after_block.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if depth == 0
                && prev_is_space
                && after_block
                    .get(i..i + 6)
                    .is_some_and(|w| w.eq_ignore_ascii_case("WHERE ")) =>
            {
                let pred = after_block[i + 6..].trim();
                return (!pred.is_empty()).then(|| pred.to_string());
            }
            _ => {}
        }
        prev_is_space = c.is_whitespace();
    }
    None
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

/// 인덱스 컬럼 표현에서 기본값인 `ASC` 만 제거한다 (DESC·NULLS·opclass·COLLATE 는 정보라 유지).
///
/// 예: `"col1 DESC NULLS FIRST"` → `"col1 DESC NULLS FIRST"`
/// 예: `"score ASC"` → `"score"`
/// 예: `"lower(name)"` → `"lower(name)"` (표현식은 그대로 유지)
fn clean_index_column(col_expr: &str) -> String {
    // 표현식(함수 호출 등)이 포함된 경우 괄호가 있으므로 그대로 반환
    if col_expr.contains('(') {
        return col_expr.to_string();
    }

    col_expr
        .split_whitespace()
        .filter(|token| !token.eq_ignore_ascii_case("ASC"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `pg_get_constraintdef` 출력에서 FK 액션을 추출한다.
///
/// 예: "FOREIGN KEY (col) REFERENCES tbl(ref_col) ON DELETE CASCADE ON UPDATE SET NULL"
/// → ("CASCADE", "SET NULL")
pub(super) fn parse_fk_options(condef: &str) -> FkOptions {
    // PG 는 `... REFERENCES t(cols) [MATCH x] [ON UPDATE a] [ON DELETE b] [DEFERRABLE]
    // [INITIALLY DEFERRED] [NOT VALID]` 순으로 출력한다 (ON UPDATE 가 ON DELETE 보다 먼저).
    // 각 절은 다음 키워드 직전까지만 읽는다 — 예전 구현은 ON UPDATE 뒤를 문자열 끝까지 읽어
    // `ON UPDATE CASCADE ON DELETE SET NULL` 이 되어 DDL 에 ON DELETE 가 두 번 들어갔다.
    const KEYWORDS: [&str; 6] = [
        "MATCH ",
        "ON UPDATE ",
        "ON DELETE ",
        "DEFERRABLE",
        "INITIALLY ",
        "NOT VALID",
    ];
    let tail = fk_options_tail(condef);
    let upper = tail.to_ascii_uppercase();
    let mut starts: Vec<usize> = KEYWORDS.iter().filter_map(|k| upper.find(k)).collect();
    starts.sort_unstable();
    let clause = |keyword: &str| -> Option<String> {
        let start = upper.find(keyword)?;
        let end = starts
            .iter()
            .copied()
            .find(|&s| s > start)
            .unwrap_or(tail.len());
        Some(tail[start + keyword.len()..end].trim().to_string())
    };
    let action = |keyword: &str| clause(keyword).unwrap_or_else(|| "NO ACTION".to_string());
    let deferrable = clause("DEFERRABLE").map(|_| {
        match clause("INITIALLY ").as_deref() {
            Some("DEFERRED") => "DEFERRABLE INITIALLY DEFERRED",
            _ => "DEFERRABLE",
        }
        .to_string()
    });
    FkOptions {
        match_type: clause("MATCH "),
        on_delete: action("ON DELETE "),
        on_update: action("ON UPDATE "),
        deferrable,
    }
}

/// `pg_get_constraintdef` 의 FK 옵션 (기본값인 MATCH SIMPLE / NOT DEFERRABLE 은 None)
#[derive(Debug, PartialEq)]
pub(super) struct FkOptions {
    pub match_type: Option<String>,
    pub on_delete: String,
    pub on_update: String,
    pub deferrable: Option<String>,
}

/// `FOREIGN KEY (..) REFERENCES t(cols)` 뒤의 옵션 부분. 따옴표 식별자 안의 괄호는 무시한다.
fn fk_options_tail(condef: &str) -> &str {
    let Some(refs) = condef.find("REFERENCES ") else {
        return "";
    };
    let mut depth = 0;
    let mut in_quotes = false;
    for (i, c) in condef[refs..].char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => {
                depth -= 1;
                if depth == 0 {
                    return condef[refs + i + 1..].trim_start();
                }
            }
            _ => {}
        }
    }
    ""
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

    #[test]
    fn parse_indexdef_include_method_and_where_after_include() {
        let parsed = parse_pg_indexdef(
            "CREATE UNIQUE INDEX i ON a.t USING btree (id) INCLUDE (name, \"x)y\") \
             NULLS NOT DISTINCT WITH (fillfactor='70') WHERE (id > 0)",
        );
        assert_eq!(parsed.columns, "id");
        assert_eq!(parsed.include.as_deref(), Some("name, \"x)y\""));
        assert_eq!(parsed.predicate.as_deref(), Some("(id > 0)"));
        assert_eq!(parsed.method.as_deref(), Some("BTREE"));

        let gin = parse_pg_indexdef(
            "CREATE INDEX g ON a.t USING gin (to_tsvector('simple'::regconfig, name))",
        );
        assert_eq!(gin.method.as_deref(), Some("GIN"));
        assert_eq!(gin.include, None);
        assert_eq!(gin.predicate, None);
    }

    #[test]
    fn parse_fk_options_reads_each_clause_up_to_next_keyword() {
        // PG 18 실측: ON UPDATE 가 ON DELETE 보다 먼저 온다
        let opts = parse_fk_options(
            "FOREIGN KEY (x, y) REFERENCES p.parent(a, b) ON UPDATE CASCADE ON DELETE SET NULL",
        );
        assert_eq!(opts.on_update, "CASCADE");
        assert_eq!(opts.on_delete, "SET NULL");
        assert_eq!(opts.match_type, None);
        assert_eq!(opts.deferrable, None);

        let opts = parse_fk_options(
            "FOREIGN KEY (o) REFERENCES q.other(id) MATCH FULL ON DELETE SET NULL (o) \
             DEFERRABLE INITIALLY DEFERRED NOT VALID",
        );
        assert_eq!(opts.match_type.as_deref(), Some("FULL"));
        assert_eq!(opts.on_delete, "SET NULL (o)");
        assert_eq!(opts.on_update, "NO ACTION");
        assert_eq!(
            opts.deferrable.as_deref(),
            Some("DEFERRABLE INITIALLY DEFERRED")
        );

        // 따옴표 식별자 안의 괄호·키워드는 옵션으로 보지 않는다
        let opts =
            parse_fk_options(r#"FOREIGN KEY (a) REFERENCES "we(ird ON DELETE x"(id) DEFERRABLE"#);
        assert_eq!(opts.on_delete, "NO ACTION");
        assert_eq!(opts.deferrable.as_deref(), Some("DEFERRABLE"));
    }

    // --- parse_pg_indexdef: 기존 동작(is_unique, columns) 회귀 방지 ---

    #[test]
    fn parse_indexdef_non_unique_basic() {
        let parsed = parse_pg_indexdef("CREATE INDEX idx ON t USING btree (col)");
        assert!(!parsed.is_unique);
        assert_eq!(parsed.columns, "col");
        assert_eq!(parsed.predicate, None);
    }

    #[test]
    fn parse_indexdef_unique_multi_column_keeps_desc() {
        let parsed = parse_pg_indexdef("CREATE UNIQUE INDEX idx ON t USING btree (a, b DESC)");
        assert!(parsed.is_unique);
        assert_eq!(parsed.columns, "a, b DESC");
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
