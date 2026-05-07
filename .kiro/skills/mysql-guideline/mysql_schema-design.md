# Schema Design

## Primary Key Policy
- Use `AUTO_INCREMENT` with appropriate unsigned integer type
- Choose type by expected row count: `tinyint` < `smallint` < `int` < `bigint`

```sql
CREATE TABLE `user` (
  `user_id` int unsigned NOT NULL AUTO_INCREMENT,
  `email` varchar(255) NOT NULL,
  `is_active` tinyint(1) NOT NULL DEFAULT 1,
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (`user_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
```

## Foreign Key Policy
- Logical FK only (no physical FK constraints)
- Referential integrity managed at application level
- Document relationships via COMMENT

```sql
CREATE TABLE `chat_history` (
  `chat_history_id` int unsigned NOT NULL AUTO_INCREMENT,
  `user_id` tinyint unsigned NOT NULL COMMENT 'logical FK: user.user_id',
  `conversation_id` char(18) NOT NULL COMMENT 'logical FK: conversation_session.conversation_id',
  `user_message` text NOT NULL,
  `bot_response` text NOT NULL,
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`chat_history_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
```

## Application-Level Referential Integrity

```python
async def create_chat_history(user_id: int, conversation_id: str, message: str, response: str):
    user = db.select("user", where={"user_id": user_id, "is_active": "Y"})
    if not user:
        raise ValueError("User does not exist")

    conversation = db.select("conversation_session", where={"conversation_id": conversation_id})
    if not conversation:
        raise ValueError("Conversation session does not exist")

    return db.insert("chat_history", {
        "user_id": user_id,
        "conversation_id": conversation_id,
        "user_message": message,
        "bot_response": response
    })
```

## Soft Delete Pattern

논리 삭제가 필요한 테이블은 `is_active` 컬럼으로 표준화한다.

```sql
`is_active` tinyint(1) NOT NULL DEFAULT 1  -- 1: 활성, 0: 삭제
```

- 물리 DELETE 금지 (감사 추적, 복구 가능성 확보)
- 조회 시 항상 `WHERE is_active = 1` 포함
- 조회 빈도가 높으면 복합 인덱스 앞쪽에 배치

```sql
-- is_active가 선택성이 낮아도, 쿼리 패턴이 항상 포함할 경우 복합 인덱스에 추가
CREATE INDEX idx_user_active_email ON user (is_active, email);
```

> ⚠️ `is_active` 단독 인덱스는 카디널리티가 낮아 효과 없음. 반드시 복합 인덱스로 사용.


- [ ] No physical FK constraints (logical only, documented with COMMENT)
- [ ] AUTO_INCREMENT with appropriate unsigned type
- [ ] Log tables have monthly partitioning
- [ ] Appropriate indexes created
- [ ] Engine: InnoDB, Charset: utf8mb4
- [ ] No procedures/triggers/events
- [ ] `created_at` 포함 (모든 테이블 필수)
- [ ] `updated_at` 포함 (불변 로그/이력 테이블 제외)
      ※ `chat_history`, `audit_log`, `access_log` 등 append-only 테이블은 생략 가능, COMMENT로 명시 권장
