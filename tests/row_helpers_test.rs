//! Feature: code-quality-improvements, Property 8: try_get_or_warn 로그 dedup —
//! 임의의 `(schema, table, column)` 키와 양의 정수 `n`에 대해, 동일 키로
//! 실패하는 호출을 `n`회 반복할 때 발화되는 `WARN` 이벤트 수는 정확히 1이다
//! (실행당 1회).
//!
//! 실제 sqlx `Row`를 주입해 실패를 유도하기는 번거로우므로, `try_get_or_warn`의
//! 내부 헬퍼 `warn_missing_column_once`(dedup + `tracing::warn!` 발화만 담당)를
//! 직접 호출하여 속성을 검증한다. 실제 `try_get_or_warn`는 실패 분기에서
//! 이 헬퍼를 호출하므로, 헬퍼의 dedup 의미가 `try_get_or_warn` 전체의 dedup
//! 의미를 보증한다.
//!
//! 전역 `LOGGED` 집합이 프로세스 수명으로 유지되는 점을 고려해, 각 테스트
//! 케이스는 `AtomicU64` 카운터로 생성한 **유일한 prefix**를 `(schema, table,
//! column)`에 부여한다. 이로써 테스트 간 / 케이스 간 키 충돌을 제거한다.
//!
//! `tracing` 이벤트 캡처는 `tracing::subscriber::with_default`로 스레드 로컬
//! 스코프를 만들고, `WARN` 레벨 이벤트만 카운트하는 최소 `Subscriber`를
//! 직접 구현해 사용한다 (추가 의존성 도입 없음).
//!
//! Validates: Requirements 5.4

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use proptest::prelude::*;
use tracing::span;
use tracing::subscriber::with_default;
use tracing::{Event, Level, Metadata, Subscriber};

use td_export::db::row_helpers::warn_missing_column_once;

// ─────────────────────────────────────────────────────────────────────────────
// 유일한 prefix 발급: 테스트 간·케이스 간 LOGGED 키 충돌 방지
// ─────────────────────────────────────────────────────────────────────────────

/// 테스트 파일 단위 전역 카운터. 프로세스에서 단조 증가한다.
static UNIQ: AtomicU64 = AtomicU64::new(0);

/// 후속 호출에서 절대 겹치지 않는 새 prefix를 반환한다.
fn fresh_prefix() -> u64 {
    UNIQ.fetch_add(1, Ordering::Relaxed)
}

// ─────────────────────────────────────────────────────────────────────────────
// WARN 이벤트 카운팅 Subscriber (최소 구현)
// ─────────────────────────────────────────────────────────────────────────────

/// `WARN` 레벨 이벤트만 카운트하는 최소 `tracing::Subscriber`.
///
/// span 수명 관리 등은 테스트 범위에서 불필요하므로 noop으로 구현한다.
/// 스레드 안전하도록 `AtomicUsize`를 사용한다.
struct WarnCounter {
    warn_count: Arc<AtomicUsize>,
}

impl WarnCounter {
    fn new() -> (Self, Arc<AtomicUsize>) {
        let counter = Arc::new(AtomicUsize::new(0));
        (
            Self {
                warn_count: Arc::clone(&counter),
            },
            counter,
        )
    }
}

impl Subscriber for WarnCounter {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        // 성능상 WARN 이상만 관심 대상으로 표시. 하위 레벨은 필터링.
        *metadata.level() <= Level::WARN
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        // 실제 span은 사용하지 않지만, trait 요구사항상 유효 ID를 반환해야 함.
        span::Id::from_u64(1)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        if *event.metadata().level() == Level::WARN {
            self.warn_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn enter(&self, _span: &span::Id) {}
    fn exit(&self, _span: &span::Id) {}
}

/// 주어진 클로저를 WARN 카운팅 Subscriber 스코프 하에서 실행하고, 관찰된
/// WARN 이벤트 수를 반환한다.
fn count_warns_during<F: FnOnce()>(body: F) -> usize {
    let (subscriber, counter) = WarnCounter::new();
    with_default(subscriber, body);
    counter.load(Ordering::Relaxed)
}

// ─────────────────────────────────────────────────────────────────────────────
// 입력 생성기: 식별자 유사 문자열 + 반복 횟수
// ─────────────────────────────────────────────────────────────────────────────

/// 비교적 평범한 문자열 생성기. prefix와 조합되어 유일성을 보장한다.
fn name_fragment() -> impl Strategy<Value = String> {
    "[a-zA-Z_][a-zA-Z0-9_]{0,16}".prop_map(|s| s.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Property 8 PBT: 로그 dedup
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 8a: 동일한 `(schema, table, column)` 키로 실패 경로를 `n`회
    /// 반복해도 `WARN` 이벤트는 정확히 1회만 발화된다.
    ///
    /// Validates: Requirements 5.4
    #[test]
    fn dedup_emits_exactly_one_warn_for_same_key(
        schema_frag in name_fragment(),
        table_frag in name_fragment(),
        column_frag in name_fragment(),
        n in 1usize..=20,
    ) {
        // prefix로 전역 LOGGED 집합과의 키 충돌을 제거한다.
        let prefix = fresh_prefix();
        let schema = format!("s{prefix}_{schema_frag}");
        let table = format!("t{prefix}_{table_frag}");
        let column = format!("c{prefix}_{column_frag}");

        let warns = count_warns_during(|| {
            for _ in 0..n {
                warn_missing_column_once(&schema, &table, &column, &"synthetic error");
            }
        });

        prop_assert_eq!(
            warns, 1,
            "n={}회 호출 후 WARN 이벤트는 1이어야 하지만 {}이었음", n, warns
        );
    }

    /// Property 8b: 서로 다른 `k`개의 키 각각에 대해 여러 번 호출하면 전체
    /// `WARN` 이벤트 수는 정확히 `k`이다 (키별 독립적 dedup).
    ///
    /// Validates: Requirements 5.4
    #[test]
    fn dedup_is_per_key(
        fragments in proptest::collection::vec(
            (name_fragment(), name_fragment(), name_fragment()),
            1..=8,
        ),
        repeats in 1usize..=10,
    ) {
        let prefix = fresh_prefix();
        // 입력 내부 중복을 제거해 "서로 다른 k개의 키" 가정을 만족시킨다.
        let mut keys: Vec<(String, String, String)> = fragments
            .into_iter()
            .enumerate()
            .map(|(i, (s, t, c))| {
                (
                    format!("s{prefix}_{i}_{s}"),
                    format!("t{prefix}_{i}_{t}"),
                    format!("c{prefix}_{i}_{c}"),
                )
            })
            .collect();
        keys.sort();
        keys.dedup();
        let k = keys.len();

        let warns = count_warns_during(|| {
            for _ in 0..repeats {
                for (s, t, c) in &keys {
                    warn_missing_column_once(s, t, c, &"synthetic error");
                }
            }
        });

        prop_assert_eq!(
            warns, k,
            "키 {}개를 {}회씩 호출 후 WARN은 {}이어야 하지만 {}이었음",
            k, repeats, k, warns
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 예시 기반 스모크 테스트 (기본 동작 문서화)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn single_call_emits_exactly_one_warn() {
    let prefix = fresh_prefix();
    let warns = count_warns_during(|| {
        warn_missing_column_once(
            &format!("s{prefix}_public"),
            &format!("t{prefix}_users"),
            &format!("c{prefix}_email"),
            &"column not found",
        );
    });
    assert_eq!(warns, 1);
}

#[test]
fn repeated_calls_with_same_key_emit_single_warn() {
    let prefix = fresh_prefix();
    let schema = format!("s{prefix}_public");
    let table = format!("t{prefix}_orders");
    let column = format!("c{prefix}_total");

    let warns = count_warns_during(|| {
        for _ in 0..5 {
            warn_missing_column_once(&schema, &table, &column, &"column not found");
        }
    });
    assert_eq!(warns, 1);
}

#[test]
fn different_columns_on_same_table_emit_separately() {
    let prefix = fresh_prefix();
    let schema = format!("s{prefix}_public");
    let table = format!("t{prefix}_products");

    let warns = count_warns_during(|| {
        warn_missing_column_once(&schema, &table, &format!("c{prefix}_name"), &"err");
        warn_missing_column_once(&schema, &table, &format!("c{prefix}_price"), &"err");
        warn_missing_column_once(&schema, &table, &format!("c{prefix}_name"), &"err");
    });
    assert_eq!(warns, 2);
}
