use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("잘못된 출력 포맷: {0}")]
    InvalidOutputFormat(String),

    #[error("필수 입력 누락: {0}")]
    MissingInput(String),

    #[error("잘못된 포트 번호: {0}")]
    InvalidPort(String),

    #[error("DB 연결 실패 ({endpoint}:{port}): {source}")]
    DbConnection {
        endpoint: String,
        port: u16,
        #[source]
        source: sqlx::Error,
    },

    #[error("메타데이터 조회 실패 ({schema}.{table}): {source}")]
    MetadataQuery {
        schema: String,
        table: String,
        #[source]
        source: sqlx::Error,
    },

    #[error("스키마를 찾을 수 없음")]
    NoSchemas,

    #[error("안전하지 않은 식별자: {0}")]
    UnsafeIdentifier(String),

    #[error("파일 쓰기 실패: {source}")]
    FileWrite {
        #[source]
        source: std::io::Error,
    },

    #[error("Excel 생성 실패: {0}")]
    ExcelWrite(String),

    #[error("입력 읽기 실패: {source}")]
    InputRead {
        #[source]
        source: std::io::Error,
    },
}
