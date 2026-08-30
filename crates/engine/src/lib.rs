//! Nova's single-node time-series engine.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use nova_storage::{Wal, WalError};
use nova_types::{MetricName, Point};
use thiserror::Error;

/// Durable, in-memory-first time-series engine.
pub struct Engine {
    series: HashMap<MetricName, BTreeMap<i64, f64>>,
    wal: Wal,
    wal_path: PathBuf,
}

/// Engine errors.
#[derive(Debug, Error)]
pub enum EngineError {
    /// Durable storage failed.
    #[error(transparent)]
    Wal(#[from] WalError),
    /// A supplied range was inverted.
    #[error("range start must be less than or equal to range end")]
    InvalidRange,
}

impl EngineError {
    /// A stable, machine-readable identifier for this error, suitable for wire responses and
    /// client-side matching. Unlike the [`std::fmt::Display`] message, this string does not
    /// change across releases.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Wal(error) => error.code(),
            Self::InvalidRange => "INVALID_RANGE",
        }
    }
}

impl Engine {
    /// Opens an engine and rebuilds its in-memory index from the WAL.
    ///
    /// # Errors
    ///
    /// Returns an error if durable storage cannot be opened or recovered.
    pub fn open(data_directory: impl AsRef<Path>) -> Result<Self, EngineError> {
        let wal_path = data_directory.as_ref().join("nova.wal");
        let records = Wal::replay(&wal_path)?;
        let mut series: HashMap<MetricName, BTreeMap<i64, f64>> = HashMap::new();
        for record in records {
            series
                .entry(record.metric)
                .or_default()
                .insert(record.point.timestamp, record.point.value);
        }
        let wal = Wal::open(&wal_path)?;
        Ok(Self {
            series,
            wal,
            wal_path,
        })
    }

    /// Durably writes a point.
    ///
    /// A write is an upsert keyed on `(metric, timestamp)`: writing the same timestamp again
    /// replaces the previously visible value, and the point count does not grow. Points may be
    /// written in any timestamp order — Nova does not require monotonically increasing
    /// timestamps per metric, and [`Engine::range`] always returns results in ascending
    /// timestamp order regardless of the order they were written in or replayed from the WAL.
    ///
    /// # Errors
    ///
    /// Returns an error if the point cannot be appended and synchronized to the WAL.
    pub fn write(&mut self, metric: MetricName, point: &Point) -> Result<(), EngineError> {
        self.wal.append(&metric, point)?;
        self.series
            .entry(metric)
            .or_default()
            .insert(point.timestamp, point.value);
        Ok(())
    }

    /// Returns points inclusively between `start` and `end`, ordered by timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidRange`] when `start` is greater than `end`.
    pub fn range(
        &self,
        metric: &MetricName,
        start: i64,
        end: i64,
    ) -> Result<Vec<Point>, EngineError> {
        if start > end {
            return Err(EngineError::InvalidRange);
        }
        let Some(series) = self.series.get(metric) else {
            return Ok(Vec::new());
        };
        Ok(series
            .range(start..=end)
            .map(|(&timestamp, &value)| Point::new(timestamp, value))
            .collect())
    }

    /// Returns the total number of points currently indexed.
    #[must_use]
    pub fn point_count(&self) -> usize {
        self.series.values().map(BTreeMap::len).sum()
    }

    /// Returns the number of distinct metrics.
    #[must_use]
    pub fn metric_count(&self) -> usize {
        self.series.len()
    }

    /// Returns the active WAL path.
    #[must_use]
    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
    };

    use nova_storage::WalError;
    use nova_types::{MetricName, Point};
    use tempfile::tempdir;

    use super::{Engine, EngineError};

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(EngineError::InvalidRange.code(), "INVALID_RANGE");
        assert_eq!(
            EngineError::from(WalError::InvalidMetric).code(),
            "INVALID_METRIC"
        );
    }

    #[test]
    fn survives_restart_and_reads_ranges() {
        let directory = tempdir().unwrap();
        let metric = MetricName::new("temperature").unwrap();
        {
            let mut engine = Engine::open(directory.path()).unwrap();
            engine
                .write(metric.clone(), &Point::new(100, 20.0))
                .unwrap();
            engine
                .write(metric.clone(), &Point::new(200, 21.5))
                .unwrap();
            engine
                .write(metric.clone(), &Point::new(300, 23.0))
                .unwrap();
        }

        let engine = Engine::open(directory.path()).unwrap();
        assert_eq!(
            engine.range(&metric, 150, 300).unwrap(),
            vec![Point::new(200, 21.5), Point::new(300, 23.0)]
        );
        assert_eq!(engine.point_count(), 3);
    }

    #[test]
    fn duplicate_timestamp_upserts_the_value() {
        let directory = tempdir().unwrap();
        let metric = MetricName::new("temperature").unwrap();
        let mut engine = Engine::open(directory.path()).unwrap();

        engine
            .write(metric.clone(), &Point::new(100, 20.0))
            .unwrap();
        engine
            .write(metric.clone(), &Point::new(100, 99.0))
            .unwrap();

        assert_eq!(
            engine.range(&metric, 0, 200).unwrap(),
            vec![Point::new(100, 99.0)]
        );
        assert_eq!(engine.point_count(), 1);
    }

    #[test]
    fn out_of_order_writes_are_returned_in_timestamp_order() {
        let directory = tempdir().unwrap();
        let metric = MetricName::new("temperature").unwrap();
        let mut engine = Engine::open(directory.path()).unwrap();

        engine.write(metric.clone(), &Point::new(300, 3.0)).unwrap();
        engine.write(metric.clone(), &Point::new(100, 1.0)).unwrap();
        engine.write(metric.clone(), &Point::new(200, 2.0)).unwrap();

        assert_eq!(
            engine.range(&metric, 0, 400).unwrap(),
            vec![
                Point::new(100, 1.0),
                Point::new(200, 2.0),
                Point::new(300, 3.0)
            ]
        );
    }

    #[test]
    fn duplicate_and_out_of_order_writes_survive_restart() {
        let directory = tempdir().unwrap();
        let metric = MetricName::new("temperature").unwrap();
        {
            let mut engine = Engine::open(directory.path()).unwrap();
            engine.write(metric.clone(), &Point::new(300, 3.0)).unwrap();
            engine.write(metric.clone(), &Point::new(100, 1.0)).unwrap();
            engine
                .write(metric.clone(), &Point::new(100, 99.0))
                .unwrap();
            engine.write(metric.clone(), &Point::new(200, 2.0)).unwrap();
        }

        let engine = Engine::open(directory.path()).unwrap();
        assert_eq!(
            engine.range(&metric, 0, 400).unwrap(),
            vec![
                Point::new(100, 99.0),
                Point::new(200, 2.0),
                Point::new(300, 3.0)
            ]
        );
        assert_eq!(engine.point_count(), 3);
    }

    #[test]
    fn repairs_truncated_wal_tail_on_startup() {
        let directory = tempdir().unwrap();
        let metric = MetricName::new("temperature").unwrap();
        {
            let mut engine = Engine::open(directory.path()).unwrap();
            engine.write(metric.clone(), &Point::new(100, 1.0)).unwrap();
            engine.write(metric.clone(), &Point::new(200, 2.0)).unwrap();
        }

        let wal_path = directory.path().join("nova.wal");
        let valid_len = fs::metadata(&wal_path).unwrap().len();
        OpenOptions::new()
            .append(true)
            .open(&wal_path)
            .unwrap()
            .write_all(b"NV")
            .unwrap();

        {
            let mut engine = Engine::open(directory.path()).unwrap();
            assert_eq!(
                engine.range(&metric, 0, 300).unwrap(),
                vec![Point::new(100, 1.0), Point::new(200, 2.0)]
            );
            assert_eq!(fs::metadata(&wal_path).unwrap().len(), valid_len);
            engine.write(metric.clone(), &Point::new(300, 3.0)).unwrap();
        }

        let engine = Engine::open(directory.path()).unwrap();
        assert_eq!(
            engine.range(&metric, 0, 400).unwrap(),
            vec![
                Point::new(100, 1.0),
                Point::new(200, 2.0),
                Point::new(300, 3.0)
            ]
        );
    }
}
