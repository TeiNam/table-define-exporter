# Security Rules (Rust CLI/Library)

## Required Security Checks

- [ ] No hardcoded secrets (API keys, passwords, tokens)
- [ ] All user input validated and sanitized
- [ ] 파일 경로 검증 (path traversal 방지)
- [ ] Error handling on critical paths
- [ ] No sensitive data exposed in error messages
- [ ] `unsafe` 블록 최소화 및 문서화

## Secret Management

- Never hardcode secrets in source code
- Always use environment variables (`std::env::var`)
- Validate required secrets exist at startup
- `.env` 파일은 `.gitignore`에 포함

## Rust-Specific Security

1. **`unsafe` 사용** — 반드시 안전성 증명 주석 포함, 최소 범위로 제한
2. **의존성 취약점** — `cargo audit` 정기 실행
3. **입력 파싱** — 신뢰할 수 없는 입력에 대해 크기 제한 설정
4. **파일 I/O** — 심볼릭 링크 공격, path traversal 주의
5. **정수 오버플로** — `checked_*` 또는 `saturating_*` 메서드 활용

## Code Patterns to Flag

| Pattern | Severity | Fix |
|---------|----------|-----|
| Hardcoded secrets | CRITICAL | `std::env::var()` 사용 |
| `unsafe` without comment | HIGH | 안전성 증명 주석 추가 |
| `unwrap()` on user input | HIGH | `?` 또는 적절한 에러 처리 |
| Unbounded input reading | MEDIUM | 크기 제한 설정 |
| Path from user without validation | HIGH | canonicalize + 경로 검증 |

## Core Security Principles

1. **Rust 타입 시스템 활용** — 컴파일 타임에 최대한 검증
2. **Least Privilege** — 최소 권한만 부여
3. **Fail Securely** — 에러가 데이터를 노출하지 않도록
4. **Distrust Input** — 모든 외부 입력 검증
5. **Regular Updates** — `cargo update` + `cargo audit`
