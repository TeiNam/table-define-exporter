use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;

use crate::{
    error::AppError,
    identifier::{quote_identifier, quote_pg_identifier},
    model::{DbType, RunConfig, SchemaCatalog, TableDef},
};

use super::Exporter;

/// DB 종류별 DDL 종결 규칙 (Req 2.2, 2.3, 2.4, 14.1)
///
/// 입력 DDL이 이미 `;` 또는 `);`로 끝나면 추가하지 않고,
/// 그렇지 않으면 정확히 하나의 `;`를 덧붙인다. 현재는 MySQL/PostgreSQL의
/// 종결 규칙이 동일하지만, DB별 분기 지점을 타입으로 보존해 향후 규칙
/// 분화가 생기면 변경 범위가 이 enum 내부로 제한되도록 한다.
pub(super) enum Terminator {
    Mysql,
    Postgres,
}

impl Terminator {
    /// `config.db_type`로부터 적절한 Terminator를 선택한다 (Req 14.4).
    fn from_db_type(db_type: DbType) -> Self {
        match db_type {
            DbType::MySql => Self::Mysql,
            DbType::Postgres => Self::Postgres,
        }
    }

    /// DDL을 정확히 하나의 세미콜론으로 종결한다.
    /// - Req 2.2: 입력이 이미 `;`/`);`로 끝나면 추가 세미콜론을 붙이지 않는다.
    /// - Req 2.3: 세미콜론 없이 끝나면 하나만 추가한다.
    ///
    /// `match self`로 변형(variant)을 명시적으로 참조해 Req 2.4가 요구하는
    /// DB 종류별 분기 지점이 타입 레벨에서 존재함을 보장한다.
    fn apply(&self, ddl: &str) -> String {
        match self {
            Self::Mysql | Self::Postgres => {
                let trimmed = ddl.trim_end();
                if trimmed.ends_with(';') || trimmed.ends_with(");") {
                    trimmed.to_string()
                } else {
                    format!("{trimmed};")
                }
            }
        }
    }
}

/// (공개) DB 종류에 맞는 Terminator를 선택하여 DDL에 적용한다.
///
/// 외부 통합 테스트에서 Property 5(Terminator 단일 세미콜론 종결)를
/// 직접 검증하기 위한 공개 진입점. 내부적으로 `Terminator::from_db_type`과
/// `Terminator::apply`를 호출하며, `Terminator` enum 자체는 `pub(super)`로
/// 캡슐화된 상태를 유지한다.
pub fn apply_sql_terminator(ddl: &str, db_type: DbType) -> String {
    Terminator::from_db_type(db_type).apply(ddl)
}

/// SQL 출력 담당 Exporter
pub struct SqlExporter {
    /// 스키마명 → 파일 핸들 맵
    files: HashMap<String, File>,
    /// 엔드포인트 (파일명에 사용)
    endpoint: String,
    /// DB 종류 (식별자 인용 규칙 + Terminator 선택에 사용) — Req 14.4
    db_type: DbType,
}

impl SqlExporter {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            endpoint: String::new(),
            // 초기값은 MySQL. 실제 값은 `setup`에서 `config.db_type`으로 덮어쓴다.
            db_type: DbType::MySql,
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
        // Req 14.4: config.db_type을 필드에 보관해 이후 write_tables에서 재사용한다.
        self.db_type = config.db_type;

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

        write_sql(file, schema, tables, self.db_type)
            .map_err(|source| AppError::FileWrite { source })
    }

    fn finish(&mut self) -> Result<(), AppError> {
        self.files.clear();
        Ok(())
    }
}

/// 테이블명을 DB 종류별 규칙으로 인용한다 (Req 2.1).
/// - MySQL: 백틱(`` ` ``) 인용
/// - PostgreSQL: 큰따옴표(`"`) 인용
///
/// 인용 함수 내부에서 `validate_identifier`가 호출되므로 위험 문자(`;`, `/*`,
/// `*/`, 개행 등)가 포함된 식별자는 `AppError::UnsafeIdentifier`로 거부된다.
fn quote_table_name(db_type: DbType, table_name: &str) -> Result<String, AppError> {
    match db_type {
        DbType::MySql => quote_identifier(table_name),
        DbType::Postgres => quote_pg_identifier(table_name),
    }
}

/// SQL 내용을 파일에 기록하는 내부 함수
fn write_sql(
    file: &mut File,
    schema: &str,
    tables: &[TableDef],
    db_type: DbType,
) -> std::io::Result<()> {
    let terminator = Terminator::from_db_type(db_type);

    // 데이터베이스 헤더 주석
    writeln!(file, "/* Database : {} */", schema)?;

    for t in tables {
        // Req 2.5, 14.3: 위험 식별자를 포함한 테이블은 출력에서 스킵한다 (DROP은 더 이상
        // 출력하지 않지만, 주석/DDL에 위험 식별자가 새는 것을 막기 위해 검증은 유지).
        if let Err(e) = quote_table_name(db_type, &t.table_name) {
            tracing::warn!(
                schema,
                table = %t.table_name,
                error = %e,
                "위험한 식별자를 포함한 테이블을 SQL 출력에서 스킵"
            );
            continue;
        }

        // 테이블 주석
        writeln!(file, "/* Table : {} */", t.table_name)?;
        // CREATE DDL만 출력 — DROP 구문은 제외. 원본을 보존하되 Terminator로 정확히 하나의 `;` 종결
        let ddl = t.ddl.as_deref().unwrap_or("");
        writeln!(file, "{}\n\n", terminator.apply(ddl))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminator_adds_semicolon_when_absent() {
        // Req 2.3: 세미콜론이 없으면 하나를 추가한다.
        let t = Terminator::Mysql;
        assert_eq!(
            t.apply("CREATE TABLE x (id INT)"),
            "CREATE TABLE x (id INT);"
        );
    }

    #[test]
    fn terminator_preserves_single_trailing_semicolon() {
        // Req 2.2: 이미 `;`로 끝나면 추가하지 않는다 (MySQL/PG 공통).
        let t = Terminator::Postgres;
        assert_eq!(
            t.apply("CREATE TABLE x (id INT);"),
            "CREATE TABLE x (id INT);"
        );
        let t = Terminator::Mysql;
        assert_eq!(
            t.apply("CREATE TABLE x (id INT);"),
            "CREATE TABLE x (id INT);"
        );
    }

    #[test]
    fn terminator_preserves_paren_semicolon_ending() {
        // Req 2.2: `);`로 끝나는 DDL(예: 여러 줄 CREATE TABLE)에 세미콜론을 이중으로 붙이지 않는다.
        let t = Terminator::Postgres;
        assert_eq!(
            t.apply("CREATE TABLE x (\n  id INT\n);"),
            "CREATE TABLE x (\n  id INT\n);"
        );
    }

    #[test]
    fn terminator_trims_trailing_whitespace_before_termination() {
        // 끝 공백/개행은 trim된 뒤 세미콜론이 판단되어야 한다.
        let t = Terminator::Mysql;
        assert_eq!(
            t.apply("CREATE TABLE x (id INT)\n\n"),
            "CREATE TABLE x (id INT);"
        );
        assert_eq!(
            t.apply("CREATE TABLE x (id INT);\n"),
            "CREATE TABLE x (id INT);"
        );
    }

    #[test]
    fn quote_table_name_mysql_uses_backticks() {
        // Req 2.1, 14.2: MySQL은 백틱으로 인용
        let quoted = quote_table_name(DbType::MySql, "my_table").unwrap();
        assert_eq!(quoted, "`my_table`");
    }

    #[test]
    fn quote_table_name_postgres_uses_double_quotes() {
        // Req 2.1, 14.2: PostgreSQL은 큰따옴표로 인용
        let quoted = quote_table_name(DbType::Postgres, "my_table").unwrap();
        assert_eq!(quoted, "\"my_table\"");
    }

    #[test]
    fn quote_table_name_rejects_unsafe_identifier() {
        // Req 2.5, 14.3: 위험 식별자는 Err을 반환하여 호출부에서 스킵할 수 있게 한다.
        let err = quote_table_name(DbType::MySql, "x; DROP TABLE y").unwrap_err();
        assert!(matches!(err, AppError::UnsafeIdentifier(_)));
        let err = quote_table_name(DbType::Postgres, "x;/*").unwrap_err();
        assert!(matches!(err, AppError::UnsafeIdentifier(_)));
    }
}
