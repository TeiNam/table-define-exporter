# Partitioning Strategy

## Log Tables: Monthly Declarative Partitioning

```sql
CREATE TABLE log.chat_history (
  chat_history_id bigint GENERATED ALWAYS AS IDENTITY,
  conversation_id char(18) NOT NULL,
  user_id int NOT NULL,
  user_message text NOT NULL,
  bot_response text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
) PARTITION BY RANGE (created_at);

CREATE TABLE log.chat_history_2024_01 PARTITION OF log.chat_history
  FOR VALUES FROM ('2024-01-01') TO ('2024-02-01');
CREATE TABLE log.chat_history_2024_02 PARTITION OF log.chat_history
  FOR VALUES FROM ('2024-02-01') TO ('2024-03-01');

-- Default partition (catches out-of-range data)
CREATE TABLE log.chat_history_default PARTITION OF log.chat_history DEFAULT;

-- Indexes auto-inherited by partitions
CREATE INDEX idx_chat_history_user_id ON log.chat_history (user_id);
CREATE INDEX idx_chat_history_created_at ON log.chat_history (created_at);
```

## Tables That Should Be Partitioned
- `chat_history`: chat logs
- `conversation_session`: conversation sessions (optional)
- `audit_log`: audit logs
- `access_log`: access logs

## pg_partman (Recommended)

```sql
CREATE EXTENSION pg_partman;

SELECT partman.create_parent(
  p_parent_table := 'log.chat_history',
  p_control := 'created_at',
  p_type := 'native',
  p_interval := 'monthly',
  p_premake := 3
);

-- Run periodically via cron
SELECT partman.run_maintenance();
```

## Partition Management

> ⚠️ **주의:** `DEFAULT` 파티션이 존재할 때 새 파티션을 직접 추가하면 에러 발생.
> Default 파티션에 해당 범위 데이터가 이미 존재하면 constraint 위반으로 실패한다.
> 반드시 아래 순서로 진행할 것.

```sql
-- ✅ 올바른 순서: DEFAULT 파티션이 있는 경우
-- 1. default 파티션 detach
ALTER TABLE log.chat_history DETACH PARTITION log.chat_history_default;

-- 2. 새 월 파티션 생성
CREATE TABLE log.chat_history_2024_05 PARTITION OF log.chat_history
  FOR VALUES FROM ('2024-05-01') TO ('2024-06-01');

-- 3. default 파티션 re-attach
ALTER TABLE log.chat_history ATTACH PARTITION log.chat_history_default DEFAULT;

-- ❌ 잘못된 방법: default 파티션에 2024-05 데이터가 있으면 아래 구문은 에러
-- CREATE TABLE log.chat_history_2024_05 PARTITION OF log.chat_history
--   FOR VALUES FROM ('2024-05-01') TO ('2024-06-01');
-- ERROR: updated partition constraint for default partition "chat_history_default" would be violated

-- Detach old partition (preserves data, faster than DROP)
ALTER TABLE log.chat_history DETACH PARTITION log.chat_history_2024_01;

-- Drop detached partition
DROP TABLE log.chat_history_2024_01;

-- Or move to archive schema
ALTER TABLE log.chat_history_2024_01 SET SCHEMA archive;
```

> ※ pg_partman 사용 시 위 과정이 자동화됨 — 수동 운영 시에만 해당.

## Partition Info Query

```sql
SELECT
  c.relname AS partition_name,
  pg_size_pretty(pg_total_relation_size(c.oid)) AS total_size,
  pg_stat_get_live_tuples(c.oid) AS row_count
FROM pg_inherits i
JOIN pg_class c ON c.oid = i.inhrelid
JOIN pg_class p ON p.oid = i.inhparent
WHERE p.relname = 'chat_history'
ORDER BY c.relname;
```

## Partition Pruning

Always include partition key in WHERE clause:

```python
def get_monthly_chat_history(user_id: int, year: int, month: int):
    start_date = f"{year}-{month:02d}-01"
    end_date = f"{year}-{month + 1:02d}-01" if month < 12 else f"{year + 1}-01-01"

    return db.execute_query("""
        SELECT chat_history_id, conversation_id, user_message, bot_response, created_at
        FROM log.chat_history
        WHERE user_id = %(user_id)s
          AND created_at >= %(start_date)s::timestamptz
          AND created_at < %(end_date)s::timestamptz
        ORDER BY created_at DESC
    """, {"user_id": user_id, "start_date": start_date, "end_date": end_date})
```
