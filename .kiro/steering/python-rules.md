---
inclusion: fileMatch
fileMatchPattern: "**/*.py,**/*.pyi"
---
# Python Coding Style

> This file extends [common/coding-style.md](../common/coding-style.md) with Python-specific content.

## Type Hints

Add type hints to public APIs; let internal code remain concise:

```python
# WRONG: No hints on public function
def fetch_users(ids, active_only):
    ...

# CORRECT: Explicit parameter and return types
def fetch_users(ids: list[str], active_only: bool = True) -> list[User]:
    ...
```

Use `typing` imports only when needed (Python 3.10+ supports `X | None` natively):

```python
def find_user(user_id: str) -> User | None:
    ...
```

## Comprehensions and Generators

Prefer comprehensions for simple transforms. Use generators for large or lazy sequences:

```python
# List comprehension — clear and concise
names = [u.name for u in users if u.active]

# Generator — avoids loading everything into memory
def read_large_file(path: str):
    with open(path) as f:
        yield from (line.strip() for line in f if line.strip())
```

Keep comprehensions to one level of nesting. If it needs two loops or complex conditions, use a regular loop.

## Context Managers

Use `with` for any resource that needs cleanup:

```python
# File I/O
with open("data.json") as f:
    data = json.load(f)

# Custom context managers
from contextlib import contextmanager

@contextmanager
def temp_directory():
    d = tempfile.mkdtemp()
    try:
        yield d
    finally:
        shutil.rmtree(d)
```

## Async / Await

Use `asyncio` for I/O-bound concurrency. Do not mix sync blocking calls in async code:

```python
import asyncio

async def fetch_all(urls: list[str]) -> list[Response]:
    async with aiohttp.ClientSession() as session:
        tasks = [session.get(url) for url in urls]
        return await asyncio.gather(*tasks)
```

Use `asyncio.TaskGroup` (Python 3.11+) for structured concurrency:

```python
async def process_batch(items: list[Item]) -> list[Result]:
    async with asyncio.TaskGroup() as tg:
        tasks = [tg.create_task(process(item)) for item in items]
    return [t.result() for t in tasks]
```

## Decorators

Use decorators to separate cross-cutting concerns from business logic:

```python
import functools

def retry(max_attempts: int = 3, delay: float = 1.0):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            for attempt in range(max_attempts):
                try:
                    return func(*args, **kwargs)
                except Exception:
                    if attempt == max_attempts - 1:
                        raise
                    time.sleep(delay * (2 ** attempt))
        return wrapper
    return decorator
```

## Immutability

Prefer tuples over lists for fixed collections. Use `@dataclass(frozen=True)` for value objects:

```python
from dataclasses import dataclass

@dataclass(frozen=True)
class Coordinate:
    lat: float
    lon: float
```

## Formatting

- **black** for code formatting
- **isort** for import sorting
- **ruff** for linting

## Naming Conventions

- `snake_case` for functions, variables, modules
- `PascalCase` for classes
- `UPPER_SNAKE_CASE` for module-level constants
- Prefix private attributes with `_`

## String Formatting

Use f-strings:

```python
# WRONG
message = "Hello, %s. You have %d items." % (name, count)

# CORRECT
message = f"Hello, {name}. You have {count} items."
```

# Python Patterns

> This file extends [common/patterns.md](../common/patterns.md) with Python-specific content.

## Module Organization

Structure packages by domain, not by type:

```
# WRONG: grouped by type
models/
  user.py
  order.py
services/
  user.py
  order.py

# CORRECT: grouped by domain
users/
  __init__.py
  models.py
  services.py
  repository.py
orders/
  __init__.py
  models.py
  services.py
```

Keep `__init__.py` files minimal — use them for public API re-exports, not logic.

## Protocol (Structural Typing)

Use `Protocol` for duck-typed interfaces:

```python
from typing import Protocol

class Repository(Protocol):
    def find_by_id(self, id: str) -> dict | None: ...
    def save(self, entity: dict) -> dict: ...
```

## Dataclasses and Pydantic

Use `dataclasses` for internal value objects. Use Pydantic for external data validation:

```python
from dataclasses import dataclass
from pydantic import BaseModel, EmailStr, Field

# Internal value object
@dataclass(frozen=True)
class Coordinate:
    lat: float
    lon: float

# External input validation
class CreateUserRequest(BaseModel):
    name: str
    email: EmailStr
    age: int = Field(ge=0, le=150)
```

## Dependency Injection

Pass dependencies explicitly. Use constructor injection or function parameters:

```python
class OrderService:
    def __init__(self, repo: OrderRepository, notifier: Notifier):
        self._repo = repo
        self._notifier = notifier

    def place_order(self, order: Order) -> Order:
        saved = self._repo.save(order)
        self._notifier.send(f"Order {saved.id} placed")
        return saved
```

For FastAPI, use `Depends()`:

```python
from fastapi import Depends

def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()

@app.get("/users/{user_id}")
def get_user(user_id: str, db: Session = Depends(get_db)):
    return db.query(User).get(user_id)
```

## Error Handling

Define domain-specific exceptions. Catch specific exceptions, not bare `except`:

```python
class AppError(Exception):
    """Base for all application errors."""

class NotFoundError(AppError):
    def __init__(self, entity: str, entity_id: str):
        super().__init__(f"{entity} {entity_id} not found")
        self.entity = entity
        self.entity_id = entity_id

class ValidationError(AppError):
    def __init__(self, field: str, message: str):
        super().__init__(f"{field}: {message}")
        self.field = field
```

## Configuration

Use environment variables with typed defaults:

```python
from pydantic_settings import BaseSettings

class Settings(BaseSettings):
    database_url: str
    debug: bool = False
    max_connections: int = 10

    model_config = {"env_prefix": "APP_"}

settings = Settings()
```

## Logging

Use the standard `logging` module with structured context:

```python
import logging

logger = logging.getLogger(__name__)

def process_order(order_id: str):
    logger.info("Processing order", extra={"order_id": order_id})
```

# Python Security

> This file extends [common/security.md](../common/security.md) with Python-specific content.

## SQL Injection

Always use parameterized queries. Never format SQL strings:

```python
# WRONG: SQL injection risk
cursor.execute(f"SELECT * FROM users WHERE id = '{user_id}'")

# CORRECT: Parameterized query
cursor.execute("SELECT * FROM users WHERE id = %s", (user_id,))

# CORRECT: ORM (SQLAlchemy)
session.query(User).filter(User.id == user_id).first()
```

## Input Validation

Validate all external input at the boundary. Use Pydantic for API inputs:

```python
from pydantic import BaseModel, Field, EmailStr

class CreateUserRequest(BaseModel):
    name: str = Field(min_length=1, max_length=200)
    email: EmailStr
    age: int = Field(ge=0, le=150)
```

Sanitize file paths to prevent path traversal:

```python
import os

def safe_read(base_dir: str, filename: str) -> str:
    resolved = os.path.realpath(os.path.join(base_dir, filename))
    if not resolved.startswith(os.path.realpath(base_dir)):
        raise ValueError("Path traversal detected")
    with open(resolved) as f:
        return f.read()
```

## Secrets Management

- Never hardcode secrets in source code
- Use environment variables or a secrets manager
- Use `python-dotenv` for local development only; never commit `.env` files

```python
import os

DATABASE_URL = os.environ["DATABASE_URL"]  # Fail loud if missing
```

## Dependency Security

- Pin dependencies in `requirements.txt` or `pyproject.toml`
- Run `pip audit` or `safety check` in CI to detect known vulnerabilities
- Keep dependencies updated; use Dependabot or Renovate

## Deserialization

Never use `pickle` or `eval()` on untrusted data:

```python
# WRONG: Arbitrary code execution
data = pickle.loads(untrusted_bytes)
result = eval(user_input)

# CORRECT: Use safe formats
data = json.loads(untrusted_string)
```

## Authentication

- Use `bcrypt` or `argon2` for password hashing (never MD5/SHA for passwords)
- Use constant-time comparison for tokens: `hmac.compare_digest(a, b)`
- Validate JWTs with signature verification and expiry checks

## Subprocess Safety

Avoid `shell=True`. Pass arguments as a list:

```python
import subprocess

# WRONG: Shell injection risk
subprocess.run(f"ls {user_input}", shell=True)

# CORRECT: No shell interpretation
subprocess.run(["ls", user_input], check=True)
```

## Security Scanning

```bash
bandit -r src/
pip audit
```

## Logging

Never log secrets, tokens, or passwords.

# Python Testing

> This file extends [common/testing.md](../common/testing.md) with Python-specific content.

## Framework: pytest

Use `pytest` as the default test framework. Avoid `unittest.TestCase` unless integrating with legacy code.

```python
def test_calculate_total():
    order = Order(items=[Item(price=10), Item(price=20)])
    assert order.total == 30
```

## Fixtures

Use fixtures for setup/teardown. Prefer function-scoped fixtures; use broader scopes only when setup is expensive:

```python
import pytest

@pytest.fixture
def db_session():
    session = create_test_session()
    yield session
    session.rollback()
    session.close()

@pytest.fixture(scope="module")
def api_client():
    """Expensive setup — shared across module."""
    client = TestClient(app)
    yield client

def test_create_user(db_session):
    user = create_user(db_session, name="Alice")
    assert user.id is not None
```

## Parametrize

Use `@pytest.mark.parametrize` to test multiple inputs without duplicating test functions:

```python
@pytest.mark.parametrize("input_val,expected", [
    ("hello@example.com", True),
    ("not-an-email", False),
    ("", False),
    ("a@b.c", True),
])
def test_validate_email(input_val, expected):
    assert validate_email(input_val) == expected
```

## Mocking

Use `unittest.mock` or `pytest-mock`. Patch at the point of import, not at the source:

```python
def test_send_notification(mocker):
    mock_send = mocker.patch("myapp.notifications.smtp_client.send")
    notify_user(user_id="123", message="Hello")
    mock_send.assert_called_once()
```

## Async Tests

Use `pytest-asyncio` for testing async code:

```python
import pytest

@pytest.mark.asyncio
async def test_fetch_user():
    user = await fetch_user("123")
    assert user.name == "Alice"
```

## Exception Testing

Use `pytest.raises` as a context manager:

```python
def test_invalid_age():
    with pytest.raises(ValueError, match="must be positive"):
        create_user(name="Bob", age=-1)
```

## Test Organization

```
tests/
  conftest.py          # Shared fixtures
  test_models.py       # Unit tests
  test_services.py
  integration/
    conftest.py        # Integration-specific fixtures
    test_api.py
```

- Name test files `test_*.py`
- Name fixtures in `conftest.py` at the appropriate directory level
- Keep test helpers in `tests/helpers/` if shared across multiple test files

## Coverage

```bash
pytest --cov=myapp --cov-report=term-missing
```

Focus coverage on business logic, not boilerplate.
