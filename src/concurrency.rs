//! 메타데이터 병렬 수집 유틸리티.
//!
//! `futures::stream::buffered`를 얇게 감싸서 입력 순서(order-preserving)를
//! 유지한 채 동시에 최대 `concurrency`개의 future를 실행한다.
//!
//! 설계 요지 (code-quality-improvements design.md §Components §4):
//! - Requirements 9.1 (buffered 기반), 9.2 (상한), 9.4 (결정적 순서),
//!   9.5 (바이트 동일성)을 충족한다.
//! - `buffered`는 입력 순서를 보존하므로 직렬 → 병렬 전환 후에도
//!   출력 바이트 시퀀스가 변하지 않는다.

use futures::stream::{self, StreamExt};

/// 입력 순서를 보존하며 최대 `concurrency`개의 future를 동시에 실행한다.
///
/// # 동작
/// - `items`의 각 원소를 `f`로 future에 매핑하고, `buffered(concurrency)`로
///   동시에 풀링한다.
/// - 결과 `Vec`의 `i`번째 원소는 입력 `items[i]`에 대응한다.
///
/// # Parameters
/// - `items`: 처리할 입력 값들
/// - `concurrency`: 동시에 실행할 future 최대 개수 (>= 1 권장,
///   0이 전달되면 `buffered`가 panic하므로 호출부에서 보장해야 함)
/// - `f`: 각 입력 값을 future로 변환하는 클로저. future는 `'static`이어야 한다.
///
/// # Returns
/// 입력과 동일한 순서의 결과 `Vec<T>`.
pub async fn buffer_metadata<T, F, Fut>(items: Vec<T>, concurrency: usize, f: F) -> Vec<T>
where
    F: FnMut(T) -> Fut,
    Fut: std::future::Future<Output = T>,
{
    stream::iter(items)
        .map(f)
        .buffered(concurrency)
        .collect()
        .await
}
