//! Feature: code-quality-improvements, Property 7: 병렬 메타데이터 수집 순서 보존 —
//! 임의의 입력 `items: Vec<T>`와 동시성 상한 `n: usize`에 대해,
//! `buffer_metadata(items, n, f)`의 결과 `Vec`의 인덱스 `i`는 원본 `items[i]`에
//! 대응하는 결과이다 (입력 순서 == 출력 순서).
//!
//! `buffer_metadata`는 `futures::stream::buffered`를 얇게 감싼 비동기 함수다.
//! `buffered`는 입력 순서를 보존하므로 동시에 여러 future가 out-of-order로
//! 완료되더라도 결과 순서가 입력과 일치해야 한다. 본 테스트는 두 측면에서
//! 이를 검증한다:
//!
//! - **7a (identity future)**: 지연이 없는 항등 future로 순서가 단순히 보존됨을
//!   검증한다. 동시성 상한 `n`과 입력 길이의 조합을 폭넓게 샘플링한다.
//! - **7b (delayed future)**: 각 원소에 서로 다른 작은 지연을 주입하여 완료
//!   순서가 입력 순서와 일반적으로 달라지도록 강제한 뒤에도, 결과 `Vec`의
//!   순서가 여전히 입력 순서와 일치하는지 확인한다. 뒤쪽 원소에 더 짧은 지연을
//!   부여하면 먼저 완료되지만 결과 인덱스는 입력 기준으로 유지되어야 한다.
//!
//! proptest는 비동기 테스트를 직접 지원하지 않으므로 `tokio::runtime::Runtime`을
//! 각 케이스마다 생성해 `block_on`으로 실행한다. 지연 값과 입력 길이는 테스트
//! 시간이 길어지지 않도록 작은 범위로 제한한다.
//!
//! Validates: Requirements 9.4, 9.5

use std::time::Duration;

use proptest::prelude::*;
use tokio::runtime::Runtime;

use td_export::concurrency::buffer_metadata;

// ─────────────────────────────────────────────────────────────────────────────
// 헬퍼: proptest(동기) 안에서 async 블록 실행
// ─────────────────────────────────────────────────────────────────────────────

/// 현재 스레드에서 새로운 `tokio` 런타임을 만들어 future를 완료까지 실행한다.
///
/// proptest는 동기 테스트 러너이므로 케이스마다 런타임을 생성해 격리한다.
/// `tokio::time::sleep`를 사용하기 위해 `time` feature가 필요하며, 이는
/// `Cargo.toml`의 `[dev-dependencies]` 블록에 명시되어 있다.
fn run_async<F: std::future::Future>(fut: F) -> F::Output {
    Runtime::new()
        .expect("tokio 런타임 생성 실패")
        .block_on(fut)
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 7 PBT: 순서 보존
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 7a: 항등 future를 사용할 때 `buffer_metadata`의 결과는
    /// 입력과 바이트 단위로 동일하다.
    ///
    /// Validates: Requirements 9.4, 9.5
    #[test]
    fn identity_future_preserves_order(
        items in proptest::collection::vec(any::<u32>(), 0..=32),
        n in 1usize..=8,
    ) {
        let expected = items.clone();
        let result = run_async(async move {
            buffer_metadata(items, n, |x| async move { x }).await
        });
        prop_assert_eq!(result, expected);
    }

    /// Property 7b: 원소별로 서로 다른 지연이 주입되어 완료 순서가 입력
    /// 순서와 어긋나더라도, 결과 `Vec`은 입력 순서를 그대로 보존한다.
    ///
    /// 각 원소는 `(index, delay_ms)` 쌍으로 표현한다. 지연은 0..=10ms의 작은
    /// 범위로 제한해 테스트 시간을 짧게 유지한다. future는 지연 후 원본
    /// 튜플을 그대로 반환하므로, 결과의 `index` 필드가 `0..len` 순서이면
    /// 입력 순서가 보존된 것이다.
    ///
    /// Validates: Requirements 9.4, 9.5
    #[test]
    fn delayed_future_preserves_order(
        delays in proptest::collection::vec(0u64..=10, 1..=10),
        n in 1usize..=8,
    ) {
        // (index, delay_ms) 쌍 — index는 입력 순서를 기록한다.
        let items: Vec<(usize, u64)> =
            delays.iter().enumerate().map(|(i, &d)| (i, d)).collect();
        let expected = items.clone();

        let result = run_async(async move {
            buffer_metadata(items, n, |(i, d)| async move {
                tokio::time::sleep(Duration::from_millis(d)).await;
                (i, d)
            })
            .await
        });

        // 결과 인덱스가 입력 순서(0..len)와 일치하는지 먼저 단언하여 진단
        // 가능성을 높인 뒤, 전체 값까지 동일한지 검증한다.
        let indices: Vec<usize> = result.iter().map(|(i, _)| *i).collect();
        let expected_indices: Vec<usize> = (0..expected.len()).collect();
        prop_assert_eq!(indices, expected_indices);
        prop_assert_eq!(result, expected);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 예시 기반 스모크 테스트 (기본 동작 문서화)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn empty_input_returns_empty_vec() {
    let result: Vec<u32> =
        run_async(async { buffer_metadata(Vec::<u32>::new(), 4, |x| async move { x }).await });
    assert!(result.is_empty());
}

#[test]
fn single_element_preserves_value() {
    let result: Vec<u32> =
        run_async(async { buffer_metadata(vec![42u32], 4, |x| async move { x + 1 }).await });
    assert_eq!(result, vec![43]);
}

#[test]
fn later_elements_finishing_first_still_preserve_input_order() {
    // 뒤쪽 원소일수록 지연이 짧아 먼저 완료되지만, 결과 순서는 입력 기준으로
    // 유지되어야 한다.
    let items = vec![(0usize, 8u64), (1, 6), (2, 4), (3, 2), (4, 0)];
    let expected = items.clone();
    let result = run_async(async move {
        buffer_metadata(items, 5, |(i, d)| async move {
            tokio::time::sleep(Duration::from_millis(d)).await;
            (i, d)
        })
        .await
    });
    assert_eq!(result, expected);
}

#[test]
fn concurrency_of_one_behaves_serially_and_preserves_order() {
    let items: Vec<u32> = (0u32..10).collect();
    let expected = items.clone();
    let result = run_async(async move { buffer_metadata(items, 1, |x| async move { x }).await });
    assert_eq!(result, expected);
}
