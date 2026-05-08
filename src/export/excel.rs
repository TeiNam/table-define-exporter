use rust_xlsxwriter::{Format, FormatBorder, Workbook, Worksheet, XlsxError};

use crate::{
    error::AppError,
    model::{RunConfig, SchemaCatalog, TableDef},
};

use super::Exporter;

/// Excel 출력 담당 Exporter
pub struct ExcelExporter {
    workbook: Workbook,
    /// 저장 파일명 (setup 시 결정)
    filename: String,
    /// 스키마 목록 (시트 순서 유지)
    schemas: Vec<String>,
}

impl ExcelExporter {
    pub fn new() -> Self {
        Self {
            workbook: Workbook::new(),
            filename: String::new(),
            schemas: Vec::new(),
        }
    }

    /// title 스타일: 검정 배경, 흰색 볼드, 전면 테두리
    fn title_format() -> Format {
        Format::new()
            .set_background_color(0x000000)
            .set_font_color(0xFFFFFF)
            .set_bold()
            .set_border(FormatBorder::Thin)
    }

    /// start 스타일: 하단 테두리만
    fn start_format() -> Format {
        Format::new().set_border_bottom(FormatBorder::Thin)
    }

    /// end 스타일: 상단 테두리만
    fn end_format() -> Format {
        Format::new().set_border_top(FormatBorder::Thin)
    }
}

impl Default for ExcelExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exporter for ExcelExporter {
    fn setup(&mut self, catalog: &SchemaCatalog, config: &RunConfig) -> Result<(), AppError> {
        self.filename = format!("{}.xlsx", config.endpoint);
        self.schemas = catalog.keys().cloned().collect();
        self.schemas.sort(); // 일관된 순서 보장

        // 스키마별 시트 추가
        for schema in &self.schemas {
            self.workbook
                .add_worksheet()
                .set_name(schema)
                .map_err(|e| AppError::ExcelWrite(e.to_string()))?;
        }

        Ok(())
    }

    fn write_tables(&mut self, schema: &str, tables: &[TableDef]) -> Result<(), AppError> {
        // 해당 스키마의 워크시트 찾기
        let sheet_index = self
            .schemas
            .iter()
            .position(|s| s == schema)
            .ok_or_else(|| AppError::ExcelWrite(format!("시트를 찾을 수 없음: {}", schema)))?;

        let worksheet = self
            .workbook
            .worksheet_from_index(sheet_index)
            .map_err(|e| AppError::ExcelWrite(e.to_string()))?;

        write_tables_to_sheet(worksheet, tables).map_err(|e| AppError::ExcelWrite(e.to_string()))
    }

    fn finish(&mut self) -> Result<(), AppError> {
        self.workbook
            .save(&self.filename)
            .map_err(|e| AppError::ExcelWrite(e.to_string()))
    }
}

/// 워크시트에 테이블 데이터를 기록하는 내부 함수
fn write_tables_to_sheet(ws: &mut Worksheet, tables: &[TableDef]) -> Result<(), XlsxError> {
    let title_fmt = ExcelExporter::title_format();
    let start_fmt = ExcelExporter::start_format();
    let end_fmt = ExcelExporter::end_format();

    let mut row: u32 = 0;

    for t in tables {
        // start row (하단 테두리)
        for col in 0u16..10 {
            ws.write_with_format(row, col, "", &start_fmt)?;
        }
        row += 1;

        // Table name 행: A:B 병합 = "Table name", C:J 병합 = 테이블명
        ws.merge_range(row, 0, row, 1, "Table name", &title_fmt)?;
        ws.merge_range(row, 2, row, 9, t.table_name.as_str(), &Format::new())?;
        row += 1;

        // Description 행: A:B 병합 = "Description", C:J 병합 = 주석
        let comment = t.general.comment.as_deref().unwrap_or("");
        ws.merge_range(row, 0, row, 1, "Description", &title_fmt)?;
        ws.merge_range(row, 2, row, 9, comment, &Format::new())?;
        row += 1;

        // Column Information 타이틀
        ws.merge_range(row, 0, row, 9, "Column Information", &title_fmt)?;
        row += 1;

        if t.general.table_type == "BASE TABLE" {
            // 컬럼 헤더
            ws.write_with_format(row, 0, "No", &title_fmt)?;
            ws.write_with_format(row, 1, "Column", &title_fmt)?;
            ws.write_with_format(row, 2, "Data Type", &title_fmt)?;
            ws.write_with_format(row, 3, "Nullable", &title_fmt)?;
            ws.write_with_format(row, 4, "Key", &title_fmt)?;
            ws.write_with_format(row, 5, "Extra", &title_fmt)?;
            ws.write_with_format(row, 6, "Collate", &title_fmt)?;
            ws.write_with_format(row, 7, "Default", &title_fmt)?;
            ws.merge_range(row, 8, row, 9, "Comment", &title_fmt)?;
            row += 1;

            // 컬럼 데이터 행
            for (i, c) in t.columns.iter().enumerate() {
                ws.write(row, 0, i as u32)?;
                ws.write(row, 1, c.column_name.as_str())?;
                ws.write(row, 2, c.column_type.as_str())?;
                ws.write(row, 3, c.nullable.as_str())?;
                ws.write(row, 4, c.column_key.as_deref().unwrap_or(""))?;
                ws.write(row, 5, c.extra.as_deref().unwrap_or(""))?;
                ws.write(row, 6, c.collation.as_deref().unwrap_or(""))?;
                ws.write(row, 7, c.default_value.as_deref().unwrap_or(""))?;
                ws.merge_range(
                    row,
                    8,
                    row,
                    9,
                    c.comment.as_deref().unwrap_or(""),
                    &Format::new(),
                )?;
                row += 1;
            }

            // 인덱스 섹션
            if !t.indexes.is_empty() {
                ws.merge_range(row, 0, row, 9, "Indexes", &title_fmt)?;
                row += 1;

                // 인덱스 헤더
                ws.merge_range(row, 0, row, 1, "Index Type", &title_fmt)?;
                ws.merge_range(row, 2, row, 5, "Index Name", &title_fmt)?;
                ws.merge_range(row, 6, row, 9, "Columns", &title_fmt)?;
                row += 1;

                for idx in &t.indexes {
                    let idx_type = if idx.non_unique == 1 {
                        "Normal Index"
                    } else {
                        "Unique Index"
                    };
                    ws.merge_range(row, 0, row, 1, idx_type, &Format::new())?;
                    ws.merge_range(row, 2, row, 5, idx.index_name.as_str(), &Format::new())?;
                    // 파셜 인덱스(partial index): predicate가 존재하면 컬럼 뒤에 " WHERE <predicate>" 추가
                    let columns_cell = if let Some(pred) = &idx.predicate {
                        format!("{} WHERE {}", idx.index_columns, pred)
                    } else {
                        idx.index_columns.clone()
                    };
                    ws.merge_range(row, 6, row, 9, columns_cell.as_str(), &Format::new())?;
                    row += 1;
                }
            }

            // 제약 조건 섹션 (오타 Referance 유지 - Go 버전 호환)
            if !t.constraints.is_empty() {
                ws.merge_range(row, 0, row, 9, "Constraint", &title_fmt)?;
                row += 1;

                // 제약 헤더
                ws.merge_range(row, 0, row, 2, "Constraint Name", &title_fmt)?;
                ws.write_with_format(row, 3, "Column", &title_fmt)?;
                ws.merge_range(row, 4, row, 7, "Referance", &title_fmt)?;
                ws.write_with_format(row, 8, "ON DELETE", &title_fmt)?;
                ws.write_with_format(row, 9, "ON UPDATE", &title_fmt)?;
                row += 1;

                for con in &t.constraints {
                    ws.merge_range(row, 0, row, 2, con.constraint_name.as_str(), &Format::new())?;
                    ws.write(row, 3, con.constraint_column.as_str())?;
                    ws.merge_range(row, 4, row, 7, con.reference.as_str(), &Format::new())?;
                    ws.write(row, 8, con.delete_action.as_str())?;
                    ws.write(row, 9, con.update_action.as_str())?;
                    row += 1;
                }
            }
        } else if t.general.table_type == "VIEW" {
            // View Create SQL 섹션
            ws.merge_range(row, 0, row, 9, "View Create SQL", &title_fmt)?;
            row += 1;

            let view_query = t.view.as_ref().map(|v| v.view_query.as_str()).unwrap_or("");
            ws.merge_range(row, 0, row, 9, view_query, &Format::new())?;
            row += 1;
        }

        // Table Information 섹션
        ws.merge_range(row, 0, row, 9, "Table Information", &title_fmt)?;
        row += 1;

        // Engine / Row Format 행
        ws.merge_range(row, 0, row, 1, "Engine", &title_fmt)?;
        ws.merge_range(
            row,
            2,
            row,
            3,
            t.general.engine.as_deref().unwrap_or(""),
            &Format::new(),
        )?;
        ws.merge_range(row, 4, row, 5, "Row Format", &title_fmt)?;
        ws.merge_range(
            row,
            6,
            row,
            9,
            t.general.row_format.as_deref().unwrap_or(""),
            &Format::new(),
        )?;
        row += 1;

        // Table Type / Collation 행
        ws.merge_range(row, 0, row, 1, "Table Type", &title_fmt)?;
        ws.merge_range(
            row,
            2,
            row,
            3,
            t.general.table_type.as_str(),
            &Format::new(),
        )?;
        ws.merge_range(row, 4, row, 5, "Collation", &title_fmt)?;

        // VIEW의 Collation은 ViewInfo.collate 사용, BASE TABLE은 general.collate 사용
        let collation = if t.general.table_type == "VIEW" {
            t.view.as_ref().map(|v| v.collate.as_str()).unwrap_or("")
        } else {
            t.general.collate.as_deref().unwrap_or("")
        };
        ws.merge_range(row, 6, row, 9, collation, &Format::new())?;
        row += 1;

        // end row (상단 테두리)
        for col in 0u16..10 {
            ws.write_with_format(row, col, "", &end_fmt)?;
        }
        row += 1;

        // 빈 행
        row += 1;
    }

    Ok(())
}
