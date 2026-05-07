# Performance Optimization Rules

## Optimize Only When Needed

- 벤치마크 없이 최적화하지 않음
- `cargo bench` 또는 `criterion`으로 측정 후 최적화
- 불필요한 할당(allocation) 줄이기
- 핫 패스에서 `String` 대신 `&str` 활용

## Build Troubleshooting

빌드 실패 시:
1. 에러 메시지 분석
2. 점진적으로 수정
3. 각 수정 후 검증 (`cargo check` → `cargo build`)

## Approach for Complex Tasks

복잡한 작업에 깊은 사고가 필요할 때:
1. 구조화된 접근 방식 사용
2. 여러 비평 라운드를 통한 철저한 분석
3. 다양한 관점에서 검토
