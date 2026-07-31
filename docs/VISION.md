# Nova vision

Nova asks a focused systems question:

> What would Redis look like if it had been designed from day one as a high-performance
> time-series database in Rust?

The answer is not a Redis clone. It is a database that borrows the discipline of a small,
predictable command path while building storage, compression, indexing, queries, and retention
specifically for continuous timestamped data.

## Product promise

Nova aims to make high-volume time-series storage:

- Fast for sustained ingestion and recent reads
- Compact for long histories
- Predictable at the tail, not only impressive at the median
- Simple enough for a small team to operate
- Observable and recoverable when systems fail
- Open enough to embed, extend, benchmark, and learn from

The intended identity is:

> A Redis-inspired, Rust-native, memory-first time-series database optimized for sustained
> append-heavy workloads and predictable low-latency reads.

## Who Nova is for

Nova is for engineers whose systems continuously emit numerical observations: infrastructure
teams, device platforms, industrial systems, financial services, energy networks, and software
vendors that need time-series storage inside their own products.

It is not intended to replace PostgreSQL for relational workloads or Redis for general-purpose
data structures. Its direct comparison set includes dedicated time-series and analytical systems
such as InfluxDB, TimescaleDB, QuestDB, VictoriaMetrics, and ClickHouse, while its architecture
occupies a distinct memory-first and storage-engine-focused position.

## What success looks like

Nova succeeds when:

1. A developer can run a durable local database in minutes.
2. A team can understand its failure and consistency behavior without guesswork.
3. Compression and tiering make long retention economically practical.
4. Label indexing remains controlled under real cardinality.
5. Production claims are reproducible and honest.
6. The open-source project can outlive any single maintainer or company.

## Project strategy

The open-source core should be useful and complete. Potential future commercial work may include a
managed service, enterprise operations, support, security integrations, and hosted tooling, but
the database engine should not be deliberately weakened to create artificial upgrades.

Nova begins in a personal repository while its foundations form. If contributors and users grow,
it can move to a dedicated organization without changing the project's open-source identity.
GitHub repository transfers preserve redirects, making that progression practical.

## Focus

Nova will not try to beat every database at every workload. Each development stage should define
one measurable objective, build the instrumentation required to test it, and publish results with
enough detail to reproduce them.

Early objectives focus on correctness and recovery. Performance leadership, if achieved, must be
earned after those foundations.

