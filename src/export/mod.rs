use crate::{
    error::AppError,
    model::{OutputFormat, RunConfig, SchemaCatalog, TableDef},
};

/// 출력 포맷별 구현을 위한 트레이트
pub trait Exporter {
    /// 초기 파일/워크북 설정
    fn setup(&mut self, catalog: &SchemaCatalog, config: &RunConfig) -> Result<(), AppError>;

    /// 한 스키마의 테이블 목록을 출력에 기록
    fn write_tables(&mut self, schema: &str, tables: &[TableDef]) -> Result<(), AppError>;

    /// 파일 저장/닫기
    fn finish(&mut self) -> Result<(), AppError>;
}

/// 출력 포맷에 맞는 Exporter 인스턴스를 생성하는 팩토리 함수
pub fn create_exporter(format: OutputFormat) -> Box<dyn Exporter> {
    match format {
        OutputFormat::Excel => Box::new(excel::ExcelExporter::new()),
        OutputFormat::Markdown => Box::new(markdown::MarkdownExporter::new()),
        OutputFormat::Sql => Box::new(sql::SqlExporter::new()),
    }
}

pub mod excel;
pub mod markdown;
pub mod sql;
