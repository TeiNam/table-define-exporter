# Coding Style Rules (Rust)

## Ownership & Immutability

Rust의 소유권 시스템을 최대한 활용:
- 기본적으로 불변 바인딩 사용 (`let` 우선, `mut`은 필요할 때만)
- `Clone` 남용 금지 — 참조(`&`, `&mut`)로 해결 가능한지 먼저 확인
- 불필요한 `Arc<Mutex<>>` 사용 지양 — 단일 스레드에서는 `Rc<RefCell<>>` 또는 소유권 이동으로 해결

## File Organization

작은 파일 여러 개 > 큰 파일 소수:
- 높은 응집도, 낮은 결합도
- 모듈당 200–400줄, 최대 800줄
- 큰 모듈은 하위 모듈로 분리
- 기능/도메인 기준으로 구성

## Functions

- 작고, 집중적이며, 의미 있는 이름
- 함수당 50줄 이하
- 비자명한 코드에만 주석 작성

## Error Handling

Rust 에러 처리 패턴 준수:
- `unwrap()` / `expect()` 는 테스트 코드에서만 사용
- 프로덕션 코드에서는 `?` 연산자와 `Result<T, E>` 사용
- 커스텀 에러 타입은 `thiserror` 또는 `anyhow` 활용
- `panic!`은 복구 불가능한 상황에서만 사용

## Input Validation

시스템 경계에서 항상 검증:
- 모든 외부 입력(파일, CLI 인자, 환경변수)을 처리 전 검증
- 타입 시스템을 활용한 컴파일 타임 검증 우선
- 실패 시 명확한 에러 메시지 제공
- 외부 데이터를 절대 신뢰하지 않음

## Rust-Specific Best Practices

- `clippy` 경고를 모두 해결
- `#[must_use]` 적절히 활용
- `derive` 매크로 활용 (Debug, Clone, PartialEq 등)
- 공개 API에는 문서 주석(`///`) 필수
- `pub` 범위를 최소화 (`pub(crate)`, `pub(super)` 활용)

## Code Quality Checklist

작업 완료 전 확인:
- [ ] 코드가 읽기 쉽고 이름이 잘 선택됨
- [ ] 함수가 작음 (<50줄)
- [ ] 파일이 집중적 (<800줄)
- [ ] 깊은 중첩 없음 (>4단계)
- [ ] 적절한 에러 처리 (`unwrap()` 없음)
- [ ] 하드코딩된 값 없음 (상수 또는 설정 사용)
- [ ] 불필요한 `mut` 없음
- [ ] 주석 처리된 코드, `println!` 디버그 출력 없음
- [ ] `cargo clippy` 경고 없음
- [ ] `cargo fmt` 적용됨
