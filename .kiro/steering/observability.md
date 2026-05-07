# Observability Rules

## The Three Pillars

- **Logs** — discrete events with context
- **Metrics** — numeric measurements over time
- **Traces** — request flow across services

You don't need all three from day one. Start with structured logs; add metrics when you care about aggregates; add tracing when you have more than two services.

## Logging

### Log Levels

| Level | Use for |
|---|---|
| `FATAL` / `CRITICAL` | Process must exit |
| `ERROR` | Something failed; action needed |
| `WARN` | Unexpected but recoverable |
| `INFO` | State changes, requests, key business events |
| `DEBUG` | Developer diagnostics; off in production |
| `TRACE` | Fine-grained flow; rarely used |

Default to `INFO` in production. Never deploy with `DEBUG` enabled globally.

### Structured Logging

Log JSON, not free-form strings:

```json
{
  "time": "2026-05-06T10:23:11Z",
  "level": "error",
  "msg": "payment failed",
  "userId": "u_142",
  "orderId": "o_9912",
  "reason": "insufficient_funds",
  "traceId": "abc123"
}
```

- Use consistent field names across services (`userId`, not `user_id` in some and `uid` in others)
- Always include a trace or request ID
- Include enough context to debug without reading code

### What to Log

- **Do log**: request start/end, auth events, state transitions, errors with stack, external calls with latency
- **Do not log**: passwords, tokens, full credit card numbers, PII, request bodies with secrets
- **Sample high-volume logs** (e.g., log 1% of successful reads, 100% of errors)

### Log Retention

- Hot storage (queryable): 7-30 days
- Cold storage (archive): 1 year or regulatory minimum
- Security / audit logs: per compliance (often 1-7 years)

## Metrics

### Four Signals Every Service Should Emit

1. **Rate** — requests per second
2. **Errors** — error rate (percentage, not count)
3. **Duration** — p50, p95, p99 latency
4. **Saturation** — resource usage (CPU, memory, queue depth)

These are the "RED" (Rate, Errors, Duration) + Saturation pattern.

### Metric Naming

Follow Prometheus conventions:

- `http_requests_total` (counter, always suffix `_total`)
- `http_request_duration_seconds` (histogram, unit in name)
- `db_connections_active` (gauge)
- Use labels for dimensions: `http_requests_total{method="GET", status="200", route="/users"}`

Keep cardinality bounded. Don't put user IDs in labels.

### What to Measure

- **Business metrics**: signups, purchases, conversions — these prove the system works
- **System metrics**: latency, error rate, throughput — these prove it's healthy
- **Saturation metrics**: queue length, CPU, memory — these warn before failure

### Alerting

- Alert on **symptoms** (users affected), not **causes** (CPU high)
- Every alert must have: runbook link, severity, owner
- Page only for things that need action now
- Tune aggressively — noisy alerts train people to ignore them

## Tracing

### When to Add Tracing

- Two or more services involved in a single user request
- Async processing across queues
- Hard-to-reproduce latency issues

OpenTelemetry is the default standard. Instrument at the framework level when possible.

### Span Guidelines

- One span per logical operation (request, query, external call)
- Name spans after the operation: `GET /users/:id`, `db.query`, `s3.getObject`
- Attach attributes for business context: `user.id`, `order.id`, `feature.flag`
- Propagate trace context across service boundaries (HTTP headers, queue metadata)

Do not span every function call. That's noise.

## Correlation

Every log line, every metric exemplar, and every trace must share a request or trace ID:

- Generate at the edge (API gateway or load balancer)
- Pass via header: `X-Request-ID` or `traceparent`
- Include in every downstream log and span

This single rule unlocks end-to-end debugging.

## Error Tracking

Separate from logs, use a dedicated error tracker (Sentry, Rollbar, GlitchTip):

- Group by stack trace + error type
- Attach user, request, environment context
- Alert on new error types or spikes
- Link errors to deploys for quick rollback decisions

## Dashboards

- **One dashboard per service** — the four signals up top
- **One dashboard per user journey** — end-to-end view
- Keep dashboards readable at a glance; a wall of graphs is useless
- Link dashboards to runbooks

## Cost Control

Observability data is expensive at scale:

- Sample aggressively (head-based at the edge, or tail-based for errors)
- Drop high-cardinality labels early
- Aggregate before shipping (histograms > raw events)
- Delete old dashboards and alerts that nobody uses

## Incident Response

During an incident:

1. Check the service's four-signal dashboard first
2. Correlate by request ID across services
3. Recent deploys are the most likely cause (check deploy timeline)
4. Capture timeline in a doc as you go

After:

- Write a blameless postmortem within 72 hours
- Include: impact, timeline, root cause, remediation, action items
- File action items as tracked work

## Anti-patterns

- `print()` / `console.log()` in production code
- Logging secrets, tokens, or PII
- Alerting on every warning
- Unbounded label cardinality (user ID, request ID as labels)
- Spans on every function in a hot path
- Dashboards that nobody reads
