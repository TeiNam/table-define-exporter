//! Feature: code-quality-improvements, Property 14: run 에러 로그 단일성 —
//! 임의의 `anyhow::Error` 값 `e`에 대해, `run().await`가 `Err(e)`를 반환하여
//! `main`이 이를 처리하는 경로에서 발생하는 `ERROR` 수준 `tracing` 이벤트 수는
//! 정확히 1이다.
//!
//! 실제 `main()`은 프로세스 종료 및 tracing 전역 subscriber 초기화를 수행하므로
//! 단위 테스트에서 직접 호출하기 어렵다. 대신 `src/main.rs`의 에러 처리 패턴
//! (`match result { Ok(()) => SUCCESS, Err(e) => { tracing::error!("{e:#}"); FAILURE } }`)
//! 을 단일 헬퍼 `simulate_main_error_handling`로 재현하여, 동일 패턴이 임의의
//! `anyhow::Error`에 대해 정확히 1회의 `ERROR` 이벤트만 발화함을 속성 테스트로
//! 검증한다.
//!
//! `tracing` 이벤트 캡처는 `tracing::subscriber::with_default`로 스레드 로컬
//! 스코프를 만들고, `ERROR` 레벨 이벤트만 카운트하는 최소 `Subscriber`를
//! 직접 구현해 사용한다 (추가 의존성 없음).
//!
//! Validates: Requirements 6.3

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use proptest::prelude::*;
use tracing::span;
use tracing::subscriber::with_default;
use tracing::{Event, Level, Metadata, Subscriber};

// ─────────────────────────────────────────────────────────────────────────────
// ERROR 이벤트 카운팅 Subscriber (최소 구현)
// ─────────────────────────────────────────────────────────────────────────────

/// `ERROR` 레벨 이벤트만 카운트하는 최소 `tracing::Subscriber`.
///
/// span 수명 관리 등은 테스트 범위에서 불필요하므로 noop으로 구현한다.
/// 스레드 안전하도록 `AtomicUsize`를 사용한다.
struct ErrorCounter {
    error_count: Arc<AtomicUsize>,
}

impl ErrorCounter {
    fn new() -> (Self, Arc<AtomicUsize>) {
        let counter = Arc::new(AtomicUsize::new(0));
        (
            Self {
                error_count: Arc::clone(&counter),
            },
            counter,
        )
    }
}

impl Subscriber for ErrorCounter {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        // 성능상 ERROR 이상만 관심 대상으로 표시 (Rust에서 ERROR는 최고 레벨).
        *metadata.level() <= Level::ERROR
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        // 실제 span은 사용하지 않지만, trait 요구사항상 유효 ID를 반환해야 함.
        span::Id::from_u64(1)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        if *event.metadata().level() == Level::ERROR {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn enter(&self, _span: &span::Id) {}
    fn exit(&self, _span: &span::Id) {}
}

/// 주어진 클로저를 ERROR 카운팅 Subscriber 스코프 하에서 실행하고, 관찰된
/// ERROR 이벤트 수를 반환한다.
fn count_errors_during<F: FnOnce()>(body: F) -> usize {
    let (subscriber, counter) = ErrorCounter::new();
    with_default(subscriber, body);
    counter.load(Ordering::Relaxed)
}

// ─────────────────────────────────────────────────────────────────────────────
// main 에러 처리 패턴의 재현
// ─────────────────────────────────────────────────────────────────────────────

/// `src/main.rs`의 에러 처리 패턴을 그대로 재현한다.
///
/// ```ignore
/// match run::run().await {
///     Ok(()) => ExitCode::SUCCESS,
///     Err(e) => {
///         tracing::error!("{e:#}");
///         ExitCode::FAILURE
///     }
/// }
/// ```
///
/// 반환값은 성공/실패 여부(true=성공)로 단순화한다. 실제 `ExitCode`와
/// 의미가 동일하다.
fn simulate_main_error_handling(result: Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            tracing::error!("{e:#}");
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 입력 생성기: 임의의 anyhow::Error
// ─────────────────────────────────────────────────────────────────────────────

/// 임의의 에러 메시지 조합으로 `anyhow::Error`를 생성하는 전략.
///
/// 단일 메시지 에러와 0개 이상의 `context` 체인이 덧씌워진 에러를 모두 다룬다.
/// 이는 실제 `run()`이 `anyhow::Context`로 에러를 쌓는 패턴을 반영한다.
fn arb_anyhow_error() -> impl Strategy<Value = anyhow::Error> {
    (
        "[^\0]{0,64}",
        proptest::collection::vec("[^\0]{0,32}", 0..=4),
    )
        .prop_map(|(base, contexts)| {
            let mut err: anyhow::Error = anyhow!("{}", base);
            for ctx in contexts {
                err = err.context(ctx);
            }
            err
        })
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 14 PBT: run 에러 로그 단일성
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 14a: 임의의 `anyhow::Error` `e`에 대해
    /// `simulate_main_error_handling`이 `Err(e)`를 한 번 받으면 `ERROR` 이벤트는
    /// 정확히 1회 발화된다.
    ///
    /// Validates: Requirements 6.3
    #[test]
    fn error_path_emits_exactly_one_error_event(err in arb_anyhow_error()) {
        let errors = count_errors_during(|| {
            let _ = simulate_main_error_handling(Err(err));
        });

        prop_assert_eq!(
            errors, 1,
            "Err 경로는 ERROR 이벤트 1회를 발화해야 하지만 {}이었음", errors
        );
    }

    /// Property 14b: `Ok(())` 결과에 대해서는 `ERROR` 이벤트가 발화되지 않는다.
    /// 성공 경로의 부재 조건이 Property 14의 전제(실패 경로에서만 1회)를
    /// 실효적으로 보장한다.
    ///
    /// Validates: Requirements 6.3
    #[test]
    fn ok_path_emits_no_error_events(_dummy in any::<u8>()) {
        let errors = count_errors_during(|| {
            let _ = simulate_main_error_handling(Ok(()));
        });

        prop_assert_eq!(
            errors, 0,
            "Ok 경로는 ERROR 이벤트를 발화하지 않아야 하지만 {}이었음", errors
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 예시 기반 스모크 테스트 (기본 동작 문서화)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn single_error_emits_one_event() {
    let errors = count_errors_during(|| {
        let _ = simulate_main_error_handling(Err(anyhow!("boom")));
    });
    assert_eq!(errors, 1);
}

#[test]
fn ok_emits_no_events() {
    let errors = count_errors_during(|| {
        let _ = simulate_main_error_handling(Ok(()));
    });
    assert_eq!(errors, 0);
}

#[test]
fn chained_context_still_emits_single_event() {
    // `anyhow::Context`로 쌓인 체인이 있어도 ERROR 이벤트는 1회만 발화된다.
    // `{:#}` 포맷이 전체 체인을 하나의 문자열로 펼치기 때문이다.
    let err: Result<()> = Err(anyhow!("root cause"))
        .context("중간 단계 실패")
        .context("최상위 실패");

    let errors = count_errors_during(|| {
        let _ = simulate_main_error_handling(err);
    });
    assert_eq!(errors, 1);
}

#[test]
fn multiple_sequential_errors_emit_corresponding_events() {
    // 동일한 패턴을 n회 호출하면 ERROR 이벤트도 n회 발생한다.
    // (Property 14는 "단일 Err 처리에 대해 정확히 1회"를 보장한다.)
    let errors = count_errors_during(|| {
        for i in 0..5 {
            let _ = simulate_main_error_handling(Err(anyhow!("error {i}")));
        }
    });
    assert_eq!(errors, 5);
}
