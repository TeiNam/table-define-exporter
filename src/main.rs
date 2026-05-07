use clap::Parser;
use std::process;

use td_export::{
    config::{self, CliOverrides},
    db::{DbClient, DbClientEnum},
    export::create_exporter,
    model::{DbType, OutputFormat},
};

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
    #[arg(long, value_name = "FORMAT")]
    output: Option<String>,

    /// DB 종류: mysql, postgres
    #[arg(long = "db-type", value_name = "TYPE")]
    db_type: Option<String>,

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
    #[arg(
        long = "except-tables",
        value_name = "PATTERNS",
        value_delimiter = ','
    )]
    except_tables: Option<Vec<String>>,
}

impl Cli {
    /// 파싱된 CLI 인자를 `CliOverrides`로 변환한다.
    /// 잘못된 문자열 값(출력 포맷, DB 종류)은 여기서 검증한다.
    fn into_overrides(self) -> Result<CliOverrides, td_export::error::AppError> {
        let output_format = match self.output {
            Some(v) => Some(OutputFormat::from_str(&v)?),
            None => None,
        };
        let db_type = match self.db_type {
            Some(v) => Some(DbType::from_str(&v)?),
            None => None,
        };
        Ok(CliOverrides {
            output_format,
            db_type,
            endpoint: self.endpoint,
            port: self.port,
            user: self.user,
            database: self.database,
            target_db: self.target_db,
            except_tables: self.except_tables,
        })
    }
}

#[tokio::main]
async fn main() {
    // tracing-subscriber 초기화
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // 앱 이름/버전 로그
    tracing::info!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    // CLI 오버라이드 변환 (잘못된 포맷/DB 종류 문자열 검증)
    let overrides = match cli.into_overrides() {
        Ok(o) => o,
        Err(e) => {
            tracing::error!("{}", e);
            process::exit(1);
        }
    };

    // 대화식 설정 수집 (CLI 오버라이드가 있는 필드는 프롬프트 생략)
    let config = match config::load_config(overrides) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("{}", e);
            process::exit(1);
        }
    };

    let output_format = config.output_format;

    // DB 연결
    let db = match DbClientEnum::connect(&config).await {
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
