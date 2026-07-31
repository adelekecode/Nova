# Nova architecture

Nova takes inspiration from Redis's engineering philosophy—small command execution, append-only
durability, and operational simplicity—without copying Redis or treating time-series data as
ordinary key/value records.

This document separates what Nova does **today** from the architecture it is **working toward**.

## Design principles

1. Keep foreground command execution small and predictable.
2. Acknowledge writes only after the configured durability boundary is satisfied.
3. Bound memory, queues, requests, and background work.
4. Optimize layouts for time-ordered numerical data.
5. Move compression, compaction, snapshots, and replication off the command path.
6. Prefer explicit, versioned, testable formats over opaque serialization.
7. Add concurrency only when measurements show it improves the target workload.
8. Treat crash recovery and observability as product features.

## Current architecture

```text
client
  |
  v
TCP line protocol -> parser -> serialized engine access
                                  |
                                  +-> append + sync checksummed WAL
                                  |
                                  +-> ordered in-memory series
```

The TCP server accepts connections concurrently, while a mutex serializes access to the engine.
Writes append and synchronize a WAL frame before updating the visible in-memory index. Startup
replays valid WAL frames to rebuild the index. Range reads come from per-metric ordered maps.

This is a correctness baseline. It is not yet the target storage layout or a final claim about the
best scheduling model.

## Target architecture

```text
                  RESP3 / HTTP / Prometheus / native clients
                                      |
                                      v
                            network + protocol layer
                                      |
                                      v
                         serialized command executor
                              /               \
                             v                 v
                       write engine         query engine
                             \                 /
                              v               v
                         time-series engine + index
                                      |
                         mutable in-memory segments
                                      |
                         seal · compress · checksum
                                      |
                         immutable disk segments
                                      |
                      compact · retain · downsample
                                      |
                           object storage tiers

          background: WAL · snapshots · compression · compaction · replication
```

## Write path

```text
receive -> parse -> validate -> resolve series -> append WAL -> memory segment -> acknowledge
                                                                  |
                                                                  v
                                                     seal and flush in background
```

Durability modes will be explicit. The safest mode synchronizes before acknowledgment; group
commit may trade a bounded amount of latency for higher throughput. Unsafe modes, if offered,
will be named and documented as such.

## Read path

```text
query -> plan -> label index -> prune by time -> scan memory + disk -> aggregate -> merge -> return
```

The planner should select only relevant series and segments. Segment metadata enables time
pruning and aggregate pushdown. Decoders operate directly on compressed blocks where practical.
Every query receives memory, time, and result limits.

## Series and segments

A series is identified by a metric name and a canonical set of labels:

```text
cpu.usage{host="web-01", region="lagos", rack="4"}
```

Each series owns a sequence of bounded time segments:

```text
series
├── mutable segment
├── immutable segment
├── immutable segment
└── ...
```

Segments store timestamps and values in separate sequential buffers. This reduces allocation
overhead, improves cache locality, and makes column-specific compression possible.

The initial size—whether 4,096 points or another byte/point threshold—will be selected through
benchmarks rather than fixed by intuition.

## Compression

Candidate timestamp encodings include delta and delta-of-delta. Candidate value encodings include
Gorilla-style XOR, run-length encoding for repeats, and plain values when compression would add
cost without benefit. Labels and metadata may use dictionary encoding.

Codecs are block-scoped, checksummed, versioned, and selected using representative datasets.
Corrupt or unknown blocks fail safely.

## Labels and indexing

Nova's target index maps label pairs to candidate series IDs:

```text
host=web-01  -> {series 1, series 9, series 20}
region=lagos -> {series 1, series 2, series 9}
```

Set intersection answers compound selectors without scanning every series. Cardinality budgets,
persistent index recovery, tombstones, and index garbage collection are part of the design—not
afterthoughts.

## Persistence and recovery

The current WAL frame is:

```text
magic[4] | payload_length[u32] | crc32[u32] | payload
```

The payload contains a metric length, UTF-8 metric bytes, an `i64` millisecond timestamp, and the
raw IEEE-754 bits of an `f64`, all using little-endian integers.

The target recovery chain is:

```text
manifest -> immutable segments + snapshot -> replay recent WAL tail -> serve
```

This bounds recovery time. WAL truncation only occurs after the corresponding checkpoint and
segments are durable.

## Background work

Foreground execution should not perform unbounded compression, compaction, retention, snapshot,
or replication work. Background workers receive explicit concurrency, I/O, memory, and queue
budgets. When those budgets are exhausted, Nova applies observable backpressure.

## Replication and distribution

The first distributed model will likely stream an ordered log from a primary to replicas, with
snapshot plus incremental catch-up. Exact consistency and failover guarantees will be published
only after fault testing.

Sharding, cluster membership, and consensus are intentionally deferred until the single-node
storage and recovery model is stable. Consensus may coordinate metadata and failover; it is not a
substitute for designing correct local persistence.

## Crate boundaries

```text
crates/
├── server        Process lifecycle, networking, and command scheduling
├── protocol      Wire command and response parsing
├── engine        Write/read semantics and in-memory state
├── storage       Segment, snapshot, manifest, retention, and compaction coordination
├── wal           Append-only log format, synchronization, repair, and replay
├── compression   Timestamp, value, and metadata codecs
├── replication   Log streaming, replica state, snapshots, and failover primitives
├── types         Shared domain types and validation
└── cli           User and operator command-line client
```

Future query, index, metrics, and client crates should be introduced when real implementations
need independent ownership—not merely to make the workspace appear larger.

## Non-goals before 1.0

- General-purpose relational transactions
- Distributed consensus before local durability is proven
- Every query language at once
- Performance claims without reproducible evidence
- Compatibility that compromises clear time-series semantics

See [ROADMAP.md](ROADMAP.md) for the implementation order and exit criteria.

