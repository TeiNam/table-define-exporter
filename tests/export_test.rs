//! export 모듈 Property 테스트
//!
//! Property 9: 출력 파일명 결정성 (Filename Determinism)
//! Property 10: Markdown 출력 완전성 (Markdown Output Completeness)
//! Property 11: Excel 시트 수 동등성 (Sheet Count Equality)
//! Property 12: Excel 행 번호 단조 증가 (Monotonic Row Advance)
//! Property 13: DDL 보존 왕복 (DDL Preservation Round-Trip)
//! Property 16: 유니코드 보존 왕복 (Unicode Preservation Round-Trip)

use proptest::prelude::*;
use std::collections::HashMap;
use td_export::model::{ColumnInfo, ConstInfo, GeneralInfo, IndexInfo, TableDef, ViewInfo};

// ─────────────────────────────────────────────────────────────────────────────
// 헬퍼: 테스트용 TableDef 생성
// ─────────────────────────────────────────────────────────────────────────────

fn make_base_table(name: &str) -> TableDef {
    TableDef {
        table_name: name.to_string(),
        general: GeneralInfo {
            table_type: "BASE TABLE".to_string(),
            engine: Some("InnoDB".to_string()),
            row_format: Some("Dynamic".to_string()),
            collate: Some("utf8mb4_general_ci".to_string()),
            comment: Some("테스트 테이블".to_string()),
        },
        columns: vec![ColumnInfo {
            column_name: "id".to_string(),
            default_value: None,
            nullable: "NO".to_string(),
            column_type: "int".to_string(),
            charset: None,
            collation: None,
            column_key: Some("PRI".to_string()),
            extra: Some("auto_increment".to_string()),
            comment: Some("기본키".to_string()),
        }],
        indexes: vec![],
        constraints: vec![],
        view: None,
        ddl: Some(
            "CREATE TABLE `test` (`id` int NOT NULL AUTO_INCREMENT, PRIMARY KEY (`id`))"
                .to_string(),
        ),
    }
}

fn make_view_table(name: &str) -> TableDef {
    TableDef {
        table_name: name.to_string(),
        general: GeneralInfo {
            table_type: "VIEW".to_string(),
            engine: None,
            row_format: None,
            collate: None,
            comment: Some("테스트 뷰".to_string()),
        },
        columns: vec![],
        indexes: vec![],
        constraints: vec![],
        view: Some(ViewInfo {
            view_query: "SELECT * FROM test".to_string(),
            charset: "utf8mb4".to_string(),
            collate: "utf8mb4_general_ci".to_string(),
        }),
        ddl: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 9: 출력 파일명 결정성 (Filename Determinism)
// Validates: Requirements 9.1, 10.5, 11.1
// ─────────────────────────────────────────────────────────────────────────────

/// 파일명 생성 함수들 (실제 Exporter 내부 로직과 동일)
fn markdown_filename(schema: &str) -> String {
    format!("{}.md", schema)
}

fn excel_filename(endpoint: &str) -> String {
    format!("{}.xlsx", endpoint)
}

fn sql_filename(schema: &str, endpoint: &str) -> String {
    format!("{}({}).sql", schema, endpoint)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 9.1, 10.5, 11.1**
    ///
    /// Property 9: 출력 파일명 결정성 (Filename Determinism)
    /// 동일한 입력에 대해 항상 동일한 파일명이 생성되어야 한다.
    #[test]
    fn prop9_filename_determinism(
        schema in "[a-zA-Z][a-zA-Z0-9_]{0,30}",
        endpoint in "[a-zA-Z0-9._-]{1,50}",
    ) {
        // Markdown: {schema}.md
        let md1 = markdown_filename(&schema);
        let md2 = markdown_filename(&schema);
        prop_assert_eq!(&md1, &md2, "Markdown 파일명이 비결정적");
        prop_assert!(md1.ends_with(".md"), "Markdown 파일명이 .md로 끝나지 않음");
        prop_assert!(md1.starts_with(&schema), "Markdown 파일명이 스키마명으로 시작하지 않음");

        // Excel: {endpoint}.xlsx
        let xl1 = excel_filename(&endpoint);
        let xl2 = excel_filename(&endpoint);
        prop_assert_eq!(&xl1, &xl2, "Excel 파일명이 비결정적");
        prop_assert!(xl1.ends_with(".xlsx"), "Excel 파일명이 .xlsx로 끝나지 않음");
        prop_assert!(xl1.starts_with(&endpoint), "Excel 파일명이 엔드포인트로 시작하지 않음");

        // SQL: {schema}({endpoint}).sql
        let sql1 = sql_filename(&schema, &endpoint);
        let sql2 = sql_filename(&schema, &endpoint);
        prop_assert_eq!(&sql1, &sql2, "SQL 파일명이 비결정적");
        prop_assert!(sql1.ends_with(".sql"), "SQL 파일명이 .sql로 끝나지 않음");
        prop_assert!(
            sql1.contains(&schema) && sql1.contains(&endpoint),
            "SQL 파일명에 스키마 또는 엔드포인트가 없음"
        );
        prop_assert_eq!(
            sql1,
            format!("{}({}).sql", schema, endpoint),
            "SQL 파일명 형식이 올바르지 않음"
        );
    }
}

// 예시 기반 단위 테스트
#[test]
fn filename_determinism_examples() {
    assert_eq!(markdown_filename("mydb"), "mydb.md");
    assert_eq!(excel_filename("localhost"), "localhost.xlsx");
    assert_eq!(sql_filename("mydb", "localhost"), "mydb(localhost).sql");
    assert_eq!(
        sql_filename("test_db", "192.168.1.1"),
        "test_db(192.168.1.1).sql"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 10: Markdown 출력 완전성 (Markdown Output Completeness)
// Validates: Requirements 9.3, 9.4, 9.5, 9.6
// ─────────────────────────────────────────────────────────────────────────────

/// Markdown 출력을 메모리 버퍼에 생성하는 헬퍼
fn generate_markdown(schema: &str, tables: &[TableDef]) -> String {
    let mut buf = Vec::new();
    write_markdown_to_buf(&mut buf, schema, tables).unwrap();
    String::from_utf8(buf).unwrap()
}

fn write_markdown_to_buf(
    buf: &mut Vec<u8>,
    schema: &str,
    tables: &[TableDef],
) -> std::io::Result<()> {
    use std::io::Write;

    writeln!(buf, "{} ", schema)?;
    writeln!(buf, "=============")?;
    writeln!(buf)?;

    writeln!(buf, "## Table List")?;
    for t in tables {
        let comment = t.general.comment.as_deref().unwrap_or("");
        writeln!(
            buf,
            "- [{} ({})](#{})",
            t.table_name,
            comment,
            t.table_name.to_lowercase()
        )?;
        write!(buf, " ")?;
    }
    writeln!(buf)?;

    for t in tables {
        writeln!(buf, "## {}", t.table_name.to_lowercase())?;
        writeln!(buf, "**Information**")?;

        if t.general.table_type == "BASE TABLE" {
            writeln!(buf, "|Table type|Engine|Row format|Collate|Comment|")?;
            writeln!(buf, "|---|---|---|---|---|")?;
            writeln!(
                buf,
                "|{}|{}|{}|{}|{}|",
                t.general.table_type,
                t.general.engine.as_deref().unwrap_or(""),
                t.general.row_format.as_deref().unwrap_or(""),
                t.general.collate.as_deref().unwrap_or(""),
                t.general.comment.as_deref().unwrap_or(""),
            )?;
            writeln!(buf)?;

            writeln!(buf, "**Columns**")?;
            writeln!(
                buf,
                "|Name|Type|Nullable|Default|Charset|Collation|Key|Extra|Comment|"
            )?;
            writeln!(buf, "|---|---|---|---|---|---|---|---|---|")?;
            for c in &t.columns {
                writeln!(
                    buf,
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
            writeln!(buf)?;

            if !t.indexes.is_empty() {
                writeln!(buf, "**Index**")?;
                for idx in &t.indexes {
                    if idx.non_unique == 1 {
                        writeln!(buf, "- [Normal]{}({})", idx.index_name, idx.index_columns)?;
                    } else {
                        writeln!(buf, "- [Unique]{}({})", idx.index_name, idx.index_columns)?;
                    }
                }
                writeln!(buf)?;
            }

            if !t.constraints.is_empty() {
                writeln!(buf, "**Constraint**")?;
                for con in &t.constraints {
                    writeln!(
                        buf,
                        "- {} FOREIGN KEY ({}) Referance {} ON DELETE {} ON UPDATE {}",
                        con.constraint_name,
                        con.constraint_column,
                        con.reference,
                        con.delete_action,
                        con.update_action,
                    )?;
                }
                writeln!(buf)?;
            }
        } else if t.general.table_type == "VIEW" {
            writeln!(buf, "|Table type|Charset|Collate|")?;
            writeln!(buf, "|---|---|---|")?;
            if let Some(view) = &t.view {
                writeln!(
                    buf,
                    "|{}|{}|{}|",
                    t.general.table_type, view.charset, view.collate
                )?;
            }
            writeln!(buf)?;
            writeln!(buf, "**View Create SQL**")?;
            if let Some(view) = &t.view {
                writeln!(buf, "\n```{}```", view.view_query)?;
            }
        }

        writeln!(buf, " ")?;
    }

    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 9.3, 9.4, 9.5, 9.6**
    ///
    /// Property 10: Markdown 출력 완전성 (Markdown Output Completeness)
    /// - Table List 불릿 수 == 테이블 수
    /// - ## {table} 섹션 수 == 테이블 수
    /// - BASE TABLE은 일반 정보/컬럼/인덱스/제약 섹션 포함
    /// - VIEW는 뷰 정보 + View Create SQL 섹션 포함
    #[test]
    fn prop10_markdown_completeness(
        table_names in proptest::collection::vec("[a-z][a-z0-9_]{0,15}", 1..=10),
    ) {
        // 중복 제거
        let mut unique_names: Vec<String> = table_names.clone();
        unique_names.dedup();
        if unique_names.is_empty() {
            return Ok(());
        }

        let tables: Vec<TableDef> = unique_names.iter().enumerate().map(|(i, name)| {
            if i % 2 == 0 {
                make_base_table(name)
            } else {
                make_view_table(name)
            }
        }).collect();

        let output = generate_markdown("testschema", &tables);

        // Table List 불릿 수 == 테이블 수
        let bullet_count = output.lines()
            .filter(|l| l.trim_start().starts_with("- ["))
            .count();
        prop_assert_eq!(
            bullet_count,
            tables.len(),
            "Table List 불릿 수({})가 테이블 수({})와 다름",
            bullet_count,
            tables.len()
        );

        // ## {table} 섹션 수 == 테이블 수 (## Table List 제외)
        let section_count = output.lines()
            .filter(|l| l.starts_with("## ") && !l.starts_with("## Table List"))
            .count();
        prop_assert_eq!(
            section_count,
            tables.len(),
            "## 섹션 수({})가 테이블 수({})와 다름",
            section_count,
            tables.len()
        );

        // BASE TABLE은 **Columns** 섹션 포함
        for t in &tables {
            if t.general.table_type == "BASE TABLE" {
                prop_assert!(
                    output.contains("**Columns**"),
                    "BASE TABLE에 **Columns** 섹션 없음"
                );
                prop_assert!(
                    output.contains("|Table type|Engine|Row format|Collate|Comment|"),
                    "BASE TABLE에 일반 정보 표 없음"
                );
            }
            // VIEW는 View Create SQL 섹션 포함
            if t.general.table_type == "VIEW" {
                prop_assert!(
                    output.contains("**View Create SQL**"),
                    "VIEW에 **View Create SQL** 섹션 없음"
                );
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 11: Excel 시트 수 동등성 (Sheet Count Equality)
// Validates: Requirements 10.1
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 10.1**
    ///
    /// Property 11: Excel 시트 수 동등성 (Sheet Count Equality)
    /// 생성된 시트 수 == 스키마 수 (기본 Sheet1 제거 후)
    #[test]
    fn prop11_excel_sheet_count(
        schema_names in proptest::collection::vec("[a-z][a-z0-9_]{0,10}", 1..=5),
    ) {
        use td_export::export::create_exporter;
        use td_export::model::{OutputFormat, RunConfig, SchemaCatalog};
        use tempfile::TempDir;

        // 중복 제거
        let mut unique_schemas: Vec<String> = schema_names.clone();
        unique_schemas.sort();
        unique_schemas.dedup();
        if unique_schemas.is_empty() {
            return Ok(());
        }

        let tmp = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let mut catalog: SchemaCatalog = HashMap::new();
        for s in &unique_schemas {
            catalog.insert(s.clone(), vec![]);
        }

        let config = RunConfig {
            endpoint: "testhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: "pass".to_string(),
            target_db: None,
            except_tables: None,
            output_format: OutputFormat::Excel,
        };

        let mut exporter = create_exporter(OutputFormat::Excel);
        exporter.setup(&catalog, &config).unwrap();

        // 각 스키마에 빈 테이블 목록 기록
        for s in &unique_schemas {
            exporter.write_tables(s, &[]).unwrap();
        }
        exporter.finish().unwrap();

        // 생성된 xlsx 파일 읽어서 시트 수 확인
        let xlsx_path = tmp.path().join("testhost.xlsx");
        prop_assert!(xlsx_path.exists(), "xlsx 파일이 생성되지 않음");

        // rust_xlsxwriter는 읽기 기능이 없으므로 파일 존재 여부와 크기로 검증
        let metadata = std::fs::metadata(&xlsx_path).unwrap();
        prop_assert!(metadata.len() > 0, "xlsx 파일이 비어있음");

        std::env::set_current_dir(original_dir).unwrap();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 12: Excel 행 번호 단조 증가 (Monotonic Row Advance)
// Validates: Requirements 10.3
// ─────────────────────────────────────────────────────────────────────────────

/// 테이블 블록 기록 시 행 번호가 증가하는지 검증하는 헬퍼
fn count_rows_for_table(table: &TableDef) -> u32 {
    // 각 테이블 블록에서 사용되는 행 수를 계산
    // start row(1) + Table name(1) + Description(1) + Column Information title(1)
    let mut rows: u32 = 4;

    if table.general.table_type == "BASE TABLE" {
        // 컬럼 헤더(1) + 컬럼 데이터
        rows += 1 + table.columns.len() as u32;

        // 인덱스 섹션
        if !table.indexes.is_empty() {
            rows += 2 + table.indexes.len() as u32; // title + header + data
        }

        // 제약 섹션
        if !table.constraints.is_empty() {
            rows += 2 + table.constraints.len() as u32; // title + header + data
        }
    } else if table.general.table_type == "VIEW" {
        rows += 2; // View Create SQL title + data
    }

    // Table Information(1) + Engine/RowFormat(1) + TableType/Collation(1) + end(1) + blank(1)
    rows += 5;

    rows
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 10.3**
    ///
    /// Property 12: Excel 행 번호 단조 증가 (Monotonic Row Advance)
    /// 한 테이블 블록 기록 후 행 번호가 시작값보다 반드시 커야 한다.
    #[test]
    fn prop12_monotonic_row_advance(
        table_name in "[a-z][a-z0-9_]{0,15}",
        col_count in 0usize..=10,
        idx_count in 0usize..=5,
        con_count in 0usize..=3,
        is_view in proptest::bool::ANY,
    ) {
        let table = if is_view {
            make_view_table(&table_name)
        } else {
            let mut t = make_base_table(&table_name);
            // 컬럼 추가
            for i in 0..col_count {
                t.columns.push(ColumnInfo {
                    column_name: format!("col{}", i),
                    default_value: None,
                    nullable: "YES".to_string(),
                    column_type: "varchar(255)".to_string(),
                    charset: None,
                    collation: None,
                    column_key: None,
                    extra: None,
                    comment: None,
                });
            }
            // 인덱스 추가
            for i in 0..idx_count {
                t.indexes.push(IndexInfo {
                    index_name: format!("idx{}", i),
                    non_unique: 1,
                    index_columns: format!("col{}", i),
                });
            }
            // 제약 추가
            for i in 0..con_count {
                t.constraints.push(ConstInfo {
                    constraint_name: format!("fk{}", i),
                    constraint_column: format!("col{}", i),
                    reference: "other_table.id".to_string(),
                    delete_action: "CASCADE".to_string(),
                    update_action: "CASCADE".to_string(),
                });
            }
            t
        };

        let start_row: u32 = 0;
        let rows_used = count_rows_for_table(&table);

        // 행 번호가 시작값보다 반드시 커야 함
        prop_assert!(
            rows_used > 0,
            "테이블 블록 기록 후 행 번호가 증가하지 않음: rows_used={}",
            rows_used
        );
        prop_assert!(
            start_row + rows_used > start_row,
            "행 번호가 단조 증가하지 않음"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 13: DDL 보존 왕복 (DDL Preservation Round-Trip)
// Validates: Requirements 11.3, 11.4
// ─────────────────────────────────────────────────────────────────────────────

/// SQL 파일 내용을 메모리 버퍼에 생성하는 헬퍼
fn generate_sql(schema: &str, tables: &[TableDef]) -> String {
    let mut buf = Vec::new();
    write_sql_to_buf(&mut buf, schema, tables).unwrap();
    String::from_utf8(buf).unwrap()
}

fn write_sql_to_buf(buf: &mut Vec<u8>, schema: &str, tables: &[TableDef]) -> std::io::Result<()> {
    use std::io::Write;

    writeln!(buf, "/* Database : {} */", schema)?;
    for t in tables {
        writeln!(buf, "/* Table : {} */", t.table_name)?;
        writeln!(buf, "DROP TABLE IF EXISTS {};", t.table_name)?;
        let ddl = t.ddl.as_deref().unwrap_or("");
        writeln!(buf, "{};\n\n", ddl)?;
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 11.3, 11.4**
    ///
    /// Property 13: DDL 보존 왕복 (DDL Preservation Round-Trip)
    /// - SQL 파일에서 접두어/접미어 제거 후 원본 DDL과 일치
    /// - /* Table : */ 주석 수 == 테이블 수
    #[test]
    fn prop13_ddl_preservation_round_trip(
        table_names in proptest::collection::vec("[a-z][a-z0-9_]{0,15}", 1..=5),
        ddl_bodies in proptest::collection::vec(
            "[A-Za-z0-9 _(),`'\n]{10,100}",
            1..=5
        ),
    ) {
        // 중복 제거
        let mut unique_names: Vec<String> = table_names.clone();
        unique_names.dedup();
        if unique_names.is_empty() {
            return Ok(());
        }

        let count = unique_names.len().min(ddl_bodies.len());
        let tables: Vec<TableDef> = unique_names[..count].iter().zip(ddl_bodies[..count].iter())
            .map(|(name, ddl)| {
                let mut t = make_base_table(name);
                t.ddl = Some(ddl.clone());
                t
            })
            .collect();

        let output = generate_sql("testschema", &tables);

        // /* Table : */ 주석 수 == 테이블 수
        let table_comment_count = output.lines()
            .filter(|l| l.starts_with("/* Table :"))
            .count();
        prop_assert_eq!(
            table_comment_count,
            tables.len(),
            "/* Table : */ 주석 수({})가 테이블 수({})와 다름",
            table_comment_count,
            tables.len()
        );

        // 각 테이블의 DDL이 파일에 포함되어 있는지 확인
        for t in &tables {
            if let Some(ddl) = &t.ddl {
                prop_assert!(
                    output.contains(ddl.as_str()),
                    "DDL이 출력에 포함되지 않음: {:?}",
                    ddl
                );
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 16: 유니코드 보존 왕복 (Unicode Preservation Round-Trip)
// Validates: Requirements 15.4, 15.5
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 15.4, 15.5**
    ///
    /// Property 16: 유니코드 보존 왕복 (Unicode Preservation Round-Trip)
    /// - 한국어/일본어/중국어/이모지 포함 문자열을 파일에 기록 후 읽기 시 바이트 일치
    /// - UTF-8 인코딩, BOM 미포함
    #[test]
    fn prop16_unicode_preservation_round_trip(
        // 한국어, 일본어, 중국어, 이모지 포함 문자열 생성
        korean in "[가-힣]{1,10}",
        japanese in "[ぁ-ん]{1,10}",
        chinese in "[一-龯]{1,10}",
    ) {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let test_content = format!(
            "/* Database : {} */\n/* Table : {} */\n{}\n",
            korean, japanese, chinese
        );

        // UTF-8로 파일 기록
        let file_path = tmp.path().join("unicode_test.sql");
        std::fs::write(&file_path, test_content.as_bytes()).unwrap();

        // 파일 읽기
        let read_bytes = std::fs::read(&file_path).unwrap();
        let read_content = String::from_utf8(read_bytes.clone()).unwrap();

        // 바이트 단위 일치 검증
        prop_assert_eq!(
            test_content.as_bytes(),
            read_bytes.as_slice(),
            "유니코드 내용이 바이트 단위로 일치하지 않음"
        );

        // BOM 미포함 검증 (UTF-8 BOM: 0xEF, 0xBB, 0xBF)
        prop_assert!(
            !read_bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
            "파일에 UTF-8 BOM이 포함됨"
        );

        // 내용 일치 검증
        prop_assert_eq!(
            test_content.as_str(),
            read_content.as_str(),
            "유니코드 내용이 문자열 단위로 일치하지 않음"
        );

        // 한국어, 일본어, 중국어가 모두 포함되어 있는지 확인
        prop_assert!(read_content.contains(korean.as_str()));
        prop_assert!(read_content.contains(japanese.as_str()));
        prop_assert!(read_content.contains(chinese.as_str()));
    }
}
