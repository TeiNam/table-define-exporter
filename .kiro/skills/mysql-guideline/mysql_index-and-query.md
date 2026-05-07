# Index Strategy and Query Patterns

## Index Types

| Index Type | Use Case | Example |
|-----------|----------|---------|
| B-tree (default) | Equality, range | `CREATE INDEX idx ON t (col)` |
| Composite | Multi-column WHERE | `CREATE INDEX idx ON t (a, b)` |
| Unique | Duplicate prevention | `CREATE UNIQUE INDEX uidx ON t (col)` |
| Fulltext (ngram) | Text search | `CREATE FULLTEXT INDEX ftx ON t (col) WITH PARSER ngram` |
| Prefix | Long varchar columns | `CREATE INDEX idx ON t (col(20))` |

## Key Index Patterns

```sql
-- Composite: equality first, then range
CREATE INDEX idx_chat_history_user_date ON chat_history (user_id, created_at);

-- Unique index
CREATE UNIQUE INDEX uidx_user_email ON user (email);

-- Fulltext with ngram parser (Korean/CJK support)
CREATE FULLTEXT INDEX ftx_small_talk_search
ON small_talk (eng_sentence, kor_sentence) WITH PARSER ngram;

-- Covering index: WHERE(status) → ORDER BY(created_at) → SELECT 추가 컬럼(user_id, total_amount)
-- 조회 전용 컬럼은 뒤에 배치하여 index-only scan 유도
CREATE INDEX idx_orders_status_covering ON orders (status, created_at, user_id, total_amount);
```

## Query Patterns

### Parameterized Queries (Required)

```python
db.execute_raw_query(
    "SELECT user_id, email, is_active, created_at FROM user WHERE user_id = %(user_id)s",
    {"user_id": user_id}
)

db.select("user", columns=["user_id", "email"], where={"is_active": "Y"})
```

### UPSERT (INSERT ... ON DUPLICATE KEY)

```sql
INSERT INTO user_setting (user_id, setting_key, setting_value, updated_at)
VALUES (%(user_id)s, %(key)s, %(value)s, NOW())
ON DUPLICATE KEY UPDATE
  setting_value = VALUES(setting_value),
  updated_at = NOW();
```

### Batch Insert

```python
db.execute_raw_query("""
    INSERT INTO chat_history (user_id, conversation_id, user_message, bot_response)
    VALUES
    (%(u1)s, %(c1)s, %(m1)s, %(r1)s),
    (%(u2)s, %(c2)s, %(m2)s, %(r2)s)
""", params)
```

### EXPLAIN for Query Analysis

```sql
-- EXPLAIN 분석 시에는 SELECT * 허용 (실행 계획 확인 목적)
EXPLAIN SELECT * FROM chat_history
WHERE user_id = 1 AND created_at >= '2024-01-01' AND created_at < '2024-02-01';

EXPLAIN FORMAT=JSON SELECT ...;
```

## Query Checklist
- [ ] Parameterized queries used (SQL injection prevention)
- [ ] Partition key included in WHERE for partitioned tables
- [ ] EXPLAIN checked for complex queries
- [ ] Only needed columns selected (avoid `SELECT *`)
- [ ] Batch INSERT for bulk operations
- [ ] No N+1 query patterns
