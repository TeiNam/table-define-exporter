//! td-export CLI 진입점.
//!
//! 비즈니스 로직은 `run` 모듈에 있으며, `main`은 tracing 초기화와
//! `ExitCode` 반환만 담당한다. 에러는 단일 `tracing::error!` 지점에서 기록된다.

use std::process::ExitCode;
use tracing_subscriber::{EnvFilter, fmt};

mod run;

#[tokio::main]
async fn main() -> ExitCode {
    // tracing-subscriber 초기화.
    // RUST_LOG 환경변수가 설정되어 있으면 그 값을 사용하고,
    // 없거나 파싱에 실패하면 기본값 "info"로 폴백한다.
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match run::run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // `{:#}` 포맷은 anyhow의 에러 체인(source 포함)을 한 줄로 펼쳐 출력한다.
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}
