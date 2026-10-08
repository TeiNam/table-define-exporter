use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

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

    /// 스키마 파일에서 테이블보다 먼저 쓸 문장 (PostgreSQL 사용자 타입·시퀀스). SQL 포맷만 사용
    fn set_schema_preamble(&mut self, _schema: &str, _statements: Vec<String>) {}

    /// 파일 저장/닫기
    fn finish(&mut self) -> Result<(), AppError>;
}

/// 스키마별로 분리되는 출력 파일명: `{schema}({source}).{ext}` — `source` 는 [`source_label`]
pub fn schema_filename(schema: &str, source: &str, ext: &str) -> String {
    let stem = format!(
        "{}({})",
        sanitize_filename_part(schema),
        sanitize_filename_part(source)
    );
    fit_filename(&stem, ext)
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
    fit_filename(&sanitize_filename_part(source), "xlsx")
}

/// 파일 이름 한 개의 최대 길이 (대부분의 파일 시스템이 255바이트)
const MAX_FILENAME_BYTES: usize = 255;

/// `{stem}.{ext}`. Windows 장치 이름(`NUL`, `CON.x` 처럼 첫 `.` 앞이 CON·PRN·AUX·NUL·COM1~9·
/// LPT1~9)이면 앞에 `_` 를 붙이고, 255바이트를 넘으면 stem 을 잘라 원래 stem 의 해시를 붙인다
/// (잘린 이름끼리 겹치지 않고 실행마다 같은 이름).
fn fit_filename(stem: &str, ext: &str) -> String {
    let device = stem
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let is_device = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && device.as_bytes()[3].is_ascii_digit());
    let stem = if is_device {
        format!("_{stem}")
    } else {
        stem.to_string()
    };
    let name = format!("{stem}.{ext}");
    if name.len() <= MAX_FILENAME_BYTES {
        return name;
    }
    let mut hasher = DefaultHasher::new();
    stem.hash(&mut hasher);
    let suffix = format!("~{:016x}.{ext}", hasher.finish());
    let mut cut = MAX_FILENAME_BYTES.saturating_sub(suffix.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{suffix}", &stem[..cut])
}

/// 파일명에 쓰는 접속 대상 표기: `{endpoint}[_{port}][@{database}]`
///
/// 같은 호스트의 다른 인스턴스(포트)나 다른 PostgreSQL database 를 내보내도 서로
/// 덮어쓰지 않게 구분한다. 포트는 DB 기본값(3306/5432)이 아닐 때만, database 는
/// PostgreSQL 일 때만 붙어 기본 포트 사용 시 파일명은 `{endpoint}` 그대로다.
/// database 앞은 `@` 라서 숫자 이름의 database(`h@5433`)와 포트(`h_5433`)가 겹치지 않는다.
pub fn source_label(config: &RunConfig) -> String {
    let mut label = config.endpoint.clone();
    if config.port != config.db_type.default_port() {
        label.push_str(&format!("_{}", config.port));
    }
    if let Some(database) = &config.database {
        label.push_str(&format!("@{database}"));
    }
    label
}

/// 스키마별 출력 파일을 `dir` 에 만든다 (기존 파일은 덮어씀).
///
/// 이름이 달라도 파일 시스템이 같은 파일로 여기는 경우(macOS 의 유니코드 정규화 무시 등)
/// 두 스키마가 한 파일에 섞이지 않도록, 이번 실행에서 이미 연 파일과 같은 파일이면(Unix: 장치·inode)
/// `{schema}~N` 이름으로 다시 연다. 파일은 모두 쓰기 전에 열리므로 다시 열어도 잃는 내용이 없다.
pub(crate) fn create_schema_files<'a>(
    dir: &Path,
    schemas: impl IntoIterator<Item = &'a String>,
    source: &str,
    ext: &str,
) -> Result<HashMap<String, File>, AppError> {
    let open = |name: &str| {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(dir.join(name))
            .map_err(|source| AppError::FileWrite { source })
    };
    let mut files = HashMap::new();
    let mut opened = HashSet::new();
    for (schema, filename) in schema_filenames(schemas, source, ext) {
        let mut name = filename;
        let mut file = open(&name)?;
        let mut n = 2;
        while let Some(id) = file_id(&file)
            && !opened.insert(id)
        {
            name = schema_filename(&format!("{schema}~{n}"), source, ext);
            tracing::warn!("파일명 충돌(같은 파일) — {schema} 는 {name} 로 출력");
            file = open(&name)?;
            n += 1;
        }
        files.insert(schema.clone(), file);
    }
    Ok(files)
}

/// 같은 파일인지 비교할 (장치, inode). Unix 가 아니면 `None` — Windows(NTFS)는 대소문자만
/// 무시하고 이는 [`schema_filenames`] 가 이름으로 이미 구분한다.
fn file_id(file: &File) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        file.metadata().ok().map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 이름이 달라도 같은 파일(여기선 하드 링크 — macOS 의 유니코드 정규화 충돌과 같은 상황)이면
    /// 한 파일에 두 스키마가 섞이지 않게 `~N` 이름으로 연다.
    #[cfg(unix)]
    #[test]
    fn schema_files_that_are_the_same_file_get_suffix() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        File::create(dir.path().join("a(h).md")).unwrap();
        std::fs::hard_link(dir.path().join("a(h).md"), dir.path().join("b(h).md")).unwrap();
        let schemas = vec!["a".to_string(), "b".to_string()];
        let mut files = create_schema_files(dir.path(), &schemas, "h", "md").unwrap();
        files.get_mut("a").unwrap().write_all(b"A").unwrap();
        files.get_mut("b").unwrap().write_all(b"B").unwrap();
        drop(files);
        let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap();
        assert_eq!(read("a(h).md"), "A");
        assert_eq!(read("b~2(h).md"), "B");
    }
}
