<div align="center">

# Nova

### A fast, focused time-series database for the systems that never stop producing data.

**Memory-first. Durable by design. Built from the ground up in Rust.**

[Vision](#the-vision) · [Use cases](docs/USE_CASES.md) ·
[Architecture](#where-nova-is-going) · [Quick start](#quick-start) · [Roadmap](ROADMAP.md) ·
[Contributing](CONTRIBUTING.md)

</div>

---

Nova is an open-source time-series database inspired by the qualities that made Redis enduring:
a small command path, predictable latency, operational simplicity, and an architecture engineers
can understand.

It applies those ideas to a different problem—storing and querying enormous streams of timestamped
data. Nova is being designed for infrastructure metrics, IoT telemetry, financial ticks, energy
systems, industrial sensors, and every environment where data arrives continuously and recent
answers need to be fast.

> [!WARNING]
> Nova is pre-alpha software. Today it is a working foundation, not a production database. Its
> storage format and protocol will change before the first stable release.

## The vision

Modern systems generate more time-series data every year, but operating the databases behind that
data can still be expensive and complex. Nova's long-term goal is to make high-performance
time-series storage feel simple:

- **Fast enough for the hot path** — low-latency ingestion and recent-data reads without routing
  every operation through a heavyweight query engine.
- **Efficient across time** — mutable data in memory, compressed immutable segments on disk, and
  optional object storage for deep history.
- **Predictable under load** — foreground command execution stays small while compression,
  compaction, snapshots, and replication happen in controlled background work.
- **Understandable to operators** — explicit durability, observable resource use, safe defaults,
  and a deployment model that does not require a large platform team.
- **Embeddable and extensible** — a clean Rust core that can eventually power a standalone server,
  edge deployments, and application-native telemetry.

Nova is not intended to become a general-purpose relational database. It is being built to do one
job exceptionally well:

> Ingest timestamped data continuously, retain it efficiently, and answer time-oriented questions
> with predictable speed.

## Why build another time-series database?

Nova starts from a deliberately different center of gravity.

| Nova's direction | What it means |
| --- | --- |
| Redis-inspired execution | A short, serialized command path before introducing concurrency only where measurements justify it |
| Time-series-native storage | Segments, timestamp compression, value compression, retention, and downsampling are foundational concerns |
| Rust from day one | Memory safety, explicit resource ownership, and performance without a garbage collector |
| Memory-first, not memory-only | Recent data remains immediately accessible while historical data moves through durable storage tiers |
| Small core, rich edges | Persistence, compaction, replication, integrations, and query interfaces evolve around a focused engine |

The goal is not to win a synthetic benchmark at any cost. The goal is sustained throughput,
predictable tail latency, compact storage, rapid recovery, and an operational experience people
trust.

## Where Nova is going

```text
                           Clients and integrations
                 RESP3 · HTTP · Prometheus · native SDKs
                                      |
                                      v
                         Network and command layer
                                      |
                                      v
                    Small, predictable execution core
                           /                    \
                          v                      v
                    Write engine            Read engine
                          \                      /
                           v                    v
                       Mutable in-memory time segments
                                      |
                         seal · compress · checksum
                                      |
                                      v
                    Immutable local storage segments
                                      |
                    compact · retain · downsample · tier
                                      |
                                      v
                         Object storage / deep history

                Background: snapshots · replication · indexing
```

The planned storage engine will group points into bounded segments rather than allocate one object
per sample. Timestamps can be delta-of-delta encoded, numerical values XOR-compressed, and
aggregations pushed down into segment scans. Labels will use an inverted index so queries can find
candidate series without scanning every metric.

Those are design targets, not current claims. Each technique will earn its place through
reproducible benchmarks, failure testing, and documented tradeoffs.

## The road ahead

Nova is being built in deliberate layers:

1. **Durable vertical slice** — WAL-backed writes, restart recovery, time-range reads, TCP server,
   CLI, and correctness tests.
2. **Segmented storage** — bounded memory segments, immutable disk segments, timestamp/value
   compression, snapshots, retention, and background flushing.
3. **Query and indexing** — labels, inverted indexes, aggregations, downsampling, query limits,
   and versioned protocols.
4. **Operational maturity** — benchmarks, observability, backup tooling, corruption handling,
   stable storage compatibility, and production hardening.
5. **Distributed Nova** — streaming replication, failover, partitioning, sharding, and tiered
   object storage after the single-node engine is proven.

See the complete [development roadmap](ROADMAP.md), [architecture](ARCHITECTURE.md), and
[project vision](docs/VISION.md).

## What works today

Nova already has a small end-to-end durable path:

- Validated metric names and timestamped `f64` samples
- A checksummed append-only write-ahead log
- Recovery that rebuilds the in-memory index after restart
- Inclusive, timestamp-ordered range reads
- An asynchronous TCP server and command-line client
- Automated formatting, linting, and tests

The current milestone is intentionally narrow: write a point, persist it, restart Nova, and read
that point back by time range. Building outward from a tested durability boundary gives future
performance work a trustworthy base.

## Quick start

Nova currently requires Rust 1.90 or newer.

Start the server:

```bash
cargo run -p nova-server
```

Then use the CLI from another terminal, either one command at a time:

```bash
cargo run -p nova-cli -- PING
cargo run -p nova-cli -- WRITE cpu.usage 1700000000000 42.5
cargo run -p nova-cli -- RANGE cpu.usage 0 1800000000000
cargo run -p nova-cli -- INFO
```

or interactively, similar to `redis-cli`:

```bash
cargo run -p nova-cli
nova> PING
PONG
nova> WRITE cpu.usage 1700000000000 42.5
OK
```

Nova listens on `127.0.0.1:7422` and stores data under `./nova-data` by default. Use `--listen`
and `--data-dir` to change those values. Pass `--no-banner` to `nova-server` to suppress the
startup banner.

## Current protocol

| Command | Meaning |
| --- | --- |
| `PING` | Check server health |
| `WRITE <metric> <timestamp-ms> <value>` | Durably write one point |
| `RANGE <metric> <start-ms> <end-ms>` | Read an inclusive time range |
| `INFO` | Show version, build, and metric/point counts |

Commands and responses are newline-delimited. This intentionally small protocol gives the engine
a testable interface while its semantics mature. RESP3 and ecosystem-compatible ingestion
interfaces will be evaluated in later milestones.

`WRITE` is an upsert keyed on `(metric, timestamp)`: writing an existing timestamp again replaces
the previously visible value. Points may be written in any timestamp order — Nova does not require
monotonically increasing timestamps per metric — and `RANGE` always returns results in ascending
timestamp order regardless of the order they were written or replayed from the WAL in.

## Workspace

```text
crates/
├── server          Process lifecycle, networking, and scheduling
├── protocol        Wire command and response parsing
├── engine          Write/read semantics and in-memory state
├── storage         Segment and persistence coordination
├── wal             Checksummed append-only log and recovery
├── compression     Timestamp, value, and metadata codecs
├── replication     Replication and failover primitives
├── types           Shared domain types and validation
└── cli             User and operator command-line client
```

The crates are separated around architectural boundaries so storage formats, scheduling, and
protocols can evolve without turning Nova into a monolith.

## Principles for the project

- Correctness before cleverness
- Benchmarks before performance claims
- Failure recovery as a first-class feature
- Stable behavior before broad compatibility
- Small, reviewable changes with documented tradeoffs
- An open design process for consequential architecture decisions

## Join the project

Nova is early enough that thoughtful contributors can shape its foundations. Useful contributions
include storage-format experiments, crash and corruption tests, representative datasets,
benchmarks, documentation, protocol design, and careful reviews.

Start with [CONTRIBUTING.md](CONTRIBUTING.md). Explore the intended applications in
[docs/USE_CASES.md](docs/USE_CASES.md). Large changes to storage, public APIs, protocols, or
architecture should begin with an issue or [RFC](docs/rfcs/README.md) so the reasoning remains
visible to the community.

If the idea of building a database from first principles excites you, Nova has plenty of hard,
worthwhile problems waiting.

## License

Nova is open source under the [MIT License](LICENSE).
