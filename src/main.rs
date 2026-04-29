use clap::Parser;
use std::process;

use td_export::{config, db::DbClient, export::create_exporter, model::OutputFormat};

/// Table Definition Export - MySQL 테이블 정의서 내보내기 도구
#[derive(Parser)]
#[command(
    name = "td-export",
    version,
    about = "Table Definition Export - MySQL 테이블 정의서를 Excel/Markdown/SQL로 내보냅니다"
)]
struct Cli {
    /// 출력 포맷: excel, markdown, sql
    #[arg(long, default_value = "excel")]
    output: String,
}

#[tokio::main]
async fn main() {
    // tracing-subscriber 초기화
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // 앱 이름/버전 로그
    tracing::info!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    // 출력 포맷 파싱
    let output_format = match OutputFormat::from_str(&cli.output) {
        Ok(fmt) => fmt,
        Err(e) => {
            tracing::error!("{}", e);
            process::exit(1);
        }
    };

    // 대화식 설정 수집
    let config = match config::load_config(output_format) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("{}", e);
            process::exit(1);
        }
    };

    // DB 연결
    let db = match DbClient::connect(&config).await {
        Ok(client) => {
            tracing::info!("DB Connect Success");
            client
        }
        Err(e) => {
            tracing::error!("{}", e);
            process::exit(1);
        }
    };

    // 스키마 목록 조회
    let catalog = match db.get_schemas(&config).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("{}", e);
            process::exit(1);
        }
    };

    if catalog.is_empty() {
        tracing::info!("Not in Schema.");
        process::exit(1);
    }

    // Exporter 초기화
    let mut exporter = create_exporter(output_format);
    if let Err(e) = exporter.setup(&catalog, &config) {
        tracing::error!("{}", e);
        process::exit(1);
    }
    tracing::info!("Setup {} Files", output_format.display_name());

    tracing::info!("Get Schema Count : {}", catalog.len());

    // 스키마별 루프
    let mut schema_names: Vec<String> = catalog.keys().cloned().collect();
    schema_names.sort();

    for schema in &schema_names {
        tracing::info!("{} Table Load.", schema);

        // 테이블 목록 조회
        let except = config.except_tables.as_deref().unwrap_or(&[]);
        let mut tables = match db.get_tables(schema, except).await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("{}", e);
                process::exit(1);
            }
        };

        tracing::info!("{} Table Count : {}", schema, tables.len());
        tracing::info!("{} Table Column/Index/Const Load", schema);

        // 테이블별 메타데이터 수집
        for table in &mut tables {
            match output_format {
                OutputFormat::Excel | OutputFormat::Markdown => {
                    if table.general.table_type == "BASE TABLE" {
                        // 컬럼 조회
                        match db.get_columns(schema, &table.table_name).await {
                            Ok(cols) => table.columns = cols,
                            Err(e) => {
                                tracing::error!("{} - {}: {}", schema, table.table_name, e);
                                continue;
                            }
                        }
                        // 인덱스 조회
                        match db.get_indexes(schema, &table.table_name).await {
                            Ok(idxs) => table.indexes = idxs,
                            Err(e) => {
                                tracing::error!("{} - {}: {}", schema, table.table_name, e);
                                continue;
                            }
                        }
                        // 제약 조건 조회
                        match db.get_constraints(schema, &table.table_name).await {
                            Ok(cons) => table.constraints = cons,
                            Err(e) => {
                                tracing::error!("{} - {}: {}", schema, table.table_name, e);
                                continue;
                            }
                        }
                    } else if table.general.table_type == "VIEW" {
                        // 뷰 정의 조회
                        match db.get_view_info(schema, &table.table_name).await {
                            Ok(view) => table.view = Some(view),
                            Err(e) => {
                                tracing::error!("{} - {}: {}", schema, table.table_name, e);
                                continue;
                            }
                        }
                    }
                }
                OutputFormat::Sql => {
                    // DDL 조회
                    match db.get_table_ddl(schema, &table.table_name).await {
                        Ok(ddl) => table.ddl = Some(ddl),
                        Err(e) => {
                            tracing::error!("{} - {}: {}", schema, table.table_name, e);
                            continue;
                        }
                    }
                }
            }
        }

        // 테이블 데이터 기록
        if let Err(e) = exporter.write_tables(schema, &tables) {
            tracing::error!("{}", e);
            process::exit(1);
        }
    }

    // 파일 저장/닫기
    if let Err(e) = exporter.finish() {
        tracing::error!("{}", e);
        process::exit(1);
    }

    tracing::info!("Export Complete.");
}
