# Nova benchmarks

This directory will contain reproducible microbenchmarks, workload generators, dataset fixtures,
and end-to-end comparison harnesses.

Every published result must record:

- Nova commit and build profile
- Hardware, operating system, filesystem, and storage device
- Dataset shape, cardinality, ordering, and compression characteristics
- Durability settings and synchronization policy
- Client concurrency, batch size, and request protocol
- Throughput plus p50, p95, p99, and p999 latency
- CPU, memory, disk use, database size, and recovery time
- Competitor version and equivalent configuration for comparisons

Benchmark work begins before storage optimization so changes can be evaluated against a stable
baseline.

