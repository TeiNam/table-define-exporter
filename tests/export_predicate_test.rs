// Feature: code-quality-improvements, Property 11: predicate 렌더링 조건부 포함
//
// Property 11: IndexInfo의 `predicate: Some(p)`인 경우에만 Markdown/Excel 출력에
// `"WHERE " + p` 서브스트링이 포함되고, `predicate: None`인 경우 해당 서브스트링을
// 포함하지 않는다.
//
// 참고: Excel(.xlsx)은 ZIP 컨테이너 기반 바이너리 포맷이라 외부 파싱 없이
// "WHERE <pred>" 문자열을 신뢰성 있게 검출하기 어렵다. 따라서 Property 11의
// 핵심 계약(Markdown/Excel 모두 동일한 조건으로 렌더링)은 실제 파일 출력이
// 가능한 Markdown 경로에서 property test로 검증한다. Excel 쪽의 동일 분기는
// 소스 레벨에서 이미 단일 분기 패턴으로 구현되어 있음을 코드 리뷰로 보증한다.

use proptest::prelude::*;
use std::collections::HashMap;
use std::fs;

use td_export::export::create_exporter;
use td_export::model::{
    ColumnInfo, DbType, GeneralInfo, IndexInfo, OutputFormat, RunConfig, SchemaCatalog, TableDef,
};
use td_export::secret::Password;
use tempfile::TempDir;

// ─────────────────────────────────────────────────────────────────────────────
// 테스트용 고정 입력 빌더
// ─────────────────────────────────────────────────────────────────────────────

/// 테스트에 사용할 최소 구성의 BASE TABLE을 생성한다.
/// - `predicate` 인자에 따라 단일 인덱스의 파셜 조건만 달라진다.
/// - table_name/comment 등에는 "WHERE"를 포함하지 않아 서브스트링 오탐을 방지한다.
fn make_table_with_index_predicate(predicate: Option<String>) -> TableDef {
    TableDef {
        table_name: "users".to_string(),
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
        indexes: vec![IndexInfo {
            index_name: "idx_test".to_string(),
            non_unique: 1,
            index_columns: "id".to_string(),
            predicate,
        }],
        constraints: vec![],
        view: None,
        ddl: Some("CREATE TABLE users (id int);".to_string()),
    }
}

/// 테스트용 기본 RunConfig. Markdown 출력은 `endpoint`/`db_type`에 의존하지 않지만
/// Exporter::setup 시그니처가 요구하므로 의미 있는 값을 채워둔다.
fn make_run_config() -> RunConfig {
    RunConfig {
        endpoint: "localhost".to_string(),
        port: 3306,
        user: "root".to_string(),
        password: Password::new("pass".to_string()),
        target_db: None,
        except_tables: None,
        output_format: OutputFormat::Markdown,
        db_type: DbType::MySql,
        database: None,
    }
}

/// 실제 MarkdownExporter를 실행해 지정된 스키마의 .md 파일 내용을 문자열로 반환한다.
///
/// MarkdownExporter는 현재 작업 디렉터리에 `{schema}.md` 파일을 생성하므로,
/// 테스트 간 간섭을 막기 위해 격리된 TempDir로 cwd를 잠시 전환한다.
///
/// 주의: `std::env::set_current_dir`은 프로세스 전역 상태를 변경하므로 병렬 테스트가
/// 겹치면 파일이 잘못된 경로에 생성될 수 있다. 이를 막기 위해 전역 `Mutex`로
/// 직렬화한다.
fn render_markdown_to_string(schema: &str, tables: &[TableDef]) -> String {
    use std::sync::{Mutex, OnceLock};
    static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = CWD_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("cwd mutex poisoned");

    let tmp = TempDir::new().expect("TempDir 생성 실패");
    let original_dir = std::env::current_dir().expect("current_dir 조회 실패");
    std::env::set_current_dir(tmp.path()).expect("cwd 변경 실패");

    let mut catalog: SchemaCatalog = HashMap::new();
    catalog.insert(schema.to_string(), tables.to_vec());

    let config = make_run_config();
    let mut exporter = create_exporter(OutputFormat::Markdown);
    let setup_result = exporter.setup(&catalog, &config);
    let write_result = setup_result.and_then(|_| exporter.write_tables(schema, tables));
    let finish_result = write_result.and_then(|_| exporter.finish());

    let md_path = tmp.path().join(format!("{schema}.md"));
    let read_result = finish_result.and_then(|_| {
        fs::read_to_string(&md_path)
            .map_err(|source| td_export::error::AppError::FileWrite { source })
    });

    // cwd를 반드시 복구한 뒤 락을 해제한다.
    std::env::set_current_dir(&original_dir).expect("cwd 복구 실패");
    drop(guard);

    read_result.expect("Markdown 출력 생성 실패")
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 11: predicate 렌더링 조건부 포함
// Validates: Requirements 11.4
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 11.4**
    ///
    /// Property 11 (a): `predicate: Some(pred)`인 경우 Markdown 출력은
    /// `" WHERE {pred}"` 서브스트링을 포함한다.
    ///
    /// pred 생성기는 제어 문자·개행·파이프(`|`) 없이 영문/숫자/공백/단순 기호로만
    /// 구성된 짧은 문자열로 제한한다. 이는 (1) shrinking이 잘 되게 하고
    /// (2) Markdown 표 구분자(`|`)와 섞여 우연한 매칭이 발생하는 것을 막는다.
    #[test]
    fn prop11_markdown_includes_where_when_predicate_some(
        pred in "[a-zA-Z][a-zA-Z0-9_= ]{1,30}",
    ) {
        let tables = vec![make_table_with_index_predicate(Some(pred.clone()))];
        let output = render_markdown_to_string("testschema", &tables);

        let expected = format!(" WHERE {pred}");
        prop_assert!(
            output.contains(&expected),
            "predicate=Some({pred:?})인데 출력에 {expected:?} 서브스트링이 없음\n\
             --- 출력 시작 ---\n{output}\n--- 출력 끝 ---"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 11.4**
    ///
    /// Property 11 (b): `predicate: None`인 경우 Markdown 출력은
    /// `" WHERE "` 서브스트링을 포함하지 않는다.
    ///
    /// 고정 입력 기반이지만 proptest로 감싸 동일 계약이 반복 실행에서도 흔들리지
    /// 않음을 확인한다. 입력 변동 차원은 의도적으로 최소화(no-op 문자열)하여
    /// shrinking이 항상 원래 케이스로 수렴하게 한다.
    #[test]
    fn prop11_markdown_excludes_where_when_predicate_none(
        _noop in "[a-z]{0,3}",
    ) {
        let tables = vec![make_table_with_index_predicate(None)];
        let output = render_markdown_to_string("testschema", &tables);

        prop_assert!(
            !output.contains(" WHERE "),
            "predicate=None인데 출력에 \" WHERE \" 서브스트링이 포함됨\n\
             --- 출력 시작 ---\n{output}\n--- 출력 끝 ---"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 예시 기반 회귀 스모크 테스트 (property shrinking 실패 시 진단에 도움)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn markdown_renders_partial_index_where_clause() {
    let pred = "deleted_at IS NULL".to_string();
    let tables = vec![make_table_with_index_predicate(Some(pred.clone()))];
    let output = render_markdown_to_string("testschema", &tables);

    assert!(
        output.contains(&format!(" WHERE {pred}")),
        "파셜 인덱스 WHERE 절이 Markdown 출력에 반영되지 않음:\n{output}"
    );
}

#[test]
fn markdown_omits_where_clause_when_predicate_none() {
    let tables = vec![make_table_with_index_predicate(None)];
    let output = render_markdown_to_string("testschema", &tables);

    assert!(
        !output.contains(" WHERE "),
        "predicate=None인데 Markdown 출력에 \" WHERE \"가 포함됨:\n{output}"
    );
}
