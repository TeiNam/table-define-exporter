//! PostgreSQL DDL 재구성에 사용되는 순수 타입과 헬퍼 함수.
//!
//! 이 모듈은 외부 의존성이 없는 값 타입과 순수 함수만 포함하므로
//! 단위 테스트와 속성 기반 테스트(PBT)가 용이하다.
//! 상위 `postgres` 모듈에서 `pub use`로 재노출되어
//! `td_export::db::postgres::PgDdlColumn` 등의 기존 공개 경로를 유지한다.

// ─────────────────────────────────────────────────────────────────────────────
// DDL 재구성용 메타데이터 구조체
// ─────────────────────────────────────────────────────────────────────────────

/// DDL 재구성용 컬럼 메타데이터
#[derive(Debug, Clone)]
pub struct PgDdlColumn {
    /// 컬럼 이름
    pub name: String,
    /// 데이터 타입 (예: "integer", "varchar(255)")
    pub data_type: String,
    /// NULL 허용 여부 (true = NULL 허용)
    pub is_nullable: bool,
    /// 기본값 (예: "0", "'hello'", "nextval('seq'::regclass)")
    pub default_value: Option<String>,
    /// STORED generated 컬럼의 표현식 (예: "col1 + col2")
    pub generated_expression: Option<String>,
}

/// DDL 재구성용 제약 조건 종류
#[derive(Debug, Clone)]
pub enum PgConstraintType {
    /// PRIMARY KEY 제약 조건
    PrimaryKey,
    /// UNIQUE 제약 조건
    Unique,
    /// FOREIGN KEY 제약 조건
    ForeignKey {
        ref_schema: String,
        ref_table: String,
        ref_columns: Vec<String>,
        on_delete: String,
        on_update: String,
    },
    /// CHECK 제약 조건
    Check { expression: String },
}

/// DDL 재구성용 제약 조건 메타데이터
#[derive(Debug, Clone)]
pub struct PgDdlConstraint {
    /// 제약 조건 이름
    pub name: String,
    /// 제약 조건 종류
    pub constraint_type: PgConstraintType,
    /// 로컬 컬럼 목록 (CHECK 제약 조건에서는 비어있을 수 있음)
    pub columns: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 컬럼 타입/extra 결정 헬퍼 (순수 함수)
// ─────────────────────────────────────────────────────────────────────────────

/// PostgreSQL 컬럼 타입 문자열을 구성한다.
///
/// `udt_name`과 길이/정밀도 정보를 조합하여 사람이 읽기 쉬운 타입 문자열을 만든다.
/// - 배열 타입(`_` 접두어):
///   - `_int4` → `int4[]`
///   - `_varchar` + `char_max_length=N` → `varchar(N)[]` (Req 11.1)
///   - `_bpchar` + `char_max_length=N` → `bpchar(N)[]` (Req 11.1)
///   - 그 외 배열(길이/정밀도 없음) → `{base}[]`
/// - 문자 길이 지정: `varchar(255)`, `bpchar` → `char({length})`
/// - 숫자 정밀도/스케일: `numeric(10,2)`
/// - 그 외: `udt_name` 그대로 반환
pub fn build_pg_column_type(
    udt_name: &str,
    char_max_length: Option<i32>,
    numeric_precision: Option<i32>,
    numeric_scale: Option<i32>,
) -> String {
    // 배열 타입: `_` 접두어 제거 후 길이/정밀도를 반영하고 `[]` 접미어 추가
    if let Some(base) = udt_name.strip_prefix('_') {
        // Req 11.1: `_varchar`/`_bpchar` 배열은 character_maximum_length가 있으면 길이를 보존
        if matches!(base, "varchar" | "bpchar") {
            if let Some(length) = char_max_length {
                return format!("{base}({length})[]");
            }
        }

        // Req 11.2: `_numeric` 배열은 precision/scale이 모두 있으면 파라미터 보존
        if base == "numeric" {
            if let (Some(precision), Some(scale)) = (numeric_precision, numeric_scale) {
                return format!("numeric({precision},{scale})[]");
            }
        }

        // 그 외 배열: 파라미터 없이 `{base}[]`
        return format!("{base}[]");
    }

    // 문자 길이가 지정된 경우: `{type}({length})`
    if let Some(length) = char_max_length {
        // `bpchar`는 PostgreSQL 내부 이름이므로 `char`로 표시
        let display_name = if udt_name == "bpchar" {
            "char"
        } else {
            udt_name
        };
        return format!("{display_name}({length})");
    }

    // numeric 타입에 정밀도/스케일이 모두 지정된 경우: `numeric({p},{s})`
    if udt_name == "numeric" {
        if let (Some(precision), Some(scale)) = (numeric_precision, numeric_scale) {
            return format!("numeric({precision},{scale})");
        }
    }

    // 그 외: udt_name 그대로 반환
    udt_name.to_string()
}

/// PostgreSQL 컬럼의 extra 정보를 결정한다.
///
/// 우선순위:
/// 1. `attidentity`가 `'a'`(ALWAYS) 또는 `'d'`(BY DEFAULT) → `auto_increment`
/// 2. `column_default`에 `nextval(` 포함 (serial/bigserial) → `auto_increment`
/// 3. `attgenerated`가 `'s'`(STORED) → `STORED GENERATED`
/// 4. 그 외 → `None`
pub fn determine_pg_extra(
    attidentity: &str,
    attgenerated: &str,
    column_default: Option<&str>,
) -> Option<String> {
    // 1. identity 컬럼 감지 (ALWAYS 또는 BY DEFAULT)
    if attidentity == "a" || attidentity == "d" {
        return Some("auto_increment".to_string());
    }

    // 2. serial/bigserial 감지 (nextval 패턴)
    if let Some(default) = column_default {
        if default.contains("nextval(") {
            return Some("auto_increment".to_string());
        }
    }

    // 3. generated 컬럼 감지 (STORED만 지원, PG 13~17)
    if attgenerated == "s" {
        return Some("STORED GENERATED".to_string());
    }

    // 4. 해당 없음
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_pg_column_type: 배열 타입 길이 반영 (Req 11.1) ──────────────────

    #[test]
    fn varchar_array_with_length_includes_size() {
        // `_varchar` + character_maximum_length → `varchar(N)[]`
        assert_eq!(
            build_pg_column_type("_varchar", Some(255), None, None),
            "varchar(255)[]"
        );
    }

    #[test]
    fn bpchar_array_with_length_includes_size() {
        // `_bpchar` + character_maximum_length → `bpchar(N)[]`
        assert_eq!(
            build_pg_column_type("_bpchar", Some(10), None, None),
            "bpchar(10)[]"
        );
    }

    #[test]
    fn varchar_array_without_length_falls_back() {
        // 길이 없으면 기존 동작 유지
        assert_eq!(
            build_pg_column_type("_varchar", None, None, None),
            "varchar[]"
        );
    }

    #[test]
    fn bpchar_array_without_length_falls_back() {
        assert_eq!(
            build_pg_column_type("_bpchar", None, None, None),
            "bpchar[]"
        );
    }

    #[test]
    fn other_array_types_are_unaffected() {
        // 다른 배열 타입은 길이 반영 대상이 아님 (Req 11.1 범위 외)
        assert_eq!(build_pg_column_type("_int4", None, None, None), "int4[]");
        assert_eq!(
            build_pg_column_type("_text", Some(100), None, None),
            "text[]"
        );
    }

    // ── 비배열 경로 회귀 방지 ────────────────────────────────────────────────

    #[test]
    fn scalar_varchar_with_length_unchanged() {
        assert_eq!(
            build_pg_column_type("varchar", Some(255), None, None),
            "varchar(255)"
        );
    }

    #[test]
    fn scalar_bpchar_display_name_remains_char() {
        // 스칼라 bpchar는 표시상 `char(N)`으로 유지
        assert_eq!(
            build_pg_column_type("bpchar", Some(10), None, None),
            "char(10)"
        );
    }

    #[test]
    fn scalar_numeric_with_precision_and_scale_unchanged() {
        assert_eq!(
            build_pg_column_type("numeric", None, Some(10), Some(2)),
            "numeric(10,2)"
        );
    }
}
