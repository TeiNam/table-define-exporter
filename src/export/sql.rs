use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;

use crate::{
    error::AppError,
    model::{RunConfig, SchemaCatalog, TableDef},
};

use super::Exporter;

/// SQL 출력 담당 Exporter
pub struct SqlExporter {
    /// 스키마명 → 파일 핸들 맵
    files: HashMap<String, File>,
    /// 엔드포인트 (파일명에 사용)
    endpoint: String,
}

impl SqlExporter {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            endpoint: String::new(),
        }
    }
}

impl Default for SqlExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exporter for SqlExporter {
    fn setup(&mut self, catalog: &SchemaCatalog, config: &RunConfig) -> Result<(), AppError> {
        self.endpoint = config.endpoint.clone();

        // 스키마별 .sql 파일 생성 (기존 파일 덮어쓰기)
        for schema in catalog.keys() {
            let filename = format!("{}({}).sql", schema, config.endpoint);
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

        write_sql(file, schema, tables).map_err(|source| AppError::FileWrite { source })
    }

    fn finish(&mut self) -> Result<(), AppError> {
        self.files.clear();
        Ok(())
    }
}

/// SQL 내용을 파일에 기록하는 내부 함수
fn write_sql(file: &mut File, schema: &str, tables: &[TableDef]) -> std::io::Result<()> {
    // 데이터베이스 헤더 주석
    writeln!(file, "/* Database : {} */", schema)?;

    for t in tables {
        // 테이블 주석
        writeln!(file, "/* Table : {} */", t.table_name)?;
        // DROP TABLE IF EXISTS
        writeln!(file, "DROP TABLE IF EXISTS {};", t.table_name)?;
        // DDL 원본 그대로 보존 (트리밍/재작성 금지)
        let ddl = t.ddl.as_deref().unwrap_or("");
        writeln!(file, "{};\n\n", ddl)?;
    }

    Ok(())
}
