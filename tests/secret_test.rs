//! Feature: code-quality-improvements, Property 1: Password 문자열 표현 마스킹 —
//! 임의의 문자열 `s`에 대해 `Password::new(s)`의 `Debug`/`Display` 출력은 원본
//! 문자열 `s`를 서브스트링으로 포함하지 않아야 한다. 동시에 `Password::expose`는
//! 원문을 그대로 반환해야 한다 (라운드트립).
//!
//! 빈 문자열은 트리비얼하게 모든 출력의 서브스트링이므로 생성기에서 제외한다.
//! 또한 원본 문자열이 마스킹 템플릿(`Password([REDACTED])`, `[REDACTED]`)의
//! 서브스트링인 경우에는 속성이 트리비얼하게 깨지므로 같이 제외한다. 이는
//! 의미 있는(비밀번호가 실제로 숨겨지는) 입력 공간만을 대상으로 속성을
//! 검증하기 위함이다.

use proptest::prelude::*;
use td_export::secret::Password;

// Debug·Display가 출력하는 마스킹 문자열. 원본이 이들의 서브스트링이 되는
// 케이스는 "원문이 출력에 포함된다"는 trivial 반례를 유발하므로 입력에서
// 제외한다.
const MASK_DEBUG: &str = "Password([REDACTED])";
const MASK_DISPLAY: &str = "[REDACTED]";

/// 임의 문자열 중 비어 있지 않고 마스킹 템플릿의 서브스트링이 아닌 것을
/// 생성하는 전략. 길이는 1–200 사이로 제한한다.
fn non_empty_string_disjoint_from_mask() -> impl Strategy<Value = String> {
    ".{1,200}".prop_filter("non-empty and not a substring of the mask template", |s| {
        !MASK_DEBUG.contains(s.as_str()) && !MASK_DISPLAY.contains(s.as_str())
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 1a: Debug 출력에 원문 미포함.
    /// Validates: Requirements 1.3, 1.4, 1.5
    #[test]
    fn debug_output_does_not_contain_original(s in non_empty_string_disjoint_from_mask()) {
        let pw = Password::new(s.clone());
        let debug = format!("{pw:?}");
        prop_assert!(
            !debug.contains(&s),
            "Debug 출력 '{debug}'에 원문 '{s}'가 포함됨"
        );
        // 마스킹 템플릿 자체는 반드시 유지되어야 한다.
        prop_assert_eq!(debug, MASK_DEBUG);
    }

    /// Property 1b: Display 출력에 원문 미포함.
    /// Validates: Requirements 1.3, 1.4, 1.5
    #[test]
    fn display_output_does_not_contain_original(s in non_empty_string_disjoint_from_mask()) {
        let pw = Password::new(s.clone());
        let display = format!("{pw}");
        prop_assert!(
            !display.contains(&s),
            "Display 출력 '{display}'에 원문 '{s}'가 포함됨"
        );
        prop_assert_eq!(display, MASK_DISPLAY);
    }

    /// Property 1c: expose 라운드트립.
    /// Validates: Requirements 1.3, 1.4, 1.5
    #[test]
    fn expose_returns_original(s in ".{0,200}") {
        let pw = Password::new(s.clone());
        prop_assert_eq!(pw.expose(), s.as_str());
    }
}
