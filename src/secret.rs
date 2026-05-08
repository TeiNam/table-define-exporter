//! 비밀값(secret) 래퍼 모듈.
//!
//! 비밀번호 등 민감한 문자열이 로그·에러 메시지에 평문으로 노출되는 것을 컴파일
//! 타임에 차단하기 위한 타입을 제공한다. [`Password`]는 내부 문자열을 은닉하며
//! `Debug`·`Display` 포맷팅 시 자동으로 `[REDACTED]`로 마스킹된다. 원문이 필요한
//! 경로(DB 연결 옵션 빌더 등)에서는 명시적으로 [`Password::expose`]를 호출해야
//! 한다.
//!
//! # 설계 의도
//!
//! - `String`을 직접 구조체 필드로 사용하면 `format!("{cfg:?}")`처럼 우발적인
//!   매크로 전개 경로에서 비밀번호가 유출될 수 있다.
//! - `Password` 타입은 `Debug`/`Display`를 수동 구현해 내부 문자열을 절대
//!   출력하지 않는다.
//! - 값에 접근하려면 반드시 [`Password::expose`]를 명시적으로 호출해야 하므로,
//!   로그에 흘러갈 수 있는 지점을 코드 리뷰에서 쉽게 찾을 수 있다.

/// 비밀번호를 보관하는 래퍼 타입.
///
/// `Debug`·`Display` 출력 시 원문이 노출되지 않으며, 내부 문자열에 접근하려면
/// [`Password::expose`]를 호출해야 한다.
///
/// # Examples
///
/// ```
/// use td_export::secret::Password;
///
/// let pw = Password::new("example".to_string());
/// assert_eq!(format!("{pw:?}"), "Password([REDACTED])");
/// assert_eq!(format!("{pw}"), "[REDACTED]");
/// assert_eq!(pw.expose(), "example");
/// ```
pub struct Password(String);

impl Password {
    /// 주어진 문자열을 감싼 [`Password`]를 생성한다.
    pub fn new(s: String) -> Self {
        Self(s)
    }

    /// 내부 비밀번호 문자열을 참조로 반환한다.
    ///
    /// 반환된 `&str`을 로그·에러 메시지에 전달하면 비밀번호가 노출될 수 있으므로
    /// 반드시 DB 연결 옵션 빌더 등 의도된 경로에서만 호출해야 한다.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password([REDACTED])")
    }
}

impl std::fmt::Display for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl Clone for Password {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_masks_inner_value() {
        let pw = Password::new("some-value".to_string());
        let debug_output = format!("{pw:?}");
        assert_eq!(debug_output, "Password([REDACTED])");
        assert!(!debug_output.contains("some-value"));
    }

    #[test]
    fn display_masks_inner_value() {
        let pw = Password::new("some-value".to_string());
        let display_output = format!("{pw}");
        assert_eq!(display_output, "[REDACTED]");
        assert!(!display_output.contains("some-value"));
    }

    #[test]
    fn expose_returns_original_value() {
        // URL 특수문자(@, :, /, %, 공백)가 포함되어도 원문 그대로 반환해야 한다.
        let raw = "p@ss w/o rd:!".to_string();
        let pw = Password::new(raw.clone());
        assert_eq!(pw.expose(), raw);
    }

    #[test]
    fn clone_preserves_inner_value() {
        let pw = Password::new("orig".to_string());
        let cloned = pw.clone();
        assert_eq!(pw.expose(), cloned.expose());
    }

    #[test]
    fn empty_password_is_still_masked() {
        // 빈 문자열은 서브스트링 검사 시 예외이지만, 마스킹 자체는 정상 동작해야 한다.
        let pw = Password::new(String::new());
        assert_eq!(format!("{pw:?}"), "Password([REDACTED])");
        assert_eq!(format!("{pw}"), "[REDACTED]");
        assert_eq!(pw.expose(), "");
    }
}
