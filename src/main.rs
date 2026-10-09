//! td-export CLI 진입점.
//!
//! 비즈니스 로직은 `run` 모듈에 있으며, `main`은 tracing 초기화와
//! `ExitCode` 반환만 담당한다. 에러는 단일 `tracing::error!` 지점에서 기록된다.

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracing_subscriber::{
    EnvFilter, Layer, filter::LevelFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

mod run;

/// 실행 중 기록된 WARN 수. 일부 객체를 건너뛴 채 "완료"로만 보이지 않도록 마지막에 알린다.
pub(crate) static WARNINGS: AtomicUsize = AtomicUsize::new(0);

/// WARN 이벤트를 세는 tracing 레이어
struct WarnCounter;

impl<S: tracing::Subscriber> Layer<S> for WarnCounter {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if *event.metadata().level() == tracing::Level::WARN {
            WARNINGS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    // tracing-subscriber 초기화.
    // RUST_LOG 환경변수가 설정되어 있으면 그 값을 사용하고,
    // 없거나 파싱에 실패하면 기본값 "info"로 폴백한다.
    //
    // RUST_LOG 는 화면 출력에만 건다 — 전역 필터로 걸면 RUST_LOG=error 일 때 WARN 이 집계 전에
    // 버려져 --strict 가 경고를 놓친다.
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(env_filter))
        .with(WarnCounter.with_filter(LevelFilter::WARN))
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
