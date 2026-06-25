//! 비즈니스 로직 오케스트레이션 (business logic orchestration).
//!
//! `main.rs`에서 분리된 CLI 실행 진입점. `anyhow::Result`를 반환하여
//! `?` 연산자와 `.context(...)`로 에러 전파를 단일화한다.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;

use td_export::{
    concurrency::buffer_metadata,
    config::{self, CliOverrides},
    db::{DbClient, DbClientEnum},
    export::create_exporter,
    model::{DbType, OutputFormat, TableDef},
};

/// 테이블 메타데이터 동시 조회 상한.
///
/// MySQL/PostgreSQL 커넥션 풀 크기(4)와 정합하도록 설정한다 (Req 9.2).
/// `buffered(N)`는 입력 순서를 보존하므로 병렬화 후에도 결과 순서가
/// 결정적이며, 출력 파일의 바이트 동일성이 유지된다 (Req 9.4, 9.5).
const METADATA_CONCURRENCY: usize = 4;

/// Table Definition Export - MySQL/PostgreSQL 테이블 정의서 내보내기 도구
///
/// 모든 플래그는 선택사항입니다. 지정되지 않은 값은 실행 시 대화형으로 입력받습니다.
/// 비밀번호는 보안상 항상 프롬프트로만 입력받으며 CLI 플래그로 받지 않습니다.
#[derive(Parser)]
#[command(
    name = "td-export",
    version,
    about = "Table Definition Export - MySQL/PostgreSQL 테이블 정의서를 Excel/Markdown/SQL로 내보냅니다"
)]
struct Cli {
    /// 출력 포맷: excel, markdown, sql
    #[arg(long, value_name = "FORMAT", value_enum)]
    output: Option<OutputFormat>,

    /// DB 종류: mysql, postgres
    #[arg(long = "db-type", value_name = "TYPE", value_enum)]
    db_type: Option<DbType>,

    /// DB 서버 호스트명 또는 IP
    #[arg(long, value_name = "HOST")]
    endpoint: Option<String>,

    /// DB 서버 포트 (미지정 시 DB 종류별 기본값: mysql=3306, postgres=5432)
    #[arg(long, value_name = "PORT")]
    port: Option<u16>,

    /// DB 사용자명
    #[arg(long, value_name = "USER")]
    user: Option<String>,

    /// PostgreSQL 데이터베이스 이름 (PostgreSQL 전용, 필수)
    #[arg(long, value_name = "NAME")]
    database: Option<String>,

    /// 대상 스키마 목록 (쉼표 구분, 미지정 시 전체 비시스템 스키마)
    #[arg(long = "target-db", value_name = "SCHEMAS", value_delimiter = ',')]
    target_db: Option<Vec<String>>,

    /// 제외 테이블 패턴 (쉼표 구분, 와일드카드 `%` 사용 가능)
    #[arg(long = "except-tables", value_name = "PATTERNS", value_delimiter = ',')]
    except_tables: Option<Vec<String>>,
}

impl Cli {
    /// 파싱된 CLI 인자를 `CliOverrides`로 변환한다.
    ///
    /// `output`과 `db_type`은 `clap::ValueEnum`을 통해 이미 타입 수준에서
    /// 검증되었으므로 추가 파싱이 필요 없다.
    fn into_overrides(self) -> CliOverrides {
        CliOverrides {
            output_format: self.output,
            db_type: self.db_type,
            endpoint: self.endpoint,
            port: self.port,
            user: self.user,
            database: self.database,
            target_db: self.target_db,
            except_tables: self.except_tables,
        }
    }
}

/// 단일 테이블에 대한 메타데이터(컬럼/인덱스/제약/뷰/DDL)를 조회해
/// `TableDef`를 채워 반환한다.
///
/// 실패 시 `tracing::error!`로 로그를 남기고 해당 필드를 비운 채
/// 원본 `TableDef`를 반환한다 (직렬 루프에서 `continue`와 동등한 의미).
/// 이로써 한 테이블의 실패가 다른 테이블 수집을 막지 않는다 (Req 9.3).
async fn enrich_table(
    db: Arc<DbClientEnum>,
    schema: Arc<str>,
    table: TableDef,
    output_format: OutputFormat,
) -> TableDef {
    let started = std::time::Instant::now();
    let table = enrich_table_inner(db, Arc::clone(&schema), table, output_format).await;

    // 추출 시간: 1초 초과 시 구문 없이 시간만 경고 (느린 테이블 식별용)
    let elapsed = started.elapsed();
    if elapsed.as_secs_f64() >= 1.0 {
        tracing::warn!(
            "{}.{} slow extract: {:.2}s",
            schema,
            table.table_name,
            elapsed.as_secs_f64()
        );
    }
    table
}

async fn enrich_table_inner(
    db: Arc<DbClientEnum>,
    schema: Arc<str>,
    mut table: TableDef,
    output_format: OutputFormat,
) -> TableDef {
    match output_format {
        OutputFormat::Excel | OutputFormat::Markdown => {
            if table.general.table_type == "BASE TABLE" {
                // 개별 테이블 실패는 warn 레벨로 로그하여 Req 9.3 충족:
                // "한 테이블의 메타데이터 조회가 실패할 때, 해당 테이블의 에러를
                // 로그하고 다른 테이블의 수집을 계속 진행한다."
                // Req 6.3의 "ERROR 수준 이벤트 단일성"은 이 경로가 warn이어야
                // 성립하므로 두 요구사항이 동시에 만족된다.
                match db.get_columns(&schema, &table.table_name).await {
                    Ok(cols) => table.columns = cols,
                    Err(e) => {
                        tracing::warn!("{} - {}: {}", schema, table.table_name, e);
                        return table;
                    }
                }
                match db.get_indexes(&schema, &table.table_name).await {
                    Ok(idxs) => table.indexes = idxs,
                    Err(e) => {
                        tracing::warn!("{} - {}: {}", schema, table.table_name, e);
                        return table;
                    }
                }
                match db.get_constraints(&schema, &table.table_name).await {
                    Ok(cons) => table.constraints = cons,
                    Err(e) => {
                        tracing::warn!("{} - {}: {}", schema, table.table_name, e);
                        return table;
                    }
                }
            } else if table.general.table_type == "VIEW" {
                match db.get_view_info(&schema, &table.table_name).await {
                    Ok(view) => table.view = Some(view),
                    Err(e) => {
                        tracing::warn!("{} - {}: {}", schema, table.table_name, e);
                        return table;
                    }
                }
            }
        }
        OutputFormat::Sql => match db.get_table_ddl(&schema, &table.table_name).await {
            Ok(ddl) => table.ddl = Some(ddl),
            Err(e) => {
                tracing::warn!("{} - {}: {}", schema, table.table_name, e);
                return table;
            }
        },
    }
    table
}

/// CLI 진입점. 모든 에러는 `anyhow::Error`로 래핑되어 호출자(main)에서 단일
/// 지점으로 로그된다.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    // 앱 이름/버전 로그
    tracing::info!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    // CLI 오버라이드 변환 (clap ValueEnum이 입력값을 이미 검증)
    let overrides = cli.into_overrides();

    // 대화식 설정 수집 (CLI 오버라이드가 있는 필드는 프롬프트 생략)
    let config = config::load_config(overrides).context("설정 로드 실패")?;

    let output_format = config.output_format;

    // DB 연결. `Arc`로 감싸서 병렬 메타데이터 수집 시 각 future가 공유할 수
    // 있도록 한다 (`buffered`는 future가 `'static`이어야 함).
    let db = Arc::new(
        DbClientEnum::connect(&config)
            .await
            .context("DB 연결 실패")?,
    );
    tracing::info!("DB Connect Success");

    // 스키마 목록 조회
    let catalog = db
        .get_schemas(&config)
        .await
        .context("스키마 목록 조회 실패")?;

    if catalog.is_empty() {
        // 스키마가 비어있는 경우는 실패 경로: 기존 동작 유지 (exit 1 -> Err 반환)
        anyhow::bail!("Not in Schema.");
    }

    // Exporter 초기화
    let mut exporter = create_exporter(output_format);
    exporter
        .setup(&catalog, &config)
        .context("Exporter 초기화 실패")?;
    tracing::info!("Setup {} Files", output_format.display_name());

    tracing::info!("Get Schema Count : {}", catalog.len());

    // 스키마별 루프
    let mut schema_names: Vec<String> = catalog.keys().cloned().collect();
    schema_names.sort();

    for schema in &schema_names {
        tracing::info!("{} Table Load.", schema);

        // 테이블 목록 조회
        let except = config.except_tables.as_deref().unwrap_or(&[]);
        let tables = db
            .get_tables(schema, except)
            .await
            .with_context(|| format!("테이블 목록 조회 실패: {schema}"))?;

        tracing::info!("{} Table Count : {}", schema, tables.len());
        tracing::info!("{} Table Column/Index/Const Load", schema);

        // 테이블별 메타데이터 수집 — `buffered(N)`으로 최대 N개까지 동시 조회.
        // `buffered`는 입력 순서를 보존하므로 결과 `Vec`의 인덱스 i는 입력
        // `tables[i]`에 대응한다 (Req 9.4). 출력 파일의 바이트 시퀀스가
        // 직렬 버전과 동일해진다 (Req 9.5).
        let schema_arc: Arc<str> = Arc::from(schema.as_str());
        let tables = buffer_metadata(tables, METADATA_CONCURRENCY, |table| {
            let db = Arc::clone(&db);
            let schema = Arc::clone(&schema_arc);
            enrich_table(db, schema, table, output_format)
        })
        .await;

        // 수집된 메타데이터의 누적 메모리 추정 표시
        let schema_bytes: usize = tables.iter().map(TableDef::estimated_size).sum();
        tracing::info!("{} metadata in memory: {}", schema, fmt_bytes(schema_bytes));

        // 테이블 데이터 기록
        exporter
            .write_tables(schema, &tables)
            .with_context(|| format!("테이블 기록 실패: {schema}"))?;
    }

    // 파일 저장/닫기
    exporter.finish().context("Exporter finish 실패")?;

    tracing::info!("Export Complete.");
    Ok(())
}

/// 바이트 수를 사람이 읽기 좋은 단위로 (B / KiB / MiB).
fn fmt_bytes(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= MIB {
        format!("{:.1}MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1}KiB", b / KIB)
    } else {
        format!("{bytes}B")
    }
}
