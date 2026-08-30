//! Nova's intentionally small v0.1 text protocol.

use nova_types::MetricName;
use thiserror::Error;

/// One parsed write operation.
#[derive(Clone, Debug, PartialEq)]
pub struct WriteCommand {
    /// Target metric.
    pub metric: MetricName,
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
    /// Sample value.
    pub value: f64,
}

/// A parsed client command.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Health check.
    Ping,
    /// Persist a point.
    Write {
        /// Target metric.
        metric: MetricName,
        /// Unix timestamp in milliseconds.
        timestamp: i64,
        /// Sample value.
        value: f64,
    },
    /// Persist multiple points atomically.
    Batch {
        /// Writes to apply as one durable batch.
        writes: Vec<WriteCommand>,
    },
    /// Read an inclusive time range.
    Range {
        /// Target metric.
        metric: MetricName,
        /// Inclusive lower bound.
        start: i64,
        /// Inclusive upper bound.
        end: i64,
    },
    /// Engine statistics.
    Info,
}

/// Protocol parsing errors.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum ParseError {
    /// The command is empty or unknown.
    #[error("unknown command")]
    UnknownCommand,
    /// A command has the wrong number of arguments.
    #[error("wrong number of arguments")]
    WrongArity,
    /// The metric name is invalid.
    #[error("invalid metric name")]
    InvalidMetric,
    /// A number could not be parsed.
    #[error("invalid number")]
    InvalidNumber,
}

impl ParseError {
    /// A stable, machine-readable identifier for this error, suitable for wire responses and
    /// client-side matching. Unlike the [`std::fmt::Display`] message, this string does not
    /// change across releases.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownCommand => "UNKNOWN_COMMAND",
            Self::WrongArity => "WRONG_ARITY",
            Self::InvalidMetric => "INVALID_METRIC",
            Self::InvalidNumber => "INVALID_NUMBER",
        }
    }
}

/// Parses a single newline-delimited command.
///
/// # Errors
///
/// Returns [`ParseError`] when the command, arity, metric, or numeric arguments are invalid.
pub fn parse(input: &str) -> Result<Command, ParseError> {
    let parts: Vec<_> = input.split_ascii_whitespace().collect();
    let Some(name) = parts.first() else {
        return Err(ParseError::UnknownCommand);
    };

    match name.to_ascii_uppercase().as_str() {
        "PING" if parts.len() == 1 => Ok(Command::Ping),
        "INFO" if parts.len() == 1 => Ok(Command::Info),
        "WRITE" if parts.len() == 4 => Ok(Command::Write {
            metric: MetricName::new(parts[1]).map_err(|_| ParseError::InvalidMetric)?,
            timestamp: parts[2].parse().map_err(|_| ParseError::InvalidNumber)?,
            value: parts[3].parse().map_err(|_| ParseError::InvalidNumber)?,
        }),
        "BATCH" if parts.len() >= 4 && (parts.len() - 1) % 3 == 0 => {
            let writes = parts[1..]
                .chunks_exact(3)
                .map(|chunk| {
                    Ok(WriteCommand {
                        metric: MetricName::new(chunk[0]).map_err(|_| ParseError::InvalidMetric)?,
                        timestamp: chunk[1].parse().map_err(|_| ParseError::InvalidNumber)?,
                        value: chunk[2].parse().map_err(|_| ParseError::InvalidNumber)?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Command::Batch { writes })
        }
        "RANGE" if parts.len() == 4 => Ok(Command::Range {
            metric: MetricName::new(parts[1]).map_err(|_| ParseError::InvalidMetric)?,
            start: parts[2].parse().map_err(|_| ParseError::InvalidNumber)?,
            end: parts[3].parse().map_err(|_| ParseError::InvalidNumber)?,
        }),
        "PING" | "INFO" | "WRITE" | "BATCH" | "RANGE" => Err(ParseError::WrongArity),
        _ => Err(ParseError::UnknownCommand),
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, ParseError, parse};

    #[test]
    fn parses_write_case_insensitively() {
        let command = parse("write cpu.usage 1000 42.5").unwrap();
        assert!(matches!(
            command,
            Command::Write {
                timestamp: 1000,
                value: 42.5,
                ..
            }
        ));
    }

    #[test]
    fn rejects_bad_arity() {
        assert_eq!(parse("RANGE cpu 1"), Err(ParseError::WrongArity));
        assert_eq!(parse("BATCH cpu 1"), Err(ParseError::WrongArity));
    }

    #[test]
    fn parses_batch_writes_atomically() {
        let command = parse("BATCH cpu.usage 1000 42.5 mem.used 1000 12").unwrap();
        let Command::Batch { writes } = command else {
            panic!("expected batch command");
        };

        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].metric.as_str(), "cpu.usage");
        assert_eq!(writes[0].timestamp, 1000);
        assert_eq!(writes[0].value.to_bits(), 42.5_f64.to_bits());
        assert_eq!(writes[1].metric.as_str(), "mem.used");
        assert_eq!(writes[1].timestamp, 1000);
        assert_eq!(writes[1].value.to_bits(), 12.0_f64.to_bits());
    }

    #[test]
    fn rejects_invalid_batch_members() {
        assert_eq!(
            parse("BATCH cpu.usage 1000 42.5 bad/name 1000 12"),
            Err(ParseError::InvalidMetric)
        );
        assert_eq!(
            parse("BATCH cpu.usage nope 42.5"),
            Err(ParseError::InvalidNumber)
        );
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(ParseError::UnknownCommand.code(), "UNKNOWN_COMMAND");
        assert_eq!(ParseError::WrongArity.code(), "WRONG_ARITY");
        assert_eq!(ParseError::InvalidMetric.code(), "INVALID_METRIC");
        assert_eq!(ParseError::InvalidNumber.code(), "INVALID_NUMBER");
    }
}
