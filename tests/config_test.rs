// config 모듈 테스트
// Property 18: DbType별 기본 포트 (Default Port by DbType)

use proptest::prelude::*;
use td_export::model::DbType;

// parse_port는 pub(crate)이므로 통합 테스트에서 직접 호출할 수 없다.
// 대신 DbType::default_port()를 검증하고, parse_port의 동작은
// src/config.rs 내부 #[cfg(test)] 모듈에서 검증한다.
// 여기서는 DbType별 기본 포트 값의 정확성을 속성 기반으로 검증한다.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // Property 18: DbType별 기본 포트 정확성
    // For any DbType 변형에 대해, default_port()는 올바른 기본 포트를 반환해야 한다.
    // - DbType::MySql → 3306
    // - DbType::Postgres → 5432
    //
    // **Validates: Requirements 2.1, 2.3, 2.4**
    #[test]
    fn default_port_matches_db_type(
        db_type in prop_oneof![
            Just(DbType::MySql),
            Just(DbType::Postgres),
        ],
    ) {
        let port = db_type.default_port();
        match db_type {
            DbType::MySql => prop_assert_eq!(port, 3306, "MySQL 기본 포트는 3306이어야 함"),
            DbType::Postgres => prop_assert_eq!(port, 5432, "Postgres 기본 포트는 5432이어야 함"),
        }
    }

    // Property 18: DbType별 기본 포트 — from_str 후 default_port 일관성
    // 유효한 db-type 문자열로 파싱한 DbType의 default_port()가 올바른 값을 반환해야 한다.
    //
    // **Validates: Requirements 2.1, 2.3, 2.4**
    #[test]
    fn default_port_consistent_after_from_str(
        input in prop_oneof![
            Just("mysql".to_string()),
            Just("postgres".to_string()),
            Just("postgresql".to_string()),
            Just("MYSQL".to_string()),
            Just("POSTGRES".to_string()),
            Just("PostgreSQL".to_string()),
        ],
    ) {
        let db_type = DbType::from_str(&input).unwrap();
        let port = db_type.default_port();
        match db_type {
            DbType::MySql => prop_assert_eq!(port, 3306),
            DbType::Postgres => prop_assert_eq!(port, 5432),
        }
    }
}
