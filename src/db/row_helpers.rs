//! DB Row 메타데이터 조회 헬퍼
//!
//! sqlx `Row`에서 컬럼을 읽을 때 실패 시 경고 로그를 남기고 타입의 기본값으로
//! 대체하는 공용 헬퍼를 제공한다. 현재는 각 DB 모듈에서
//! `row.try_get(...).unwrap_or_default()` 형태의 패턴이 분산되어 있어
//! 컬럼 누락을 조용히 삼켜버리는 문제가 있었다
//! (Requirements 5.1, 5.2 참조).
//!
//! # 동작 요약
//!
//! - 성공 시 값을 그대로 반환한다.
//! - 실패 시 `tracing::warn!`으로 스키마/테이블/컬럼 이름을 포함한 경고를
//!   남기고 `T::default()`를 반환한다.
//! - 동일 `(schema, table, column)` 조합에 대해서는 실행당 한 번만 로그를
//!   남겨 반복 출력을 방지한다 (Requirement 5.4).

use std::collections::HashSet;
use std::fmt::Display;
use std::sync::{Mutex, OnceLock};

use sqlx::{ColumnIndex, Decode, Row, Type};

/// 이미 경고를 출력한 `(schema|table|column)` 키 집합.
///
/// 전역 `OnceLock`으로 초기화되며, `Mutex`로 멀티스레드(특히 병렬
/// 메타데이터 수집 경로)에서의 경쟁 상태를 방지한다. 정상 경로(성공)는
/// lock을 잡지 않으므로 추가 비용이 없다.
static LOGGED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// `(schema, table, column)` 조합에 대해 실행당 정확히 한 번만 `WARN` 이벤트를
/// 발화한다.
///
/// 이 함수는 `try_get_or_warn`의 내부 헬퍼이지만, dedup + warn 동작을
/// 독립적으로 테스트하기 위해 공개한다. 실제 sqlx `Row` 없이도
/// Property 8(로그 dedup)을 검증할 수 있게 하는 seam 역할을 한다.
///
/// # 인자
///
/// - `schema`, `table`, `column`: 경고 로그 컨텍스트이자 dedup 키의 구성요소
/// - `error`: 원인이 되는 에러 표현. `Display`로 포맷되어 `error` 필드에 기록됨
///
/// # 동작
///
/// 동일 `(schema, table, column)` 조합에 대해 최초 호출에서만 `WARN` 이벤트를
/// 발화하고, 이후 호출은 조용히 리턴한다. 전역 `LOGGED` 집합은 프로세스 종료
/// 시점까지 유지된다.
pub fn warn_missing_column_once(schema: &str, table: &str, column: &str, error: &dyn Display) {
    let key = format!("{schema}|{table}|{column}");
    let set = LOGGED.get_or_init(|| Mutex::new(HashSet::new()));
    // `expect` 대신 poison 복구 전략을 사용한다. dedup 집합은 로그 제어용
    // 보조 상태이므로 다른 스레드에서 패닉한 경우에도 계속 경고를 발화하는
    // 편이 "조용히 실패"하는 것보다 낫다. 이렇게 하면 `unwrap`/`expect`가
    // 프로덕션 코드에 남지 않는다 (coding-style 가이드 준수).
    let mut guard = set.lock().unwrap_or_else(|poison| poison.into_inner());
    if guard.insert(key) {
        tracing::warn!(
            schema,
            table,
            column,
            error = %error,
            "메타데이터 컬럼 읽기 실패 — 기본값 사용"
        );
    }
}

/// sqlx `Row`에서 `column`을 읽고, 실패 시 경고 후 기본값을 반환한다.
///
/// # 인자
///
/// - `row`: sqlx 쿼리 결과 행 참조 (MySQL `MySqlRow`, PostgreSQL `PgRow` 등)
/// - `column`: 읽을 컬럼 이름
/// - `schema`, `table`: 로그 컨텍스트로 사용되는 스키마/테이블 이름
///
/// # 반환
///
/// 성공 시 컬럼 값, 실패 시 `T::default()`.
///
/// # 로그
///
/// 컬럼 읽기 실패가 특정 `(schema, table, column)` 조합에서 처음 발생한
/// 경우에만 `WARN` 레벨로 출력한다. 이후 호출에서는 같은 조합에 대해
/// 조용히 기본값만 반환한다.
pub fn try_get_or_warn<'r, R, T>(row: &'r R, column: &str, schema: &str, table: &str) -> T
where
    R: Row,
    T: Default + Decode<'r, R::Database> + Type<R::Database>,
    for<'c> &'c str: ColumnIndex<R>,
{
    match row.try_get::<T, _>(column) {
        Ok(v) => v,
        Err(e) => {
            warn_missing_column_once(schema, table, column, &e);
            T::default()
        }
    }
}
