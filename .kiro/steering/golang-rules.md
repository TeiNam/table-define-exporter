---
inclusion: fileMatch
fileMatchPattern: "**/*.go"
---
# Go Coding Style

> This file extends [common/coding-style.md](../common/coding-style.md) with Go-specific content.

## Formatting

- **gofmt** and **goimports** are mandatory — no style debates

## Naming

- `MixedCaps` for exported identifiers, `mixedCaps` for unexported
- Short variable names for small scopes (`i`, `r`, `ctx`); descriptive names for wider scopes
- Interface names: single-method interfaces use method name + `er` (`Reader`, `Stringer`)
- Avoid stuttering: `user.New()` not `user.NewUser()`

## Error Handling

Return errors explicitly. Handle them at every call site:

```go
// WRONG: Ignoring error
data, _ := os.ReadFile("config.json")

// CORRECT: Handle or propagate
data, err := os.ReadFile("config.json")
if err != nil {
    return fmt.Errorf("reading config: %w", err)
}
```

Use `fmt.Errorf` with `%w` to wrap errors. Define sentinel errors for known conditions:

```go
var ErrNotFound = errors.New("not found")

func FindUser(id string) (*User, error) {
    user, err := db.Get(id)
    if err != nil {
        return nil, fmt.Errorf("finding user %s: %w", id, err)
    }
    if user == nil {
        return nil, ErrNotFound
    }
    return user, nil
}
```

Custom error types when callers need structured information:

```go
type ValidationError struct {
    Field   string
    Message string
}

func (e *ValidationError) Error() string {
    return fmt.Sprintf("%s: %s", e.Field, e.Message)
}
```

## Concurrency

Use goroutines for concurrent work. Communicate via channels, not shared memory:

```go
func fetchAll(ctx context.Context, urls []string) []Result {
    results := make(chan Result, len(urls))
    var wg sync.WaitGroup

    for _, url := range urls {
        wg.Add(1)
        go func(u string) {
            defer wg.Done()
            results <- fetch(ctx, u)
        }(url)
    }

    go func() {
        wg.Wait()
        close(results)
    }()

    var out []Result
    for r := range results {
        out = append(out, r)
    }
    return out
}
```

Use `select` for timeout and cancellation:

```go
select {
case result := <-ch:
    process(result)
case <-ctx.Done():
    return ctx.Err()
}
```

Always pass `context.Context` as the first parameter for cancellable operations.

## Package Organization

Group by domain, not by layer:

```
myapp/
  user/
    user.go
    service.go
    repository.go
  order/
    order.go
    service.go
  internal/
    auth/
```

- Keep `package main` thin — parse flags, wire dependencies, call `Run()`
- Use `internal/` to prevent external imports of implementation details

## Interfaces

Accept interfaces, return structs. Define interfaces where they are used, not where implemented:

```go
type UserStore interface {
    FindByID(ctx context.Context, id string) (*User, error)
}

func NewService(store UserStore) *Service {
    return &Service{store: store}
}
```

## Functional Options

Use for complex constructors:

```go
type Option func(*Server)

func WithPort(port int) Option {
    return func(s *Server) { s.port = port }
}

func NewServer(opts ...Option) *Server {
    s := &Server{port: 8080}
    for _, opt := range opts {
        opt(s)
    }
    return s
}
```

## Defer

Use `defer` for cleanup. It runs in LIFO order:

```go
f, err := os.Open(path)
if err != nil {
    return err
}
defer f.Close()
```

## Zero Values

Leverage zero values for clean initialization:

```go
var buf bytes.Buffer
buf.WriteString("hello")
```

# Go Patterns

> This file extends [common/patterns.md](../common/patterns.md) with Go-specific content.

## Error Handling Patterns

### Sentinel Errors

Define sentinel errors for well-known conditions:

```go
var (
    ErrNotFound      = errors.New("not found")
    ErrAlreadyExists = errors.New("already exists")
    ErrUnauthorized  = errors.New("unauthorized")
)

// Caller
if errors.Is(err, ErrNotFound) {
    // handle missing resource
}
```

### Custom Error Types

Use when callers need structured information:

```go
type ValidationError struct {
    Field   string
    Message string
}

func (e *ValidationError) Error() string {
    return fmt.Sprintf("validation: %s: %s", e.Field, e.Message)
}

// Caller
var ve *ValidationError
if errors.As(err, &ve) {
    log.Printf("invalid field: %s", ve.Field)
}
```

### Error Wrapping

Wrap errors with context at each layer using `%w`:

```go
func (s *Service) CreateOrder(ctx context.Context, req CreateOrderReq) error {
    user, err := s.users.FindByID(ctx, req.UserID)
    if err != nil {
        return fmt.Errorf("create order: lookup user: %w", err)
    }
    // ...
}
```

## Dependency Injection

Pass dependencies via struct fields. Wire in `main()`:

```go
type Service struct {
    store  Store
    mailer Mailer
    logger *slog.Logger
}

func NewService(store Store, mailer Mailer, logger *slog.Logger) *Service {
    return &Service{store: store, mailer: mailer, logger: logger}
}
```

## Concurrency Patterns

### Worker Pool

```go
func processItems(ctx context.Context, items []Item, workers int) error {
    g, ctx := errgroup.WithContext(ctx)
    ch := make(chan Item)

    g.Go(func() error {
        defer close(ch)
        for _, item := range items {
            select {
            case ch <- item:
            case <-ctx.Done():
                return ctx.Err()
            }
        }
        return nil
    })

    for i := 0; i < workers; i++ {
        g.Go(func() error {
            for item := range ch {
                if err := process(ctx, item); err != nil {
                    return err
                }
            }
            return nil
        })
    }

    return g.Wait()
}
```

### Fan-out / Fan-in with errgroup

```go
import "golang.org/x/sync/errgroup"

func fetchAll(ctx context.Context, urls []string) ([]Result, error) {
    g, ctx := errgroup.WithContext(ctx)
    results := make([]Result, len(urls))

    for i, url := range urls {
        i, url := i, url
        g.Go(func() error {
            r, err := fetch(ctx, url)
            if err != nil {
                return err
            }
            results[i] = r
            return nil
        })
    }

    if err := g.Wait(); err != nil {
        return nil, err
    }
    return results, nil
}
```

## Middleware Pattern

For HTTP handlers, use middleware chaining:

```go
type Middleware func(http.Handler) http.Handler

func Logging(logger *slog.Logger) Middleware {
    return func(next http.Handler) http.Handler {
        return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            start := time.Now()
            next.ServeHTTP(w, r)
            logger.Info("request", "method", r.Method, "path", r.URL.Path, "duration", time.Since(start))
        })
    }
}
```

## Configuration

Use struct-based configuration. Keep `main()` as the composition root — parse config, create dependencies, wire everything together, then start.

## Graceful Shutdown

```go
func main() {
    srv := &http.Server{Addr: ":8080", Handler: mux}

    go func() {
        if err := srv.ListenAndServe(); err != http.ErrServerClosed {
            log.Fatalf("server error: %v", err)
        }
    }()

    quit := make(chan os.Signal, 1)
    signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
    <-quit

    ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
    defer cancel()
    srv.Shutdown(ctx)
}
```

# Go Security

> This file extends [common/security.md](../common/security.md) with Go-specific content.

## SQL Injection

Always use parameterized queries:

```go
// WRONG: SQL injection risk
query := fmt.Sprintf("SELECT * FROM users WHERE id = '%s'", userID)
db.Query(query)

// CORRECT: Parameterized
db.QueryContext(ctx, "SELECT * FROM users WHERE id = $1", userID)
```

## Input Validation

Validate all external input at API boundaries:

```go
func (r CreateUserRequest) Validate() error {
    if r.Email == "" {
        return errors.New("email is required")
    }
    if r.Age < 0 || r.Age > 150 {
        return errors.New("age out of range")
    }
    return nil
}
```

## Path Traversal

Validate file paths against a base directory:

```go
func safePath(baseDir, userPath string) (string, error) {
    resolved := filepath.Join(baseDir, filepath.Clean(userPath))
    if !strings.HasPrefix(resolved, filepath.Clean(baseDir)+string(os.PathSeparator)) {
        return "", errors.New("path traversal detected")
    }
    return resolved, nil
}
```

## Secrets

- Never hardcode secrets. Use environment variables or a secrets manager
- Use `crypto/rand` for generating secrets, never `math/rand`

```go
import "crypto/rand"

func generateToken(n int) (string, error) {
    b := make([]byte, n)
    if _, err := rand.Read(b); err != nil {
        return "", err
    }
    return base64.URLEncoding.EncodeToString(b), nil
}
```

## Cryptography

- Use `crypto/subtle.ConstantTimeCompare` for token comparison
- Use `golang.org/x/crypto/bcrypt` for password hashing

```go
import "crypto/subtle"

func verifyToken(provided, expected []byte) bool {
    return subtle.ConstantTimeCompare(provided, expected) == 1
}
```

## Context & Timeouts

Always use `context.Context` for timeout control:

```go
ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
defer cancel()
```

## HTTP Security

Set security headers in middleware:

```go
func SecurityHeaders(next http.Handler) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        w.Header().Set("X-Content-Type-Options", "nosniff")
        w.Header().Set("X-Frame-Options", "DENY")
        next.ServeHTTP(w, r)
    })
}
```

## Goroutine Safety

- Protect shared state with `sync.Mutex` or use channels
- Avoid goroutine leaks: ensure goroutines can exit via context cancellation or channel close
- Use `-race` flag in tests: `go test -race ./...`

## Security Scanning

```bash
gosec ./...
govulncheck ./...
```

## Dependency Security

- Run `govulncheck ./...` in CI
- Use `go mod tidy` to remove unused dependencies
- Pin module versions via `go.sum`

# Go Testing

> This file extends [common/testing.md](../common/testing.md) with Go-specific content.

## Test Functions

Use the standard `testing` package. Name tests `Test<Function>_<scenario>`:

```go
func TestParseEmail_ValidInput(t *testing.T) {
    email, err := ParseEmail("alice@example.com")
    if err != nil {
        t.Fatalf("unexpected error: %v", err)
    }
    if email.Domain != "example.com" {
        t.Errorf("got domain %q, want %q", email.Domain, "example.com")
    }
}
```

## Table-Driven Tests

Use table-driven tests for multiple input/output scenarios:

```go
func TestValidateAge(t *testing.T) {
    tests := []struct {
        name    string
        age     int
        wantErr bool
    }{
        {"valid", 25, false},
        {"zero", 0, false},
        {"negative", -1, true},
        {"too old", 200, true},
    }

    for _, tt := range tests {
        t.Run(tt.name, func(t *testing.T) {
            err := ValidateAge(tt.age)
            if (err != nil) != tt.wantErr {
                t.Errorf("ValidateAge(%d) error = %v, wantErr %v", tt.age, err, tt.wantErr)
            }
        })
    }
}
```

## Subtests and Parallel

Use `t.Run` for grouping related tests. Use `t.Parallel()` for independent tests:

```go
func TestUserService(t *testing.T) {
    t.Run("Create", func(t *testing.T) {
        t.Parallel()
        // ...
    })

    t.Run("Delete", func(t *testing.T) {
        t.Parallel()
        // ...
    })
}
```

## Test Helpers

Use `t.Helper()` so failures report the caller's line:

```go
func assertNoError(t *testing.T, err error) {
    t.Helper()
    if err != nil {
        t.Fatalf("unexpected error: %v", err)
    }
}
```

## Mocking with Interfaces

Use interfaces for dependency injection and test with fakes:

```go
type MockStore struct {
    users map[string]*User
}

func (m *MockStore) FindByID(ctx context.Context, id string) (*User, error) {
    u, ok := m.users[id]
    if !ok {
        return nil, ErrNotFound
    }
    return u, nil
}
```

## Integration Tests

Use build tags or `testing.Short()` to separate integration tests:

```go
func TestDatabaseIntegration(t *testing.T) {
    if testing.Short() {
        t.Skip("skipping integration test")
    }
    // ...
}
```

Run with: `go test -short ./...` (skip integration) or `go test ./...` (all).

## Race Detection

Always run with the `-race` flag in CI:

```bash
go test -race ./...
```

## Benchmarks

```go
func BenchmarkParseEmail(b *testing.B) {
    for i := 0; i < b.N; i++ {
        ParseEmail("alice@example.com")
    }
}
```

Run with: `go test -bench=. -benchmem ./...`

## Coverage

```bash
go test -cover ./...
go test -coverprofile=coverage.out ./...
go tool cover -html=coverage.out
```

## Test Organization

```
user/
  user.go
  user_test.go              # Unit tests (same package)
```

- Keep tests in the same package for access to unexported types
- Use `_test` package suffix (e.g., `package user_test`) for black-box tests of the public API
