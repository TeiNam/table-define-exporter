# Performance Guidelines

## Algorithm & Data Structure

- Choose appropriate data structures for access patterns (hash map for lookups, sorted set for range queries)
- Avoid O(n²) operations on collections that may grow — prefer indexed access or batch processing
- Use streaming/generators for large datasets instead of loading everything into memory
- Pre-compute and cache expensive calculations when inputs change infrequently

## Database & Query Optimization

- Avoid N+1 queries — use eager loading, joins, or batch fetches
- Add indexes for columns used in WHERE, JOIN, and ORDER BY clauses
- Use pagination (cursor-based for large datasets, offset for small ones)
- Monitor and log slow queries; set query timeout limits

## Caching Strategy

- Cache at the right layer: application cache for computed results, CDN for static assets, query cache for repeated DB reads
- Always define cache invalidation strategy before adding cache
- Use TTL-based expiration as a safety net even with active invalidation

## Network & I/O

- Parallelize independent I/O operations (concurrent API calls, batch DB operations)
- Set timeouts on all external calls — never wait indefinitely
- Use connection pooling for databases and HTTP clients
- Compress large payloads (gzip/brotli for HTTP, binary formats for internal services)

## Frontend Performance

- Lazy-load routes, images, and heavy components
- Minimize bundle size — tree-shake unused code, split chunks by route
- Avoid layout thrashing — batch DOM reads before writes
- Target Core Web Vitals: LCP < 2.5s, FID < 100ms, CLS < 0.1

## Profiling & Measurement

- Profile before optimizing — measure actual bottlenecks, don't guess
- Set performance budgets (bundle size, API response time, memory usage)
- Use language-native profiling tools (Chrome DevTools, cProfile, pprof, perf)
- Monitor production performance, not just dev environment

