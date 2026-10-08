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
    /// generated 컬럼이면 종류와 표현식
    pub generated: Option<PgGenerated>,
    /// identity 컬럼이면 종류와 시퀀스 시작값/증가값
    pub identity: Option<PgIdentity>,
}

/// identity 컬럼 (`pg_attribute.attidentity`)
#[derive(Debug, Clone, PartialEq)]
pub struct PgIdentity {
    /// `'a'` → `ALWAYS`, `'d'` → `BY DEFAULT`
    pub always: bool,
    /// 시퀀스 `START WITH`
    pub start: i64,
    /// 시퀀스 `INCREMENT BY`
    pub increment: i64,
}

impl PgIdentity {
    /// `attidentity` 코드와 시퀀스 값으로 생성한다. identity 컬럼이 아니면 `None`.
    pub fn from_catalog(
        attidentity: &str,
        start: Option<i64>,
        increment: Option<i64>,
    ) -> Option<Self> {
        let always = match attidentity {
            "a" => true,
            "d" => false,
            _ => return None,
        };
        Some(Self {
            always,
            start: start.unwrap_or(1),
            increment: increment.unwrap_or(1),
        })
    }

    /// 컬럼 정의 뒤에 붙는 ` GENERATED {ALWAYS|BY DEFAULT} AS IDENTITY [(START WITH n INCREMENT BY m)]`.
    // ponytail: START WITH/INCREMENT BY 만 보존 — MINVALUE/MAXVALUE/CACHE/CYCLE 은 생략.
    // 바꿔 쓰는 스키마가 생기면 pg_sequence 의 나머지 컬럼도 같은 방식으로 추가.
    pub fn to_sql(&self) -> String {
        let kind = if self.always { "ALWAYS" } else { "BY DEFAULT" };
        let options = if (self.start, self.increment) == (1, 1) {
            String::new()
        } else {
            format!(
                " (START WITH {} INCREMENT BY {})",
                self.start, self.increment
            )
        };
        format!(" GENERATED {kind} AS IDENTITY{options}")
    }
}

/// generated 컬럼 종류 (`pg_attribute.attgenerated`)
#[derive(Debug, Clone)]
pub enum PgGenerated {
    /// `'s'` — `GENERATED ALWAYS AS (expr) STORED`
    Stored(String),
    /// `'v'` — `GENERATED ALWAYS AS (expr) VIRTUAL` (PG 18+, STORED 생략 시 기본값)
    Virtual(String),
}

impl PgGenerated {
    /// `attgenerated` 코드와 표현식으로 생성한다. generated 컬럼이 아니면 `None`.
    pub fn from_catalog(attgenerated: &str, expression: Option<String>) -> Option<Self> {
        match (attgenerated, expression) {
            ("s", Some(expr)) => Some(Self::Stored(expr)),
            ("v", Some(expr)) => Some(Self::Virtual(expr)),
            _ => None,
        }
    }
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
        /// `MATCH FULL` / `MATCH PARTIAL` 의 FULL·PARTIAL (기본값 SIMPLE 은 None)
        match_type: Option<String>,
        /// `DEFERRABLE` / `DEFERRABLE INITIALLY DEFERRED` (기본값 NOT DEFERRABLE 은 None)
        deferrable: Option<String>,
    },
    /// CHECK 제약 조건
    Check { expression: String },
    /// EXCLUDE 제약 조건 — `pg_get_constraintdef` 원문 (예: `EXCLUDE USING gist (during WITH &&)`)
    Exclude { definition: String },
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

/// PostgreSQL 컬럼의 extra 정보를 결정한다.
///
/// 우선순위:
/// 1. `attidentity`가 `'a'`(ALWAYS) 또는 `'d'`(BY DEFAULT) → `auto_increment`
/// 2. `column_default`에 `nextval(` 포함 (serial/bigserial) → `auto_increment`
/// 3. `attgenerated`가 `'s'`(STORED) → `STORED GENERATED`, `'v'`(VIRTUAL, PG 18+) → `VIRTUAL GENERATED`
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
    if let Some(default) = column_default
        && default.contains("nextval(")
    {
        return Some("auto_increment".to_string());
    }

    // 3. generated 컬럼 감지 (MySQL extra 표기와 동일하게 맞춘다)
    match attgenerated {
        "s" => return Some("STORED GENERATED".to_string()),
        "v" => return Some("VIRTUAL GENERATED".to_string()),
        _ => {}
    }

    // 4. 해당 없음
    None
}
