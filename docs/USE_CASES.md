# Nova use cases

Time-series data is any stream of observations whose timestamp and evolution matter. Nova is
designed for workloads that append continuously, scan by time range, aggregate over windows, and
retain far more history than can fit in memory.

## Infrastructure and AI

Cloud servers, Kubernetes clusters, and AI accelerators continuously report CPU, memory, disk,
network, pod restarts, GPU utilization, VRAM, temperature, power, token throughput, request
latency, and errors. Nova could underpin observability, capacity planning, anomaly detection, and
model-training operations.

## IoT and smart devices

Smart meters, environmental sensors, appliances, and embedded devices emit voltage, current,
power, temperature, location, and state. One million devices reporting once per second produce
86.4 billion samples per day, making compression, retention, batching, and predictable ingestion
central requirements.

## Financial markets

Market ticks contain price, volume, bid, ask, and venue data. Use cases include market replay,
risk analysis, quantitative research, exchange monitoring, and algorithmic trading. These
workloads require high ingest rates, careful ordering semantics, and efficient scans over narrow
and broad time windows.

## Industrial manufacturing

Factories observe vibration, temperature, pressure, RPM, conveyor speed, and power consumption.
Historical trends support predictive maintenance, quality analysis, process optimization, and
failure investigation.

## Energy systems

Grids, solar installations, wind farms, batteries, and charging networks measure generation,
charge, grid frequency, voltage, current, and equipment condition. Operators need both live
visibility and compressed long-term history.

## Telecommunications

Networks track signal strength, packet loss, latency, call failures, throughput, tower health, and
subscriber experience. Time-window aggregation and label filtering help isolate regional,
equipment, and carrier problems.

## Transportation and autonomous systems

Vehicles and fleets produce GPS, speed, steering, battery, diagnostic, and sensor metadata.
Efficient telemetry storage enables replay, maintenance, safety analysis, and fleet optimization.
Large binary camera or LiDAR payloads are not Nova's primary storage target; Nova can store their
timestamped metadata and references.

## Healthcare and wearables

Wearables and medical equipment measure heart rate, oxygen saturation, blood pressure, glucose,
and other signals. This domain also requires strong privacy, security, audit, and regulatory
controls; Nova should not claim suitability until those capabilities are designed and validated.

## Smart cities

Traffic, air quality, noise, weather, flooding, utilities, and parking sensors create continuous
city-scale observations. Retention and downsampling allow immediate operations and long-term
planning to use the same data foundation.

## Gaming and interactive systems

Games emit server tick rate, latency, FPS, player position, match events, and infrastructure
health. Nova could support live operations, balancing analysis, replay metadata, and performance
diagnostics.

## Retail and restaurants

Point-of-sale and kitchen systems can measure orders per minute, preparation time, queue depth,
payment latency, terminal health, inventory movement, and customer wait time. Across many
locations, these series enable real-time operational dashboards and trend analysis.

## Embedded analytics for software vendors

SaaS and infrastructure products often need metrics storage but do not want customers to operate a
separate database. A future embeddable Nova engine could provide a focused, Rust-native telemetry
layer inside those products.

## Why a purpose-built engine?

At one sample per second, one million sources create:

- 1 million points per second
- 60 million points per minute
- 3.6 billion points per hour
- 86.4 billion points per day

Traditional row-oriented databases can serve time-series workloads, especially with careful
partitioning and extensions, but the cost profile changes at this scale. A purpose-built engine
can store time-adjacent values sequentially, compress repeated patterns, prune by time, enforce
retention, and push aggregates into scans.

Nova must prove those benefits with realistic comparisons rather than assuming specialization
automatically wins.

