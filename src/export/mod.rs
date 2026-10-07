use std::collections::HashSet;

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

/// 스키마별로 분리되는 출력 파일명: `{schema}({source}).{ext}` — `source` 는 [`source_label`]
pub fn schema_filename(schema: &str, source: &str, ext: &str) -> String {
    format!(
        "{}({}).{ext}",
        sanitize_filename_part(schema),
        sanitize_filename_part(source)
    )
}

/// 스키마별 출력 파일명 목록 (스키마명 정렬 순).
///
/// 대소문자를 구분하지 않는 파일시스템(macOS·Windows 기본)에서는 `Sales`/`sales`, 정리 후
/// 같아지는 `x/y`/`x_y` 가 같은 파일이 되어 한쪽 출력이 사라진다. 대소문자 무시로 겹치는
/// 이름은 `{schema}~2`, `~3` 으로 구분한다 (정렬 후 부여해 실행마다 같은 결과).
pub fn schema_filenames<'a>(
    schemas: impl IntoIterator<Item = &'a String>,
    source: &str,
    ext: &str,
) -> Vec<(&'a String, String)> {
    let mut sorted: Vec<&String> = schemas.into_iter().collect();
    sorted.sort();
    let mut used = HashSet::new();
    sorted
        .into_iter()
        .map(|schema| {
            let mut name = schema_filename(schema, source, ext);
            let mut n = 2;
            while !used.insert(name.to_lowercase()) {
                name = schema_filename(&format!("{schema}~{n}"), source, ext);
                n += 1;
            }
            if n > 2 {
                tracing::warn!("파일명 충돌(대소문자 무시) — {schema} 는 {name} 로 출력");
            }
            (schema, name)
        })
        .collect()
}

/// 워크북(Excel) 출력 파일명: `{source}.xlsx` — `source` 는 [`source_label`]
pub fn workbook_filename(source: &str) -> String {
    format!("{}.xlsx", sanitize_filename_part(source))
}

/// 파일명에 쓰는 접속 대상 표기: `{endpoint}[_{port}][_{database}]`
///
/// 같은 호스트의 다른 인스턴스(포트)나 다른 PostgreSQL database 를 내보내도 서로
/// 덮어쓰지 않게 구분한다. 포트는 DB 기본값(3306/5432)이 아닐 때만, database 는
/// PostgreSQL 일 때만 붙어 기본 포트 사용 시 파일명은 `{endpoint}` 그대로다.
pub fn source_label(config: &RunConfig) -> String {
    let port = (config.port != config.db_type.default_port()).then(|| config.port.to_string());
    [Some(config.endpoint.clone()), port, config.database.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("_")
}

/// 파일명에 쓸 수 없는 문자(경로 구분자, Windows 예약 문자, 제어 문자)를 `_`로 바꾼다.
///
/// 스키마명은 DB 에서 오는 외부 데이터라 `/`·`\` 를 남기면 cwd 밖에 파일을 쓸 수 있고,
/// endpoint 는 IPv6(`::1`)·소켓 경로(`/tmp`)처럼 그대로는 파일명이 될 수 없는 값이 온다.
fn sanitize_filename_part(part: &str) -> String {
    part.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
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

// Property 5(Terminator 단일 세미콜론 종결) 테스트를 위한 공개 진입점 재노출.
// `Terminator` enum 자체는 캡슐화 유지.
pub use sql::apply_sql_terminator;
