use std::borrow::Cow;
use std::cmp::max;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::{
    error::AppError,
    model::{RunConfig, SchemaCatalog, TableDef},
};

use super::Exporter;

/// Markdown 출력 담당 Exporter
pub struct MarkdownExporter {
    /// 스키마명 → 파일 핸들 맵
    files: HashMap<String, File>,
}

impl MarkdownExporter {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }
}

impl Default for MarkdownExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exporter for MarkdownExporter {
    fn setup(&mut self, catalog: &SchemaCatalog, config: &RunConfig) -> Result<(), AppError> {
        // 스키마별 .md 파일 생성 (기존 파일 덮어쓰기)
        let source = super::source_label(config);
        self.files = super::create_schema_files(Path::new(""), catalog.keys(), &source, "md")?;
        Ok(())
    }

    fn write_tables(&mut self, schema: &str, tables: &[TableDef]) -> Result<(), AppError> {
        let file = match self.files.get_mut(schema) {
            Some(f) => f,
            None => return Ok(()),
        };

        write_markdown(file, schema, tables).map_err(|source| AppError::FileWrite { source })
    }

    fn finish(&mut self) -> Result<(), AppError> {
        // 파일 핸들 드롭 (flush는 Drop에서 자동 처리)
        self.files.clear();
        Ok(())
    }
}

/// Markdown 내용을 파일에 기록하는 내부 함수
fn write_markdown(file: &mut File, schema: &str, tables: &[TableDef]) -> std::io::Result<()> {
    // 스키마 제목
    writeln!(file, "{} ", schema)?;
    writeln!(file, "=============")?;
    writeln!(file)?;

    // Table List 섹션
    writeln!(file, "## Table List")?;
    for t in tables {
        let comment = t.general.comment.as_deref().unwrap_or("");
        writeln!(
            file,
            "- [{} ({})](#{})",
            t.table_name,
            cell(comment),
            t.table_name.to_lowercase()
        )?;
        write!(file, " ")?;
    }
    writeln!(file)?;

    // 테이블별 섹션
    for t in tables {
        writeln!(file, "## {}", t.table_name.to_lowercase())?;
        writeln!(file, "**Information**")?;

        if t.general.is_table() {
            // 일반 정보 표
            writeln!(file, "|Table type|Engine|Row format|Collate|Comment|")?;
            writeln!(file, "|---|---|---|---|---|")?;
            writeln!(
                file,
                "|{}|{}|{}|{}|{}|",
                cell(&t.general.table_type),
                cell(t.general.engine.as_deref().unwrap_or("")),
                cell(t.general.row_format.as_deref().unwrap_or("")),
                cell(t.general.collate.as_deref().unwrap_or("")),
                cell(t.general.comment.as_deref().unwrap_or("")),
            )?;
            writeln!(file)?;

            // 컬럼 표
            writeln!(file, "**Columns**")?;
            writeln!(
                file,
                "|Name|Type|Nullable|Default|Charset|Collation|Key|Extra|Comment|"
            )?;
            writeln!(file, "|---|---|---|---|---|---|---|---|---|")?;
            for c in &t.columns {
                writeln!(
                    file,
                    "|{}|{}|{}|{}|{}|{}|{}|{}|{}|",
                    cell(&c.column_name),
                    cell(&c.column_type),
                    cell(&c.nullable),
                    cell(c.display_default()),
                    cell(c.charset.as_deref().unwrap_or("")),
                    cell(c.collation.as_deref().unwrap_or("")),
                    cell(c.column_key.as_deref().unwrap_or("")),
                    cell(c.extra.as_deref().unwrap_or("")),
                    cell(c.comment.as_deref().unwrap_or("")),
                )?;
            }
            writeln!(file)?;

            // 인덱스 섹션
            if !t.indexes.is_empty() {
                writeln!(file, "**Index**")?;
                for idx in &t.indexes {
                    write!(
                        file,
                        "- [{}]{}({})",
                        idx.kind_label(),
                        cell(&idx.index_name),
                        cell(&idx.index_columns)
                    )?;
                    // 커버링 인덱스(PostgreSQL): INCLUDE 컬럼
                    if let Some(include) = &idx.include_columns {
                        write!(file, " INCLUDE ({})", cell(include))?;
                    }
                    // 파셜 인덱스(partial index): predicate가 존재하면 " WHERE <predicate>" 추가
                    if let Some(pred) = &idx.predicate {
                        write!(file, " WHERE {}", cell(pred))?;
                    }
                    writeln!(file)?;
                }
                writeln!(file)?;
            }

            // 제약 조건 섹션 (Reference 라벨 사용 — 의도적 오타 수정 반영)
            if !t.constraints.is_empty() {
                writeln!(file, "**Constraint**")?;
                for con in &t.constraints {
                    writeln!(
                        file,
                        "- {} FOREIGN KEY ({}) Reference {} ON DELETE {} ON UPDATE {}",
                        cell(&con.constraint_name),
                        cell(&con.constraint_column),
                        cell(&con.reference),
                        con.delete_action,
                        con.update_action,
                    )?;
                }
                writeln!(file)?;
            }
        } else if t.general.is_view() {
            // 뷰 정보 표
            writeln!(file, "|Table type|Charset|Collate|")?;
            writeln!(file, "|---|---|---|")?;
            if let Some(view) = &t.view {
                writeln!(
                    file,
                    "|{}|{}|{}|",
                    cell(&t.general.table_type),
                    cell(&view.charset),
                    cell(&view.collate)
                )?;
            } else {
                writeln!(file, "|{}||  |", t.general.table_type)?;
            }
            writeln!(file)?;

            // View Create SQL 섹션
            writeln!(file, "**View Create SQL**")?;
            if let Some(view) = &t.view {
                write_view_fenced_sql(file, &view.view_query)?;
            }
        }

        writeln!(file, " ")?;
    }

    Ok(())
}

/// Markdown 표 셀(과 목록 한 줄)을 깨뜨리거나 서식으로 해석되는 문자를 이스케이프한다.
///
/// 코멘트의 줄바꿈·`|`, PostgreSQL 기본값의 `||` 연산자는 행을 가르고, MySQL 기본값의
/// `\` 이스케이프(`'a\nb'`)·`<태그>`·`` ` ``·`*` 는 서식으로 해석돼 다른 값으로 보인다.
/// `\` `|` `` ` `` `*` `<` 앞에는 백슬래시를 붙이고, 줄바꿈 → `<br>`.
fn cell(s: &str) -> Cow<'_, str> {
    if !s.contains(['\\', '|', '`', '*', '<', '\n', '\r']) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' | '|' | '`' | '*' | '<' => {
                out.push('\\');
                out.push(c);
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("<br>");
            }
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// VIEW의 SQL 본문을 언어 태그가 붙은 fenced code block으로 기록한다.
///
/// Requirements 3.1/3.2/3.3 준수:
/// - 빈 줄 → 열기 펜스 라인(```sql) → SQL 본문 → 닫기 펜스 라인을 각각 별도 줄로 출력
/// - 한 줄 안에 언어 태그와 본문을 함께 배치하지 않는다
/// - 본문에 포함된 최장 연속 백틱 길이가 `m`일 때 펜스 길이는 `max(3, m + 1)`
fn write_view_fenced_sql(file: &mut File, sql: &str) -> std::io::Result<()> {
    let fence_len = max(3, longest_backtick_run(sql) + 1);
    let fence: String = "`".repeat(fence_len);

    // 이전 섹션과 분리되는 빈 줄
    writeln!(file)?;
    // 열기 펜스 + 언어 태그
    writeln!(file, "{fence}sql")?;
    // SQL 본문 — 말미 개행 보장
    if sql.ends_with('\n') {
        file.write_all(sql.as_bytes())?;
    } else {
        file.write_all(sql.as_bytes())?;
        writeln!(file)?;
    }
    // 닫기 펜스
    writeln!(file, "{fence}")?;
    Ok(())
}

/// 문자열 내 최장 연속 백틱(`) 길이를 반환한다.
fn longest_backtick_run(s: &str) -> usize {
    let mut max_run = 0usize;
    let mut cur = 0usize;
    for ch in s.chars() {
        if ch == '`' {
            cur += 1;
            if cur > max_run {
                max_run = cur;
            }
        } else {
            cur = 0;
        }
    }
    max_run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_escapes_pipes_and_newlines() {
        assert!(matches!(cell("plain text"), Cow::Borrowed("plain text")));
        assert_eq!(cell("a | b"), "a \\| b");
        assert_eq!(
            cell("('a'::text || 'b'::text)"),
            "('a'::text \\|\\| 'b'::text)"
        );
        assert_eq!(
            cell("첫 줄\r\n둘째\n셋째\r끝"),
            "첫 줄<br>둘째<br>셋째<br>끝"
        );
        // MySQL 기본값의 백슬래시 이스케이프·HTML 태그·코드/강조 표시는 글자 그대로 보이게
        assert_eq!(cell(r"'a\nb'"), r"'a\\nb'");
        assert_eq!(cell("<b>x</b>"), r"\<b>x\</b>");
        assert_eq!(cell("`a` * 2"), r"\`a\` \* 2");
    }

    #[test]
    fn longest_backtick_run_empty() {
        assert_eq!(longest_backtick_run(""), 0);
    }

    #[test]
    fn longest_backtick_run_no_backticks() {
        assert_eq!(longest_backtick_run("SELECT 1 FROM t"), 0);
    }

    #[test]
    fn longest_backtick_run_single() {
        assert_eq!(longest_backtick_run("`a`"), 1);
    }

    #[test]
    fn longest_backtick_run_picks_max() {
        // 1개, 그 다음 3개, 그 다음 2개 → 3
        assert_eq!(longest_backtick_run("`a```b``c"), 3);
    }

    #[test]
    fn longest_backtick_run_resets_on_non_backtick() {
        // 2개 + x + 4개 → 4
        assert_eq!(longest_backtick_run("``x````"), 4);
    }
}
