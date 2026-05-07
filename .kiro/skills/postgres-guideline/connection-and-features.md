# Connection Management and PostgreSQL Features

## psycopg 3 Connection Pool

```python
from psycopg_pool import ConnectionPool
from psycopg.rows import dict_row

pool = ConnectionPool(
    conninfo="host=localhost port=5432 dbname=myapp user=app",
    min_size=4, max_size=10,
    kwargs={"row_factory": dict_row, "autocommit": False}
)
```

## Transaction Management

```python
# with block = auto transaction (commit on success, rollback on exception)
with pool.connection() as conn:
    with conn.transaction():
        with conn.cursor() as cur:
            cur.execute("UPDATE account SET balance = balance - 100 WHERE id = 1")
            cur.execute("UPDATE account SET balance = balance + 100 WHERE id = 2")
```

## Async Support

```python
from psycopg_pool import AsyncConnectionPool

async_pool = AsyncConnectionPool(
    conninfo="host=localhost port=5432 dbname=myapp user=app",
    min_size=4, max_size=10,
    kwargs={"row_factory": dict_row}
)

async def get_user(user_id: int):
    async with async_pool.connection() as conn:
        async with conn.cursor() as cur:
            await cur.execute(
                "SELECT user_id, email, is_active, created_at FROM app.user WHERE user_id = %(user_id)s",
                {"user_id": user_id}
            )
            return await cur.fetchone()
```

## Advisory Lock

```python
# Session-level (pg_advisory_lock): 커넥션 반환 시 자동 해제
# Transaction-level (pg_advisory_xact_lock): 트랜잭션 종료 시 자동 해제 → 권장

# ✅ Transaction-level: with conn.transaction() 종료 시 lock 자동 해제, finally 불필요
async with async_pool.connection() as conn:
    async with conn.transaction():
        async with conn.cursor() as cur:
            await cur.execute("SELECT pg_advisory_xact_lock(%(id)s)", {"id": job_id})
            result = await cur.fetchone()
            # lock 획득 실패 시 대기 (non-blocking 필요하면 pg_try_advisory_xact_lock 사용)
            await cur.execute(
                "UPDATE app.job SET status = 'processing' WHERE job_id = %(id)s",
                {"id": job_id}
            )
        # 트랜잭션 커밋 + lock 해제가 원자적으로 처리됨

# ✅ Non-blocking (lock 획득 실패 시 즉시 반환)
async with async_pool.connection() as conn:
    async with conn.transaction():
        async with conn.cursor() as cur:
            await cur.execute("SELECT pg_try_advisory_xact_lock(%(id)s)", {"id": job_id})
            result = await cur.fetchone()
            if not result["pg_try_advisory_xact_lock"]:
                return  # 다른 프로세스가 처리 중
            await cur.execute(
                "UPDATE app.job SET status = 'processing' WHERE job_id = %(id)s",
                {"id": job_id}
            )

# ❌ 잘못된 패턴: session-level lock + 수동 finally 해제
# commit 후 크래시 발생 시 finally의 unlock이 실행되지 않아 lock leak 위험
# async with conn.cursor() as cur:
#     await cur.execute("SELECT pg_try_advisory_lock(...)")
#     try:
#         await conn.commit()
#     finally:
#         await cur.execute("SELECT pg_advisory_unlock(...)")  # ← 위험
```

## LISTEN/NOTIFY

```python
import psycopg

def notify(conn, channel: str, payload: str):
    conn.execute(f"NOTIFY {channel}, %(payload)s", {"payload": payload})
    conn.commit()

def listen(conninfo: str, channel: str):
    with psycopg.connect(conninfo, autocommit=True) as conn:
        conn.execute(f"LISTEN {channel}")
        for notify in conn.notifies():
            print(f"Received: {notify.payload}")
```

## Server Configuration Template

```sql
ALTER SYSTEM SET max_connections = 100;
ALTER SYSTEM SET work_mem = '8MB';
ALTER SYSTEM SET idle_in_transaction_session_timeout = '30s';
ALTER SYSTEM SET statement_timeout = '30s';
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
REVOKE ALL ON SCHEMA public FROM public;
SELECT pg_reload_conf();
```

## Performance Checklist
- [ ] Connection pooling configured (psycopg_pool or PgBouncer)
- [ ] Transaction scope minimized
- [ ] Partial indexes used where applicable
- [ ] autovacuum status verified
- [ ] `work_mem`, `maintenance_work_mem` tuned
