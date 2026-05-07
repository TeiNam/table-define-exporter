---
name: postgres-guideline
description: >
  PostgreSQL 16+ 스키마 설계, 테이블/인덱스 생성, 쿼리 최적화, 파티셔닝,
  psycopg3 커넥션 관리에 적용. 트리거: CREATE TABLE, GENERATED ALWAYS AS IDENTITY,
  EXPLAIN ANALYZE, GIN/BRIN/GiST 인덱스, RLS, PARTITION BY RANGE, pg_partman,
  LISTEN/NOTIFY, Advisory Lock, UPSERT ON CONFLICT, CTE, timestamptz 관련 작업.
origin: custom
---

# PostgreSQL Database Guideline

## When to Activate

- Writing SQL queries or migrations
- Designing database schemas
- Troubleshooting slow queries
- Implementing Row Level Security
- Setting up connection pooling
- Creating partitioned tables

## PostgreSQL Version and Defaults
- PostgreSQL 16.7+
- Character set: UTF-8
- Schema separation by purpose (`public` schema direct use discouraged)

```sql
CREATE DATABASE myapp
  ENCODING 'UTF8'
  LC_COLLATE 'en_US.UTF-8'
  LC_CTYPE 'en_US.UTF-8'
  TEMPLATE template0;

CREATE SCHEMA app;    -- application tables
CREATE SCHEMA log;    -- log tables
CREATE SCHEMA ref;    -- reference/master tables
```

## Naming Rules
- Tables: snake_case (e.g. `chat_history`, `user_chat_setting`)
- Columns: snake_case (e.g. `user_id`, `created_at`)
- Indexes: `idx_{table}_{column}` / `uidx_{table}_{column}`
- Sequences: `{table}_{column}_seq` (auto with IDENTITY)
- Constraints: `{table}_{type}_{column}` (e.g. `user_pk_user_id`)

## Data Type Guide

| Use Case | Recommended Type | Notes |
|----------|-----------------|-------|
| Small PK | `int` | ~2.1 billion |
| Large PK | `bigint` | Required for log tables |
| Small integer | `smallint` | -32768 ~ 32767 |
| Boolean | `boolean` | Never use 'Y'/'N' strings |
| Variable string | `varchar(n)` or `text` | Use `text` if no length limit |
| Fixed string | `char(n)` | Fixed-length codes only |
| Timestamp | `timestamptz` | Timezone required |
| Date only | `date` | |
| JSON data | `jsonb` | Not `json` (indexing support) |
| Money | `numeric(p,s)` | Never use float / `numeric(15,2)`: 원화, `numeric(10,2)`: USD, `numeric(5,4)`: 비율(0.1234=12.34%) |
| IP address | `inet` | PostgreSQL native type |
| Arrays | `type[]` | Simple lists (e.g. `text[]`) |
| IDs (external) | `uuid` via `gen_random_uuid()` | |

## Prohibited Items
- Stored Procedures: prohibited
- Triggers: prohibited (handle `updated_at` in application)
- Events/Schedulers: use external (cron, Airflow)
- Complex Views: discouraged, simple read-only only
- RULE: prohibited (unpredictable behavior)
- SERIAL type: use IDENTITY instead

## Reference Files
- `schema-design.md` — PK/FK policy, RLS, checklists
- `index-and-query.md` — Index strategy, query patterns, pagination, queue
- `partitioning.md` — Partitioning strategy, pg_partman, management
- `connection-and-features.md` — psycopg 3, Advisory Lock, LISTEN/NOTIFY, server config
