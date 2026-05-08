use std::cmp::max;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;

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
    fn setup(&mut self, catalog: &SchemaCatalog, _config: &RunConfig) -> Result<(), AppError> {
        // 스키마별 .md 파일 생성 (기존 파일 덮어쓰기)
        for schema in catalog.keys() {
            let filename = format!("{}.md", schema);
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&filename)
                .map_err(|source| AppError::FileWrite { source })?;
            self.files.insert(schema.clone(), file);
        }
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
            comment,
            t.table_name.to_lowercase()
        )?;
        write!(file, " ")?;
    }
    writeln!(file)?;

    // 테이블별 섹션
    for t in tables {
        writeln!(file, "## {}", t.table_name.to_lowercase())?;
        writeln!(file, "**Information**")?;

        if t.general.table_type == "BASE TABLE" {
            // 일반 정보 표
            writeln!(file, "|Table type|Engine|Row format|Collate|Comment|")?;
            writeln!(file, "|---|---|---|---|---|")?;
            writeln!(
                file,
                "|{}|{}|{}|{}|{}|",
                t.general.table_type,
                t.general.engine.as_deref().unwrap_or(""),
                t.general.row_format.as_deref().unwrap_or(""),
                t.general.collate.as_deref().unwrap_or(""),
                t.general.comment.as_deref().unwrap_or(""),
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
                    c.column_name,
                    c.column_type,
                    c.nullable,
                    c.default_value.as_deref().unwrap_or(""),
                    c.charset.as_deref().unwrap_or(""),
                    c.collation.as_deref().unwrap_or(""),
                    c.column_key.as_deref().unwrap_or(""),
                    c.extra.as_deref().unwrap_or(""),
                    c.comment.as_deref().unwrap_or(""),
                )?;
            }
            writeln!(file)?;

            // 인덱스 섹션
            if !t.indexes.is_empty() {
                writeln!(file, "**Index**")?;
                for idx in &t.indexes {
                    let idx_type = if idx.non_unique == 1 {
                        "Normal"
                    } else {
                        "Unique"
                    };
                    write!(
                        file,
                        "- [{}]{}({})",
                        idx_type, idx.index_name, idx.index_columns
                    )?;
                    // 파셜 인덱스(partial index): predicate가 존재하면 " WHERE <predicate>" 추가
                    if let Some(pred) = &idx.predicate {
                        write!(file, " WHERE {}", pred)?;
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
                        con.constraint_name,
                        con.constraint_column,
                        con.reference,
                        con.delete_action,
                        con.update_action,
                    )?;
                }
                writeln!(file)?;
            }
        } else if t.general.table_type == "VIEW" {
            // 뷰 정보 표
            writeln!(file, "|Table type|Charset|Collate|")?;
            writeln!(file, "|---|---|---|")?;
            if let Some(view) = &t.view {
                writeln!(
                    file,
                    "|{}|{}|{}|",
                    t.general.table_type, view.charset, view.collate
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
