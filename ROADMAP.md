# Nova development roadmap

Nova's destination is a production-grade, open-source time-series database with a fast
single-node core, compressed historical storage, expressive time-oriented queries, and a clear
path from one process to a distributed service.

This roadmap is organized by capability and exit criteria rather than calendar promises. Versions
may change as benchmarks and failure testing reveal better designs.

## North-star outcome

Nova should eventually provide:

- Sustained, predictable ingestion for append-heavy time-series workloads
- Low-latency reads over recent data and efficient scans over compressed history
- Fast, deterministic crash recovery with explicitly documented durability modes
- High-cardinality label filtering without unbounded index memory growth
- Retention, downsampling, compaction, and tiered storage
- Replication, failover, backup, and safe operational tooling
- Prometheus-compatible integrations and stable client libraries
- An embeddable Rust engine and a simple standalone deployment

Performance claims will only be published with reproducible hardware, datasets, configurations,
competitor versions, and methodology. “Millions of points per second” is a goal to investigate,
not a claim Nova makes today.

## Status legend

- [x] Implemented and covered by automated tests
- [ ] Planned
- **Exit criteria** define what must be true before the milestone is considered complete

## Milestone 0 — project foundation

### Repository and engineering

- [x] Rust workspace with independent architectural crates
- [x] Server, CLI, engine, protocol, storage, WAL, compression, replication, and shared types
  boundaries
- [x] README, architecture, vision, use cases, contribution guide, security policy, and licenses
- [x] GitHub issue forms, pull-request template, and CI
- [ ] Architecture decision record and RFC templates
- [ ] Maintainer and reviewer policy
- [ ] Release checklist and changelog process
- [ ] Dependency auditing, license checks, and supply-chain policy
- [ ] Code coverage reporting with meaningful thresholds for storage and recovery code

### Quality baseline

- [x] Formatting, strict linting, unit tests, and documentation tests in CI
- [ ] Property-based tests for parsers and persistent formats
- [ ] Fuzz targets for protocol, WAL, segment, and snapshot decoders
- [ ] Sanitizer and Miri jobs for critical unsafe-free code paths
- [ ] Cross-platform CI for Linux and macOS
- [ ] Supported Rust version policy

**Exit criteria**

- A new contributor can build, test, understand, and change one crate from the documentation.
- Every persistent or network format has corruption tests and a versioning policy.
- CI prevents formatting, lint, test, audit, and compatibility regressions.

## Milestone 1 — durable single-node vertical slice

### Implemented

- [x] Validated metric names and timestamped `f64` points
- [x] Checksummed, append-only WAL frames
- [x] Write acknowledgment after `sync_data`
- [x] WAL replay on restart
- [x] Timestamp-ordered in-memory range reads
- [x] TCP server, line protocol, CLI, `PING`, `WRITE`, `RANGE`, and `INFO`
- [x] Recovery test proving write → close → reopen → read

### Required to complete the slice

- [x] End-to-end tests that launch the server and exercise real client connections
- [x] Clean shutdown with a final durability barrier
- [x] Torn/truncated WAL-tail detection and safe repair
- [x] Maximum frame, metric, request, and connection limits
- [x] Duplicate timestamp and out-of-order write semantics
- [x] Batch write command with atomicity rules
- [ ] Config file plus environment and command-line precedence
- [x] Structured error codes rather than human-text-only errors
- [x] Version and build metadata in `INFO`
- [ ] Graceful resource exhaustion behavior

**Exit criteria**

- Acknowledged writes survive process termination and restart under an automated crash matrix.
- Corrupt complete frames fail loudly; incomplete final frames are handled without losing earlier
  valid records.
- Protocol limits make untrusted client input bounded.
- The quick start works on a clean machine exactly as documented.

## Milestone 2 — benchmark and observability harness

Build the measurement system before optimizing the engine.

- [ ] Criterion microbenchmarks for parsing, WAL encoding, checksums, and in-memory insertion
- [ ] Workload generator for constant-rate, bursty, ordered, and out-of-order ingestion
- [ ] Dataset fixtures representing infrastructure metrics, IoT sensors, financial ticks, and
  high-cardinality labels
- [ ] End-to-end throughput, p50/p95/p99/p999 latency, memory, CPU, disk, and recovery benchmarks
- [ ] Soak tests lasting hours and days
- [ ] Flamegraph and allocation profiling scripts
- [ ] Machine-readable benchmark results and regression thresholds
- [ ] Internal metrics for commands, errors, queue depth, WAL latency, memory, and disk activity
- [ ] Health, readiness, and diagnostics endpoints

**Exit criteria**

- Every major optimization can be evaluated against repeatable baselines.
- CI detects material performance regressions on controlled runners.
- Nova reports enough internal state to explain latency or throughput degradation.

## Milestone 3 — memory segments and ingestion engine

Replace the per-point ordered-map baseline with a time-series-native memory layout.

- [ ] Stable series identity derived from metric name and canonical label set
- [ ] Bounded mutable segments with configurable point and byte limits
- [ ] Sequential timestamp and value buffers for cache locality
- [ ] Explicit handling for late and out-of-order points
- [ ] Segment sealing and immutable handoff
- [ ] Per-series and global memory accounting
- [ ] Backpressure rather than unbounded queues
- [ ] Configurable durability modes: always-sync, group commit, and explicitly unsafe modes
- [ ] Group commit with latency bounds
- [ ] Single logical command executor benchmarked against sharded alternatives
- [ ] Background worker scheduling isolated from foreground latency

**Exit criteria**

- Memory use is bounded under sustained ingestion.
- Nova maintains its configured durability contract during bursts and slow storage.
- Segment sealing cannot reorder, duplicate, or silently lose acknowledged points.
- The chosen execution model is supported by published measurements.

## Milestone 4 — immutable segment storage

- [ ] Versioned segment-file specification
- [ ] Separate timestamp, value, and metadata blocks
- [ ] Start/end time, point count, min/max, checksum, and codec metadata
- [ ] Atomic segment publication using temporary files and rename
- [ ] Manifest or catalog describing live segments
- [ ] Memory-mapped or buffered historical reads selected through benchmarks
- [ ] Segment-level and block-level integrity validation
- [ ] Startup recovery without replaying the entire lifetime WAL
- [ ] Snapshot/checkpoint format
- [ ] WAL truncation only after checkpoint durability is proven
- [ ] Segment inspection and repair CLI

**Exit criteria**

- Historical data survives restart independently of the active WAL.
- Nova can detect corruption and identify the exact affected file or block.
- Recovery time is proportional to recent uncheckpointed work, not total database history.
- Persistent-format compatibility is tested with checked-in golden fixtures.

## Milestone 5 — time-series compression

- [ ] Plain reference codecs for correctness comparison
- [ ] Delta and delta-of-delta timestamp codecs
- [ ] Gorilla-style XOR codec for floating-point values
- [ ] Run-length encoding for repeated values and regular intervals
- [ ] Dictionary encoding for repeated labels and metadata
- [ ] Optional LZ4/Zstandard compression for appropriate blocks
- [ ] Codec selection by measured data characteristics
- [ ] Zero-copy or low-copy decoding interfaces
- [ ] Corruption, truncation, and decompression-bomb protections
- [ ] Compression ratio and decode-throughput benchmark suite

**Exit criteria**

- Every codec round-trips arbitrary valid input under property tests and fuzzing.
- Codec metadata is versioned and unknown codecs fail safely.
- Compression improves representative storage footprints without unacceptable query-latency
  regressions.

## Milestone 6 — labels and indexing

- [ ] Canonical, immutable label sets
- [ ] Stable internal series IDs
- [ ] Inverted index from label pairs to candidate series
- [ ] Exact, negative, regular-expression, and existence matchers
- [ ] Cardinality accounting and configurable limits
- [ ] Index persistence and recovery
- [ ] Tombstones for deleted or expired series
- [ ] Index compaction and garbage collection
- [ ] APIs for cardinality discovery and top label values
- [ ] Protections against adversarial label explosions

**Exit criteria**

- Label queries avoid full-database scans.
- Index memory and disk growth are measured and bounded.
- Restart, retention, and deletion preserve index/data consistency.
- High-cardinality benchmarks publish the practical operating envelope.

## Milestone 7 — query engine

- [ ] Query intermediate representation independent of wire protocols
- [ ] Time and label predicate planning
- [ ] Segment pruning using time bounds and metadata
- [ ] Compression-aware scans
- [ ] Merge of mutable memory and immutable disk results
- [ ] `LAST N` and range scans
- [ ] `min`, `max`, `sum`, `count`, `avg`, and rate-style aggregations
- [ ] Time buckets and downsampling
- [ ] Grouping by labels
- [ ] Aggregate pushdown into segment scans
- [ ] Vectorized/SIMD execution where benchmarks justify it
- [ ] Query memory budgets, deadlines, cancellation, and result limits
- [ ] Consistent-read semantics during concurrent ingestion and compaction

**Exit criteria**

- Queries return deterministic results across memory, disk, and compaction boundaries.
- Expensive queries cannot starve ingestion or exceed configured resource budgets.
- Correctness is tested against a simple reference executor.
- Query plans and resource consumption are explainable to operators.

## Milestone 8 — retention, compaction, and downsampling

- [ ] Per-database and per-series retention policies
- [ ] Expiration scheduling and tombstone semantics
- [ ] Leveled or size-tiered compaction chosen by workload measurements
- [ ] Crash-safe segment replacement
- [ ] Read correctness while compaction is active
- [ ] Automatic rollups such as 1 second → 1 minute → 1 hour
- [ ] User-defined rollup functions and retention windows
- [ ] Disk-space watermarks and emergency backpressure
- [ ] Compaction throttling and observable debt
- [ ] Administrative dry-run and explain commands

**Exit criteria**

- Retention reclaims space predictably without blocking the command path.
- Crashing at every compaction transition leaves a recoverable database.
- Downsampled results have documented precision and boundary semantics.
- Operators can see and control compaction debt.

## Milestone 9 — stable protocols and ecosystem integrations

- [ ] Versioned native protocol with capability negotiation
- [ ] RESP3 compatibility where semantics fit Nova cleanly
- [ ] HTTP ingestion and query API
- [ ] Prometheus remote write ingestion
- [ ] PromQL-compatible query surface or a clearly documented supported subset
- [ ] OpenMetrics exposition for Nova internals
- [ ] Import/export tools for CSV and line-oriented formats
- [ ] Rust client with connection pooling, batching, retries, and timeouts
- [ ] Go, Python, Java, and TypeScript clients based on demonstrated demand
- [ ] Grafana data-source integration
- [ ] Docker image, Compose example, and package artifacts

**Exit criteria**

- Protocol compatibility tests run against every release.
- Clients expose retry and idempotency behavior consistent with server semantics.
- A user can ingest, query, visualize, back up, and restore Nova using documented tools.

## Milestone 10 — production operations

- [ ] Stable configuration schema and validation
- [ ] Online backup and verified restore
- [ ] Point-in-time recovery boundaries
- [ ] Authentication, authorization, and TLS
- [ ] Audit logging for administrative actions
- [ ] Per-tenant quotas and resource isolation where multi-tenancy is enabled
- [ ] Upgrade, downgrade, and rollback procedures
- [ ] Storage migration tooling
- [ ] Kubernetes manifests, Helm chart, and operator only after process semantics stabilize
- [ ] Capacity-planning guide and production runbooks
- [ ] Failure injection for disk-full, slow I/O, clock changes, memory pressure, and network faults
- [ ] Release candidates and long-running upgrade tests

**Exit criteria**

- Backup restoration is continuously tested, not only documented.
- Operators can upgrade supported versions without data loss.
- Security defaults are safe for networked deployment.
- Known limits and failure behavior are documented with evidence.

## Milestone 11 — replication and high availability

- [ ] Ordered replication log with monotonic offsets
- [ ] Full snapshot plus incremental catch-up
- [ ] Primary-to-replica streaming
- [ ] Replica lag, health, and durability metrics
- [ ] Read-only replicas and documented consistency
- [ ] Reconnect and partial resynchronization
- [ ] Manual promotion with fencing
- [ ] Automated failover design and failure model
- [ ] Split-brain prevention
- [ ] Cross-version replication compatibility

**Exit criteria**

- Replicas converge after disconnects, restarts, and snapshots.
- Promotion cannot acknowledge conflicting primaries under the documented failure model.
- Data-loss windows are explicit for every durability configuration.
- Jepsen-style fault tests or equivalent validate the published guarantees.

## Milestone 12 — partitioning, clustering, and tiered storage

- [ ] Deterministic series-based partitioning
- [ ] Cluster metadata and membership model
- [ ] Rebalancing without unbounded ingestion pauses
- [ ] Query fan-out, partial failure, and result merging
- [ ] Tenant-aware placement and quotas
- [ ] Local SSD to object-storage tiering
- [ ] Object immutability, checksums, caching, and lifecycle rules
- [ ] Disaster recovery across failure domains
- [ ] Evaluate Raft or another consensus mechanism for metadata and failover coordination
- [ ] Multi-region architecture only with explicit latency and consistency tradeoffs

**Exit criteria**

- Rebalancing and node failure preserve the documented availability and durability guarantees.
- Cluster-wide queries remain bounded and report partial results explicitly.
- Object-tier loss, corruption, and stale-cache behavior are tested.
- Operators can add, replace, and remove nodes using supported workflows.

## Milestone 13 — Nova 1.0

Nova 1.0 is a compatibility and trust milestone, not merely a feature milestone.

- [ ] Stable storage format and published compatibility window
- [ ] Stable protocol and client compatibility policy
- [ ] Semantic versioning and deprecation policy
- [ ] At least one production-proven deployment profile
- [ ] Published performance results and honest comparison methodology
- [ ] Security review of exposed and persistent-format surfaces
- [ ] Complete operator, architecture, client, and contributor documentation
- [ ] Recovery, backup, upgrade, and fault-injection suites passing continuously
- [ ] Governance and maintainer succession documented

**Exit criteria**

- Users can operate Nova through failure, backup, restore, and upgrade using supported procedures.
- The project can evolve without silently breaking stored data or clients.
- Claims in the README are backed by repeatable tests and real deployments.

## Ecosystem after 1.0

Development can expand according to user demand:

- Embedded engine API
- Edge synchronization
- Managed Nova service
- Kubernetes operator
- Additional language clients
- SQL, PromQL, or InfluxQL compatibility layers
- Streaming and change-data integrations
- Cloud marketplaces and enterprise support

The open-source engine remains the foundation. Commercial offerings, if created, should fund the
project without making the core database deliberately incomplete.
