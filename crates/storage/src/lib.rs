//! Storage coordination for Nova.
//!
//! Today this crate exposes the durable WAL boundary. It will grow into the coordinator for
//! mutable memory segments, immutable disk segments, snapshots, compaction, and retention while
//! keeping their concrete formats in focused crates.

pub use nova_wal::{Wal, WalError, WalRecord};
