//! Shared domain types used across Nova.

use std::fmt;

/// Maximum accepted metric-name length, in bytes.
pub const MAX_METRIC_NAME_BYTES: usize = 255;

/// A single time-series sample.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
    /// Floating-point sample value.
    pub value: f64,
}

impl Point {
    /// Creates a point.
    #[must_use]
    pub const fn new(timestamp: i64, value: f64) -> Self {
        Self { timestamp, value }
    }
}

/// A validated metric name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MetricName(String);

impl MetricName {
    /// Creates a metric name containing ASCII letters, digits, `_`, `-`, `.`, or `:`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidMetricName`] when the value is empty, longer than 255 bytes, or contains
    /// unsupported characters.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidMetricName> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_METRIC_NAME_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-.:".contains(&byte))
        {
            return Err(InvalidMetricName);
        }
        Ok(Self(value))
    }

    /// Returns the metric name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MetricName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Error returned when a metric name is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidMetricName;

impl fmt::Display for InvalidMetricName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "metric names must be 1-255 characters and contain only letters, digits, _, -, ., or :",
        )
    }
}

impl std::error::Error for InvalidMetricName {}

#[cfg(test)]
mod tests {
    use super::{MAX_METRIC_NAME_BYTES, MetricName};

    #[test]
    fn validates_metric_names() {
        assert!(MetricName::new("system.cpu:usage").is_ok());
        assert!(MetricName::new("").is_err());
        assert!(MetricName::new("spaces are invalid").is_err());
    }

    #[test]
    fn enforces_metric_name_length_limit() {
        assert!(MetricName::new("a".repeat(MAX_METRIC_NAME_BYTES)).is_ok());
        assert!(MetricName::new("a".repeat(MAX_METRIC_NAME_BYTES + 1)).is_err());
    }
}
