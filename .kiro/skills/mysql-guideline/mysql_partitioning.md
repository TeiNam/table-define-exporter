# Partitioning Strategy

## Log Tables: Monthly RANGE Partitioning

```sql
CREATE TABLE `chat_history` (
  `chat_history_id` int unsigned NOT NULL AUTO_INCREMENT,
  `conversation_id` char(18) NOT NULL,
  `user_id` tinyint unsigned NOT NULL,
  `user_message` text NOT NULL,
  `bot_response` text NOT NULL,
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`chat_history_id`, `created_at`),
  KEY `idx_chat_history_conversation_id` (`conversation_id`),
  KEY `idx_chat_history_user_id` (`user_id`)
) ENGINE=InnoDB
PARTITION BY RANGE (YEAR(created_at) * 100 + MONTH(created_at)) (
  PARTITION p202401 VALUES LESS THAN (202402),
  PARTITION p202402 VALUES LESS THAN (202403),
  PARTITION p202403 VALUES LESS THAN (202404),
  PARTITION p_future VALUES LESS THAN MAXVALUE
);
```

Note: PK must include partition key (`created_at`) for MySQL partitioned tables.

## Tables That Should Be Partitioned
- `chat_history`: chat logs
- `conversation_session`: conversation sessions (optional)
- `audit_log`: audit logs
- `access_log`: access logs

## Partition Management

> ⚠️ **주의:** `p_future (MAXVALUE)` 파티션이 존재하는 경우 `ADD PARTITION`은 에러 발생.
> 반드시 `REORGANIZE PARTITION`으로 `p_future`를 분할해야 한다.

```sql
-- ✅ 올바른 방법: p_future를 새 월 파티션 + p_future로 재분할
ALTER TABLE chat_history REORGANIZE PARTITION p_future INTO (
  PARTITION p202405 VALUES LESS THAN (202406),
  PARTITION p_future VALUES LESS THAN MAXVALUE
);

-- ❌ 잘못된 방법: p_future가 있으면 아래 구문은 ERROR 발생
-- ALTER TABLE chat_history ADD PARTITION (
--   PARTITION p202405 VALUES LESS THAN (202406)
-- );
-- ERROR 1481: MAXVALUE can only be used in last partition definition

-- p_future 없이 운영하는 경우에만 ADD PARTITION 사용 가능
-- ALTER TABLE chat_history ADD PARTITION (
--   PARTITION p202405 VALUES LESS THAN (202406)
-- );
```

```sql
-- Drop old partition (per data retention policy)
ALTER TABLE chat_history DROP PARTITION p202401;
```

## Partition Info Query

```sql
SELECT PARTITION_NAME, PARTITION_DESCRIPTION, TABLE_ROWS, DATA_LENGTH
FROM INFORMATION_SCHEMA.PARTITIONS
WHERE TABLE_NAME = 'chat_history'
AND PARTITION_NAME IS NOT NULL;
```

## Partition Pruning

Always include partition key in WHERE clause:

```python
def get_monthly_chat_history(user_id: int, year: int, month: int):
    start_date = f"{year}-{month:02d}-01"
    end_date = f"{year}-{month + 1:02d}-01" if month < 12 else f"{year + 1}-01-01"

    return db.execute_raw_query("""
        SELECT * FROM chat_history
        WHERE user_id = %(user_id)s
        AND created_at >= %(start_date)s
        AND created_at < %(end_date)s
        ORDER BY created_at DESC
    """, {"user_id": user_id, "start_date": start_date, "end_date": end_date})
```

## Verify Partition Pruning

```sql
EXPLAIN SELECT * FROM chat_history
WHERE created_at >= '2024-03-01' AND created_at < '2024-04-01';
-- Check "partitions" column shows only p202403
```
