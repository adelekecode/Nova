//! Replication protocol primitives.
//!
//! Nova will not implement distributed operation until the single-node log, recovery, and storage
//! semantics are stable. This crate reserves a dependency boundary for ordered log streaming,
//! replica state, snapshots, and failover without coupling those concerns to the engine.

/// Monotonic position in Nova's future replication stream.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReplicationOffset(u64);

impl ReplicationOffset {
    /// The beginning of a replication stream.
    pub const ZERO: Self = Self(0);

    /// Creates an offset from its wire representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the offset's wire representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
