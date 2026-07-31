//! Compression boundaries for Nova's future immutable segments.
//!
//! Codecs are intentionally not implemented yet. Delta-of-delta timestamp encoding, Gorilla-style
//! XOR value encoding, run-length encoding, and label dictionaries must be selected using
//! representative datasets and reproducible benchmarks rather than assumed to be universally
//! optimal.

/// Timestamp encodings planned for benchmark evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimestampEncoding {
    /// Store every timestamp directly.
    Plain,
    /// Store changes between consecutive timestamp deltas.
    DeltaOfDelta,
}

/// Numerical value encodings planned for benchmark evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueEncoding {
    /// Store raw IEEE-754 bits.
    Plain,
    /// Store the significant bits of XOR differences between adjacent values.
    GorillaXor,
}
