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
    /// 스키마명 → 테이블보다 먼저 쓸 문장 (PostgreSQL 사용자 타입·시퀀스)
    preambles: HashMap<String, Vec<String>>,
}

impl SqlExporter {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            endpoint: String::new(),
            // 초기값은 MySQL. 실제 값은 `setup`에서 `config.db_type`으로 덮어쓴다.
            db_type: DbType::MySql,
            preambles: HashMap::new(),
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
        let source = super::source_label(config);
        for (schema, filename) in super::schema_filenames(catalog.keys(), &source, "sql") {
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
        let preamble = self.preambles.remove(schema).unwrap_or_default();
        let file = match self.files.get_mut(schema) {
            Some(f) => f,
            None => return Ok(()),
        };

        write_sql(file, schema, &preamble, tables, self.db_type)
            .map_err(|source| AppError::FileWrite { source })
    }

    fn set_schema_preamble(&mut self, schema: &str, statements: Vec<String>) {
        self.preambles.insert(schema.to_string(), statements);
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

/// 뷰를 서로의 참조 순서대로 정렬한다 (참조되는 뷰가 먼저, 나머지는 입력 순서 = 이름 순).
///
/// 정의 SQL 에 다른 뷰의 한정 이름이 나오면 의존으로 본다 — PostgreSQL 은 검색 경로를 비워
/// 참조가 항상 `schema.view` 로, MySQL 의 SHOW CREATE VIEW 는 항상 `` `db`.`view` `` 로 나온다.
/// 문자열 리터럴 속 이름 같은 오탐으로 순환이 생기면 남은 뷰는 입력 순서대로 둔다.
fn order_views<'a>(schema: &str, views: Vec<&'a TableDef>, db_type: DbType) -> Vec<&'a TableDef> {
    let deps: Vec<Vec<usize>> = views
        .iter()
        .map(|view| {
            let ddl = view.ddl.as_deref().unwrap_or("");
            views
                .iter()
                .enumerate()
                .filter(|(_, other)| other.table_name != view.table_name)
                .filter(|(_, other)| references_view(ddl, schema, &other.table_name, db_type))
                .map(|(i, _)| i)
                .collect()
        })
        .collect();
    let mut done = vec![false; views.len()];
    let mut ordered = Vec::with_capacity(views.len());
    while ordered.len() < views.len() {
        let ready = (0..views.len()).find(|&i| !done[i] && deps[i].iter().all(|&d| done[d]));
        // 순환(오탐)이면 남은 것 중 첫 번째를 그대로 낸다
        let next = ready.unwrap_or_else(|| (0..views.len()).find(|&i| !done[i]).unwrap_or(0));
        done[next] = true;
        ordered.push(views[next]);
    }
    ordered
}

/// `ddl` 이 `schema.name` 뷰를 참조하는가 (DB 별 인용 형태, 앞뒤가 식별자 문자가 아닐 때만)
fn references_view(ddl: &str, schema: &str, name: &str, db_type: DbType) -> bool {
    let needles: Vec<String> = match db_type {
        DbType::MySql => vec![format!(
            "`{}`.`{}`",
            schema.replace('`', "``"),
            name.replace('`', "``")
        )],
        DbType::Postgres => {
            let quoted = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
            [schema.to_string(), quoted(schema)]
                .iter()
                .flat_map(|s| [format!("{s}.{name}"), format!("{s}.{}", quoted(name))])
                .collect()
        }
    };
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    needles.iter().any(|needle| {
        ddl.match_indices(needle.as_str()).any(|(i, m)| {
            let before = ddl[..i].chars().next_back();
            let after = ddl[i + m.len()..].chars().next();
            !before.is_some_and(|c| is_ident(c) || c == '"' || c == '.')
                && !after.is_some_and(is_ident)
        })
    })
}

/// SQL 블록 주석 안에 넣을 이름. `/*`(PostgreSQL 은 주석 중첩)·`*/`·줄바꿈을 끊어
/// 스키마/테이블 이름으로 주석을 닫고 문장을 끼워 넣지 못하게 한다.
fn comment_text(name: &str) -> String {
    name.replace("/*", "/ *")
        .replace("*/", "* /")
        .replace(['\n', '\r'], " ")
}

/// SQL 내용을 파일에 기록하는 내부 함수
fn write_sql(
    file: &mut File,
    schema: &str,
    preamble: &[String],
    tables: &[TableDef],
    db_type: DbType,
) -> std::io::Result<()> {
    let terminator = Terminator::from_db_type(db_type);

    // 데이터베이스 헤더 주석
    writeln!(file, "/* Database : {} */", comment_text(schema))?;
    match db_type {
        // 파일은 UTF-8 이고 리터럴은 '' 이스케이프만 쓴다 — 복원 세션 설정과 무관하게 해석되도록
        // 고정한다 (standard_conforming_strings=off 면 '\'' 가 문자열을 탈출해 주입이 된다)
        DbType::Postgres => {
            writeln!(
                file,
                "SET client_encoding = 'UTF8';\nSET standard_conforming_strings = on;"
            )?;
            // DDL 이 스키마로 한정돼 있으므로 빈 DB 에서도 실행되게 스키마부터 만든다
            match quote_pg_identifier(schema) {
                Ok(quoted) => writeln!(file, "CREATE SCHEMA IF NOT EXISTS {quoted};\n")?,
                Err(e) => {
                    tracing::warn!(schema, error = %e, "위험한 스키마 이름 — CREATE SCHEMA 생략");
                    writeln!(file)?;
                }
            }
        }
        // MySQL: 한글 코멘트 등이 깨지지 않게 문자셋을 고정하고, FK 가 뒤에 나오는 테이블을
        // 참조해도 실행되도록 검사를 잠시 끈다 (mysqldump 와 동일)
        DbType::MySql => writeln!(
            file,
            "SET NAMES utf8mb4;\nSET @OLD_FOREIGN_KEY_CHECKS = @@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS = 0;\n"
        )?,
    }

    // 테이블이 참조하는 사용자 타입·시퀀스를 먼저 (pg_dump 와 동일)
    if !preamble.is_empty() {
        writeln!(file, "/* Types & Sequences */")?;
        for statement in preamble {
            writeln!(file, "{statement}")?;
        }
        writeln!(file)?;
    }

    // 모든 테이블을 만든 뒤 실행할 문장 (PostgreSQL FK)
    let mut deferred: Vec<&str> = Vec::new();
    // 뷰는 참조하는 테이블이 먼저 있어야 하므로 테이블을 모두 쓴 뒤에, 뷰끼리는 참조 순서대로 쓴다.
    let views: Vec<&TableDef> = tables.iter().filter(|t| t.general.is_view()).collect();
    let ordered = tables
        .iter()
        .filter(|t| !t.general.is_view())
        .chain(order_views(schema, views, db_type));
    for t in ordered {
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
        writeln!(file, "/* Table : {} */", comment_text(&t.table_name))?;
        // CREATE DDL만 출력 — DROP 구문은 제외. 원본을 보존하되 Terminator로 정확히 하나의 `;` 종결
        let ddl = t.ddl.as_deref().unwrap_or("");
        writeln!(file, "{}\n\n", terminator.apply(ddl))?;
        deferred.extend(t.ddl_after.iter().map(String::as_str));
    }

    // FK 는 참조 대상 테이블이 모두 만들어진 뒤 추가 (pg_dump 와 동일)
    if !deferred.is_empty() {
        writeln!(file, "/* Foreign Keys */")?;
        for statement in deferred {
            writeln!(file, "{statement}")?;
        }
        writeln!(file)?;
    }
    if db_type == DbType::MySql {
        writeln!(file, "SET FOREIGN_KEY_CHECKS = @OLD_FOREIGN_KEY_CHECKS;")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(db_type: DbType, tables: &[TableDef]) -> String {
        use std::io::{Read, Seek};
        let mut file = tempfile::tempfile().unwrap();
        write_sql(&mut file, "s", &[], tables, db_type).unwrap();
        file.rewind().unwrap();
        let mut out = String::new();
        file.read_to_string(&mut out).unwrap();
        out
    }

    fn table(name: &str, create: &str, after: &[&str]) -> TableDef {
        TableDef {
            table_name: name.to_string(),
            ddl: Some(create.to_string()),
            ddl_after: after.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn postgres_foreign_keys_come_after_all_tables() {
        let out = render(
            DbType::Postgres,
            &[
                table(
                    "child",
                    "CREATE TABLE child (x int)",
                    &["ALTER TABLE child ADD FK;"],
                ),
                table("parent", "CREATE TABLE parent (a int)", &[]),
            ],
        );
        let fk = out.find("ALTER TABLE child ADD FK;").unwrap();
        assert!(out.find("CREATE TABLE parent").unwrap() < fk, "{out}");
        assert!(
            out.contains("/* Foreign Keys */\nALTER TABLE child ADD FK;\n"),
            "{out}"
        );
        assert!(!out.contains("FOREIGN_KEY_CHECKS"), "{out}");
    }

    #[test]
    fn preamble_comes_before_tables() {
        use std::io::{Read, Seek};
        let mut file = tempfile::tempfile().unwrap();
        let preamble = vec!["CREATE TYPE mood AS ENUM ('ok');".to_string()];
        let tables = [table("t", "CREATE TABLE t (m mood)", &[])];
        write_sql(&mut file, "s", &preamble, &tables, DbType::Postgres).unwrap();
        file.rewind().unwrap();
        let mut out = String::new();
        file.read_to_string(&mut out).unwrap();
        let ty = out
            .find("/* Types & Sequences */\nCREATE TYPE mood")
            .unwrap();
        assert!(ty < out.find("CREATE TABLE t").unwrap(), "{out}");
    }

    #[test]
    fn comment_text_cannot_close_or_nest_comments() {
        let name = "x*/ DROP TABLE t; /*\nnext";
        let safe = comment_text(name);
        assert!(
            !safe.contains("*/") && !safe.contains("/*") && !safe.contains('\n'),
            "{safe}"
        );
        let out = render(
            DbType::Postgres,
            &[table(name, "CREATE TABLE t (id int)", &[])],
        );
        assert!(!out.contains("x*/ DROP"), "{out}");
    }

    #[test]
    fn headers_pin_encoding_and_string_semantics() {
        let pg = render(DbType::Postgres, &[]);
        assert!(
            pg.contains(
                "SET client_encoding = 'UTF8';\nSET standard_conforming_strings = on;\n\
                 CREATE SCHEMA IF NOT EXISTS \"s\";"
            ),
            "{pg}"
        );
        let my = render(DbType::MySql, &[]);
        assert!(my.contains("SET NAMES utf8mb4;"), "{my}");
    }

    #[test]
    fn views_come_after_tables() {
        let mut view = table("a_view", "CREATE VIEW a_view AS SELECT id FROM t", &[]);
        view.general.table_type = "VIEW".to_string();
        let out = render(
            DbType::MySql,
            &[view, table("t", "CREATE TABLE t (id int)", &[])],
        );
        assert!(
            out.find("CREATE TABLE t").unwrap() < out.find("CREATE VIEW a_view").unwrap(),
            "{out}"
        );
    }

    #[test]
    fn views_are_ordered_by_dependency() {
        let view = |name: &str, ddl: &str| {
            let mut v = table(name, ddl, &[]);
            v.general.table_type = "VIEW".to_string();
            v
        };
        // a_view 가 z_view 를 참조 → 이름 순서와 반대로 z_view 가 먼저
        let pg = render(
            DbType::Postgres,
            &[
                view(
                    "a_view",
                    "CREATE VIEW \"s\".\"a_view\" AS\n SELECT id FROM s.z_view;",
                ),
                view(
                    "z_view",
                    "CREATE VIEW \"s\".\"z_view\" AS\n SELECT id FROM s.base;",
                ),
                table("base", "CREATE TABLE s.base (id int)", &[]),
            ],
        );
        let pos = |needle: &str| pg.find(needle).unwrap();
        assert!(pos("CREATE TABLE s.base") < pos("\"z_view\" AS"), "{pg}");
        assert!(pos("\"z_view\" AS") < pos("\"a_view\" AS"), "{pg}");
        // 이름 일부만 겹치는 경우(s.z_view2)는 의존이 아니다
        assert!(!references_view(
            "SELECT 1 FROM s.z_view2",
            "s",
            "z_view",
            DbType::Postgres
        ));
        assert!(references_view(
            "FROM `s`.`z_view` x",
            "s",
            "z_view",
            DbType::MySql
        ));
        // 순환(오탐)이어도 모든 뷰를 한 번씩 낸다
        let cyc = render(
            DbType::Postgres,
            &[
                view("v1", "CREATE VIEW v1 AS SELECT 's.v2'"),
                view("v2", "CREATE VIEW v2 AS SELECT 's.v1'"),
            ],
        );
        assert_eq!(cyc.matches("CREATE VIEW").count(), 2, "{cyc}");
    }

    #[test]
    fn mysql_disables_and_restores_foreign_key_checks() {
        let out = render(
            DbType::MySql,
            &[table("child", "CREATE TABLE child (x int)", &[])],
        );
        let off = out
            .find("SET @OLD_FOREIGN_KEY_CHECKS = @@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS = 0;")
            .unwrap();
        let create = out.find("CREATE TABLE child").unwrap();
        let restore = out
            .find("SET FOREIGN_KEY_CHECKS = @OLD_FOREIGN_KEY_CHECKS;")
            .unwrap();
        assert!(off < create && create < restore, "{out}");
        assert!(!out.contains("/* Foreign Keys */"), "{out}");
    }

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
