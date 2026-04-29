# Common Patterns

## Starting New Features

When implementing new functionality:
1. Check if the codebase already has a similar pattern — follow existing conventions
2. Look for established libraries or prior art before building from scratch
3. Start with the simplest approach that works; add complexity only when needed

## Design Patterns

### Repository Pattern

Encapsulate data access behind a consistent interface:
- Define standard operations: findAll, findById, create, update, delete
- Concrete implementations handle storage details (database, API, file, etc.)
- Business logic depends on the abstract interface, not the storage mechanism
- Enables easy swapping of data sources and simplifies testing with mocks
- Best for: projects with multiple data sources or when testability is a priority. Avoid for trivial CRUD where the ORM is sufficient.

### API Response Format

Use a consistent envelope for all API responses:
- Include a success/status indicator
- Include the data payload (nullable on error)
- Include structured error details on failure (error code, message, field-level validation errors)
- Include pagination metadata when returning collections (total, page/cursor, limit)
- Use appropriate HTTP status codes (don't return 200 for errors)

### Dependency Injection

Pass dependencies to modules instead of importing them directly:
- Makes testing straightforward (inject mocks/stubs)
- Makes dependencies explicit and visible
- Avoids hidden coupling to specific implementations
- Use constructor injection or function parameters; avoid service locators

### Error Propagation

Establish a consistent error handling strategy across the application:
- Define domain-specific error types (NotFoundError, ValidationError, AuthorizationError)
- Map domain errors to appropriate HTTP status codes at the API boundary
- Preserve error context when re-throwing (wrap, don't replace)
- Log at the boundary, not at every level

