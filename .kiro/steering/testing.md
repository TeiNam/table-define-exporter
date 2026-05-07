# Testing Rules (Rust)

## Minimum Test Coverage: 70% on Critical Paths

Test types:
1. **Unit Tests** — 핵심 비즈니스 로직, 개별 함수, 유틸리티 (`#[cfg(test)]` 모듈)
2. **Integration Tests** — `tests/` 디렉토리에서 공개 API 테스트

## Running Tests

- **Never use `cd` to change directories before running tests** — always use the `cwd` parameter in executeBash instead
- `cargo test` 로 전체 테스트 실행
- `cargo test -- --nocapture` 로 출력 확인
- 특정 테스트: `cargo test test_name`

## TDD Workflow (Required)

1. 테스트 먼저 작성 (RED)
2. 테스트 실행 — 반드시 실패해야 함
3. 최소 구현 작성 (GREEN)
4. 테스트 실행 — 반드시 통과해야 함
5. 리팩토링 (IMPROVE)
6. 커버리지 확인 (70%+)

## Required Edge Cases to Test

1. **빈 입력** — 빈 문자열, 빈 Vec
2. **경계값** — min/max, 0, usize::MAX
3. **에러 경로** — 파일 없음, 파싱 실패, 잘못된 형식
4. **유니코드** — 특수 문자, 이모지, 멀티바이트
5. **대용량 데이터** — 성능 저하 없는지 확인

## Test Anti-Patterns (Avoid)

- 구현 세부사항 테스트 (내부 상태) — 동작을 테스트할 것
- 테스트 간 의존성 (공유 상태)
- 너무 적은 assertion (아무것도 검증하지 않는 통과 테스트)
- 외부 의존성 미모킹

## Test Best Practices

1. **테스트 먼저 작성** — Always TDD
2. **테스트당 하나의 assertion** — 단일 동작에 집중
3. **서술적 테스트 이름** — `test_parse_empty_input_returns_error`
4. **Arrange-Act-Assert** — 명확한 테스트 구조
5. **`#[should_panic]`** — 패닉 예상 테스트에 활용
6. **`proptest` / `quickcheck`** — 속성 기반 테스트 고려
7. **테스트 빠르게 유지** — 단위 테스트는 각 50ms 이하
