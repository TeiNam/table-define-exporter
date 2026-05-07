# Schema Design

## Primary Key Policy
- Use `GENERATED ALWAYS AS IDENTITY` (not SERIAL)
- UUID allowed for distributed or external-facing IDs

```sql
CREATE TABLE app.user (
  user_id int GENERATED ALWAYS AS IDENTITY,
  email varchar(255) NOT NULL,
  is_active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),  -- 트리거 없이 애플리케이션에서 직접 갱신
  CONSTRAINT user_pk_user_id PRIMARY KEY (user_id)
);

-- External-facing ID with UUID
CREATE TABLE app.user (
  user_id int GENERATED ALWAYS AS IDENTITY,
  public_id uuid NOT NULL DEFAULT gen_random_uuid(),
  email varchar(255) NOT NULL,
  is_active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT user_pk_user_id PRIMARY KEY (user_id),
  CONSTRAINT uidx_user_public_id UNIQUE (public_id)
);
```

## Foreign Key Policy
- Logical FK only (no physical FK constraints)
- Referential integrity managed at application level
- Avoids lock contention and performance degradation

```sql
CREATE TABLE app.chat_history (
  chat_history_id bigint GENERATED ALWAYS AS IDENTITY,
  user_id int NOT NULL,              -- logical FK: app.user.user_id
  conversation_id char(18) NOT NULL, -- logical FK: app.conversation_session
  user_message text NOT NULL,
  bot_response text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT chat_history_pk PRIMARY KEY (chat_history_id)
);

COMMENT ON COLUMN app.chat_history.user_id IS 'logical FK: app.user.user_id';
```

### Application-Level Referential Integrity

```python
async def create_chat_history(user_id: int, conversation_id: str, message: str, response: str):
    user = await db.execute_query(
        "SELECT user_id FROM app.user WHERE user_id = %(user_id)s AND is_active = true",
        {"user_id": user_id}
    )
    if not user:
        raise ValueError("User does not exist")

    result = await db.execute_command(
        """INSERT INTO log.chat_history (user_id, conversation_id, user_message, bot_response)
           VALUES (%(user_id)s, %(cid)s, %(msg)s, %(resp)s)
           RETURNING chat_history_id""",
        {"user_id": user_id, "cid": conversation_id, "msg": message, "resp": response}
    )
    return result
```

## Soft Delete Pattern

논리 삭제가 필요한 테이블은 `is_active` 컬럼으로 표준화한다.

```sql
`is_active` boolean NOT NULL DEFAULT true
```

- 물리 DELETE 금지 (감사 추적, 복구 가능성 확보)
- 조회 시 항상 `WHERE is_active = true` 포함
- Partial Index로 활성 레코드만 인덱싱 → 인덱스 크기 절감

```sql
-- Partial index: 활성 유저만 인덱싱 (삭제된 유저 제외)
CREATE INDEX idx_user_active_email ON app.user (email) WHERE is_active = true;
```

> ⚠️ `is_active` 단독 B-tree 인덱스는 카디널리티가 낮아 효과 없음.
> PostgreSQL의 Partial Index 또는 복합 인덱스로 사용할 것.

## Row Level Security (RLS)

```sql
ALTER TABLE app.orders ENABLE ROW LEVEL SECURITY;

-- Optimized RLS policy (wrap auth call in SELECT to avoid per-row evaluation)
-- ※ auth.uid()는 프로젝트 고유 함수 — Supabase 등 플랫폼에 맞게 교체 필요
CREATE POLICY user_orders ON app.orders
  USING (
    (SELECT auth.uid()) = user_id
    AND (SELECT is_active FROM app.user WHERE user_id = (SELECT auth.uid()))
  );

-- Always index RLS policy columns
CREATE INDEX idx_orders_user_id ON app.orders (user_id);

REVOKE ALL ON SCHEMA public FROM public;
```

## JSONB Usage

```sql
CREATE TABLE app.user_setting (
  user_id int NOT NULL,
  setting_data jsonb NOT NULL DEFAULT '{}',
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT user_setting_pk PRIMARY KEY (user_id)
);

-- Query
SELECT setting_data->>'theme' AS theme FROM app.user_setting WHERE user_id = 1;

-- Partial update
UPDATE app.user_setting
SET setting_data = setting_data || '{"theme": "dark"}'::jsonb, updated_at = now()
WHERE user_id = 1;

-- Key existence check
SELECT * FROM app.user_setting WHERE setting_data ? 'theme';
```

## Table Creation Checklist
- [ ] PK uses `GENERATED ALWAYS AS IDENTITY` (not SERIAL)
- [ ] No physical FK constraints (logical only, documented with COMMENT)
- [ ] `timestamptz` used (never `timestamp`)
- [ ] `boolean` type used (never 'Y'/'N' strings)
- [ ] `created_at` 포함 (모든 테이블 필수)
- [ ] `updated_at` 포함 (append-only 로그 테이블 제외) — 트리거 없이 애플리케이션에서 갱신
- [ ] Soft Delete 테이블은 `is_active boolean DEFAULT true` + Partial Index 적용
- [ ] No procedures/triggers/rules
- [ ] Schema separated by purpose (`app`, `log`, `ref`)
