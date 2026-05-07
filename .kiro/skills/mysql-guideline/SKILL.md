---
name: mysql-guideline
description: >
  MySQL 8.0+ 스키마 설계, 테이블/인덱스 생성, 쿼리 최적화, 파티셔닝,
  커넥션 관리에 적용. 트리거: CREATE TABLE, ALTER TABLE, slow query 분석,
  index 설계, RANGE partition, MySQL migration, utf8mb4, InnoDB,
  트랜잭션 관리, UPSERT, Covering Index, 복합 인덱스 관련 작업.
origin: custom
---

# MySQL Database Guideline

## When to Activate

- Writing MySQL queries or migrations
- Designing MySQL database schemas
- Troubleshooting slow queries
- Creating partitioned tables
- Setting up connection management

## MySQL Version and Defaults
- MySQL 8.0.40+
- Character set: utf8mb4, utf8mb4_general_ci
- Engine: InnoDB

## Naming Rules
- Tables: snake_case (e.g. `chat_history`, `user_chat_setting`)
- Columns: snake_case (e.g. `user_id`, `created_at`, `updated_at`)
- Indexes: `idx_{table}_{column}`
- Unique: `uidx_{table}_{column}`
- Fulltext: `ftx_{table}_{column}`

## Data Type Guide

| Use Case | Recommended Type | Notes |
|----------|-----------------|-------|
| Tiny PK/flag | `tinyint unsigned` | 0~255 |
| Small PK | `smallint unsigned` | 0~65535 |
| Standard PK | `int unsigned` | 0~4.2 billion |
| Large PK | `bigint unsigned` | Log tables |
| Boolean | `tinyint(1)` | 0/1 |
| Variable string | `varchar(n)` | Specify max length |
| Long text | `text` | No length limit |
| Fixed string | `char(n)` | Fixed-length codes |
| Timestamp | `datetime` | With DEFAULT CURRENT_TIMESTAMP |
| JSON data | `json` | MySQL 8.0+ native JSON |
| Money | `decimal(p,s)` | Never use float / `decimal(15,2)`: 원화, `decimal(10,2)`: USD, `decimal(5,4)`: 비율(0.1234=12.34%) |

## Prohibited Items
- Stored Procedures: prohibited
- Triggers: prohibited
- Events: prohibited
- Complex Views: discouraged, simple read-only only

## Reference Files
- `schema-design.md` — PK/FK policy, checklists
- `index-and-query.md` — Index strategy, query patterns
- `partitioning.md` — Partitioning strategy, management
- `connection-and-features.md` — Connection management, transactions
