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
                    if idx.non_unique == 1 {
                        writeln!(file, "- [Normal]{}({})", idx.index_name, idx.index_columns)?;
                    } else {
                        writeln!(file, "- [Unique]{}({})", idx.index_name, idx.index_columns)?;
                    }
                }
                writeln!(file)?;
            }

            // 제약 조건 섹션 (오타 Referance 유지 - Go Excel 버전 호환)
            if !t.constraints.is_empty() {
                writeln!(file, "**Constraint**")?;
                for con in &t.constraints {
                    writeln!(
                        file,
                        "- {} FOREIGN KEY ({}) Referance {} ON DELETE {} ON UPDATE {}",
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
                writeln!(file, "\n```{}```", view.view_query)?;
            }
        }

        writeln!(file, " ")?;
    }

    Ok(())
}
