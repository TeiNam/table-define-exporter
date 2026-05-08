// Feature: code-quality-improvements, Property 6: Markdown VIEW fenced code block
//
// Property 6: VIEW SQL 문자열 sql(연속 백틱 포함 여부 무관)에 대해 MarkdownExporter의
// VIEW 출력은 다음을 만족한다.
// - 언어 태그 "sql"을 가진 열기 펜스 라인, SQL 본문, 동일 길이 닫기 펜스를 각각
//   별도의 줄로 포함한다.
// - sql 내부의 가장 긴 연속 백틱 길이보다 펜스 길이가 크다.
//
// Validates: Requirements 3.1, 3.2, 3.3
//
// 참고: MarkdownExporter는 현재 작업 디렉터리에 {schema}.md 파일을 생성하므로,
// 테스트 간 cwd 간섭을 막기 위해 TempDir + 전역 Mutex 패턴을 사용한다
// (tests/export_predicate_test.rs와 동일한 패턴).

use proptest::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::sync::{Mutex, OnceLock};

use td_export::export::create_exporter;
use td_export::model::{
    DbType, GeneralInfo, OutputFormat, RunConfig, SchemaCatalog, TableDef, ViewInfo,
};
use td_export::secret::Password;
use tempfile::TempDir;

// ─────────────────────────────────────────────────────────────────────────────
// 테스트 고정 입력 빌더
// ─────────────────────────────────────────────────────────────────────────────

/// VIEW 타입 TableDef를 주어진 SQL 본문으로 구성한다.
fn make_view_table(sql: String) -> TableDef {
    TableDef {
        table_name: "v_example".to_string(),
        general: GeneralInfo {
            table_type: "VIEW".to_string(),
            engine: None,
            row_format: None,
            collate: None,
            comment: Some("VIEW 펜스 테스트".to_string()),
        },
        columns: vec![],
        indexes: vec![],
        constraints: vec![],
        view: Some(ViewInfo {
            view_query: sql,
            charset: "utf8mb4".to_string(),
            collate: "utf8mb4_general_ci".to_string(),
        }),
        ddl: None,
    }
}

/// 테스트용 기본 RunConfig. Markdown 경로는 endpoint/db_type에 의존하지 않지만
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

/// MarkdownExporter로 VIEW가 담긴 단일 테이블을 렌더링하여 결과 문자열을 반환한다.
///
/// MarkdownExporter는 cwd 기준으로 `{schema}.md`를 생성한다. 테스트 프로세스 내의
/// 다른 cwd-의존 테스트와의 레이스를 피하기 위해 전역 Mutex로 직렬화한다.
fn render_view_markdown(sql: String) -> String {
    static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = CWD_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("cwd mutex poisoned");

    let tmp = TempDir::new().expect("TempDir 생성 실패");
    let original_dir = std::env::current_dir().expect("current_dir 조회 실패");
    std::env::set_current_dir(tmp.path()).expect("cwd 변경 실패");

    let schema = "viewschema";
    let tables = vec![make_view_table(sql)];
    let mut catalog: SchemaCatalog = HashMap::new();
    catalog.insert(schema.to_string(), tables.clone());

    let config = make_run_config();
    let mut exporter = create_exporter(OutputFormat::Markdown);
    let setup_result = exporter.setup(&catalog, &config);
    let write_result = setup_result.and_then(|_| exporter.write_tables(schema, &tables));
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

/// VIEW 섹션의 열기 펜스 라인을 찾아 펜스 길이(백틱 개수)를 반환한다.
///
/// 렌더러는 "**View Create SQL**" 마커 → 빈 줄 → 열기 펜스("{fence}sql") 순서로
/// 기록하므로 marker + 2 위치에서 펜스 라인을 얻는다. 본문이 우연히 "sql"로
/// 끝나더라도 고정 위치 조회를 쓰므로 오탐이 없다.
fn find_view_fence_length(output: &str) -> Option<usize> {
    let lines: Vec<&str> = output.lines().collect();
    let marker_idx = lines.iter().position(|l| *l == "**View Create SQL**")?;
    let fence_line = lines.get(marker_idx + 2)?;

    let backtick_count = fence_line.chars().take_while(|&c| c == '`').count();
    // "sql" 태그(3글자)만 뒤따라야 하며 최소 3개 백틱이어야 유효한 펜스다.
    if backtick_count >= 3
        && fence_line.len() == backtick_count + 3
        && &fence_line[backtick_count..] == "sql"
    {
        Some(backtick_count)
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 6 (a): 기본 fenced code block 형태
// Validates: Requirements 3.1, 3.2
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    // 파일 I/O + cwd 전환이 포함되므로 실행 비용이 높아 케이스 수를 50으로 제한한다.
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 3.1, 3.2**
    ///
    /// Property 6 (a): 백틱이 없는 임의의 VIEW SQL에 대해 출력은
    /// - 언어 태그가 붙은 열기 펜스 라인 "```sql"을 별도 줄로 포함한다
    /// - 닫기 펜스 "```"을 별도 줄로 포함한다 (한 줄짜리 ```{sql}``` 패턴이 아님)
    ///
    /// 생성기는 백틱을 제외한 ASCII safe 집합으로 제한하여 shrinking이 간결하고
    /// 의도치 않은 펜스 간섭이 발생하지 않게 한다.
    #[test]
    fn prop6a_opens_and_closes_with_sql_fence(
        sql in "[A-Za-z0-9 _=;(),.*+\\-]{1,80}",
    ) {
        let output = render_view_markdown(sql.clone());

        // 열기 펜스: "```sql"이 한 줄 전체를 이루어야 함 (개행으로 경계 확인)
        prop_assert!(
            output.contains("\n```sql\n"),
            "출력에 한 줄짜리 ```sql 열기 펜스 라인이 없음\n--- 출력 ---\n{output}",
        );
        // 닫기 펜스: "```"이 한 줄 전체를 이루어야 함
        prop_assert!(
            output.contains("\n```\n"),
            "출력에 한 줄짜리 ``` 닫기 펜스 라인이 없음\n--- 출력 ---\n{output}",
        );
        // 옛 버그 패턴(같은 줄에 태그 + 본문)이 사라져야 함
        let bad_one_liner = format!("```{sql}```");
        prop_assert!(
            !output.contains(&bad_one_liner),
            "옛 한 줄 패턴 ```{{sql}}```가 출력에 남아 있음",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 6 (b): 본문 내 연속 백틱보다 펜스가 길다
// Validates: Requirements 3.3
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 3.3**
    ///
    /// Property 6 (b): 본문에 N개 연속 백틱이 존재할 때 펜스 길이는 N보다 크다.
    ///
    /// prefix/suffix 생성기는 백틱을 포함하지 않는 ASCII safe 집합으로 제한하여
    /// 본문 내 연속 백틱의 최장 길이가 정확히 `n`이 되도록 보장한다.
    /// n 상한을 10으로 두어 출력 크기를 억제한다.
    #[test]
    fn prop6b_fence_length_exceeds_body_backticks(
        prefix in "[A-Za-z0-9 _]{0,40}",
        n in 1usize..=10,
        suffix in "[A-Za-z0-9 _]{0,40}",
    ) {
        let sql = format!("{prefix}{}{suffix}", "`".repeat(n));
        let output = render_view_markdown(sql.clone());

        let fence_len = find_view_fence_length(&output)
            .unwrap_or_else(|| panic!("VIEW 펜스 라인을 찾지 못함\n--- 출력 ---\n{output}"));

        prop_assert!(
            fence_len > n,
            "펜스 길이({fence_len})가 본문 최장 연속 백틱({n})보다 크지 않음\n\
             --- 출력 ---\n{output}",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 6 (c): 열기 펜스·본문·닫기 펜스가 각각 별도 줄
// Validates: Requirements 3.1, 3.2
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// **Validates: Requirements 3.1, 3.2**
    ///
    /// Property 6 (c): 열기 펜스 라인, SQL 본문, 닫기 펜스 라인이 각각 별도의
    /// 줄로 출력된다. 본문은 단일 라인으로 제한하여 마커 기준 상대 위치로
    /// 구조를 확정적으로 검증한다 (multi-line body는 소스 레벨 단위 테스트로 보증).
    #[test]
    fn prop6c_three_parts_on_separate_lines(
        sql in "[A-Za-z0-9 _=;(),.*+\\-]{1,80}",
    ) {
        let output = render_view_markdown(sql.clone());
        let lines: Vec<&str> = output.lines().collect();

        let marker_idx = lines
            .iter()
            .position(|l| *l == "**View Create SQL**")
            .unwrap_or_else(|| panic!("**View Create SQL** 마커 없음\n{output}"));

        prop_assert_eq!(
            lines[marker_idx + 1],
            "",
            "마커 뒤에는 빈 줄이 와야 한다",
        );
        prop_assert_eq!(
            lines[marker_idx + 2],
            "```sql",
            "열기 펜스는 ```sql 단독 라인이어야 한다",
        );
        prop_assert_eq!(
            lines[marker_idx + 3],
            sql.as_str(),
            "본문은 열기 펜스 다음 줄에 원문 그대로 존재해야 한다",
        );
        prop_assert_eq!(
            lines[marker_idx + 4],
            "```",
            "닫기 펜스는 ``` 단독 라인이어야 한다",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 예시 기반 회귀 스모크 테스트 (property shrinking 실패 시 진단용)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn view_without_backticks_uses_default_triple_backtick_fence() {
    let output = render_view_markdown("SELECT 1".to_string());
    let fence_len = find_view_fence_length(&output).expect("fence not found");
    assert_eq!(fence_len, 3);
    assert!(output.contains("\n```sql\nSELECT 1\n```\n"));
}

#[test]
fn view_with_triple_backtick_body_expands_fence_to_four() {
    // 본문에 ``` 가 포함되면 펜스는 4개 이상이어야 한다.
    let sql = "SELECT '```' AS code";
    let output = render_view_markdown(sql.to_string());
    let fence_len = find_view_fence_length(&output).expect("fence not found");
    assert!(
        fence_len >= 4,
        "```이 본문에 있는데 펜스가 3개 이하({fence_len})로 생성됨"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 21.2: VIEW 포맷 회귀 방지 — 고정 샘플 예제 테스트
// Validates: Requirements 3, 15.4
// ─────────────────────────────────────────────────────────────────────────────

/// **Validates: Requirements 3, 15.4**
///
/// 고정 샘플 VIEW SQL을 MarkdownExporter에 전달했을 때, 출력이 언어 태그 `sql`을
/// 가진 fenced code block 패턴을 줄 단위로 포함하는지 검증한다.
///
/// 회귀 방지 목적: 15.1에서 도입한 "언어 태그 + 별도 줄" 규약이 이후 변경으로
/// 깨지지 않도록 대표 SQL에 대해 결정적 assert를 수행한다.
#[test]
fn view_fixed_sample_regression_contains_language_tagged_fence() {
    let sql = "SELECT id, name FROM users WHERE active";
    let output = render_view_markdown(sql.to_string());

    // 언어 태그 ```sql 열기 펜스가 별도 줄로 존재해야 함
    assert!(
        output.contains("\n```sql\n"),
        "언어 태그 ```sql 열기 펜스가 별도 줄에 없음\n--- 출력 ---\n{output}",
    );
    // 닫기 펜스 ``` 가 별도 줄로 존재해야 함
    assert!(
        output.contains("\n```\n"),
        "``` 닫기 펜스가 별도 줄에 없음\n--- 출력 ---\n{output}",
    );
    // SQL 본문 원문이 출력에 포함되어야 함
    assert!(
        output.contains(sql),
        "SQL 본문이 출력에 포함되지 않음\n--- 출력 ---\n{output}",
    );
    // 전체 fenced 블록이 한 단위(열기/본문/닫기가 연속된 줄)로 구성되어야 함
    let expected_block = "\n```sql\nSELECT id, name FROM users WHERE active\n```\n";
    assert!(
        output.contains(expected_block),
        "기대한 fenced 블록 패턴이 출력에 없음\n--- 출력 ---\n{output}",
    );
}
