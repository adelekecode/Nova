use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, ErrorKind, Read, Write},
    path::Path,
};

use crc32fast::Hasher;
use nova_types::{MetricName, Point};
use thiserror::Error;

const MAGIC: [u8; 4] = *b"NVW1";
const MAX_PAYLOAD_BYTES: u32 = 1024;

/// An append-only write-ahead log.
pub struct Wal {
    file: File,
}

/// A recovered WAL entry.
#[derive(Clone, Debug, PartialEq)]
pub struct WalRecord {
    /// Metric receiving the point.
    pub metric: MetricName,
    /// Persisted point.
    pub point: Point,
}

/// WAL errors.
#[derive(Debug, Error)]
pub enum WalError {
    /// An I/O operation failed.
    #[error("WAL I/O error: {0}")]
    Io(#[from] io::Error),
    /// A frame failed validation.
    #[error("corrupt WAL frame: {0}")]
    Corrupt(&'static str),
    /// A recovered metric was invalid.
    #[error("corrupt WAL frame: invalid metric name")]
    InvalidMetric,
}

impl Wal {
    /// Opens or creates a WAL file.
    ///
    /// # Errors
    ///
    /// Returns [`WalError::Io`] when the directory or file cannot be opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WalError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
        Ok(Self { file })
    }

    /// Appends and flushes one point before returning.
    ///
    /// # Errors
    ///
    /// Returns an error if the record cannot be encoded, written, or synchronized.
    pub fn append(&mut self, metric: &MetricName, point: &Point) -> Result<(), WalError> {
        let metric_bytes = metric.as_str().as_bytes();
        let metric_len =
            u16::try_from(metric_bytes.len()).map_err(|_| WalError::Corrupt("metric too long"))?;

        let mut payload = Vec::with_capacity(18 + metric_bytes.len());
        payload.extend_from_slice(&metric_len.to_le_bytes());
        payload.extend_from_slice(metric_bytes);
        payload.extend_from_slice(&point.timestamp.to_le_bytes());
        payload.extend_from_slice(&point.value.to_bits().to_le_bytes());

        let payload_len =
            u32::try_from(payload.len()).map_err(|_| WalError::Corrupt("frame too large"))?;
        let checksum = crc32fast::hash(&payload);

        self.file.write_all(&MAGIC)?;
        self.file.write_all(&payload_len.to_le_bytes())?;
        self.file.write_all(&checksum.to_le_bytes())?;
        self.file.write_all(&payload)?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Replays every complete, valid record in a WAL file.
    ///
    /// # Errors
    ///
    /// Returns an error when the WAL cannot be read or contains a corrupt complete frame.
    pub fn replay(path: impl AsRef<Path>) -> Result<Vec<WalRecord>, WalError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let mut reader = BufReader::new(File::open(path)?);
        let mut records = Vec::new();

        loop {
            let mut header = [0_u8; 12];
            match reader.read_exact(&mut header) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
                Err(error) => return Err(error.into()),
            }

            if header[0..4] != MAGIC {
                return Err(WalError::Corrupt("invalid magic bytes"));
            }
            let payload_len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            if payload_len > MAX_PAYLOAD_BYTES {
                return Err(WalError::Corrupt("payload exceeds limit"));
            }
            let expected_checksum =
                u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            let mut payload = vec![0_u8; payload_len as usize];
            reader.read_exact(&mut payload)?;

            let mut hasher = Hasher::new();
            hasher.update(&payload);
            if hasher.finalize() != expected_checksum {
                return Err(WalError::Corrupt("checksum mismatch"));
            }
            records.push(decode_payload(&payload)?);
        }

        Ok(records)
    }
}

fn decode_payload(payload: &[u8]) -> Result<WalRecord, WalError> {
    if payload.len() < 18 {
        return Err(WalError::Corrupt("payload too short"));
    }
    let metric_len = usize::from(u16::from_le_bytes(
        payload[0..2].try_into().expect("fixed slice"),
    ));
    let expected_len = 18 + metric_len;
    if payload.len() != expected_len {
        return Err(WalError::Corrupt("invalid payload length"));
    }
    let metric = std::str::from_utf8(&payload[2..2 + metric_len])
        .map_err(|_| WalError::Corrupt("metric is not UTF-8"))?;
    let timestamp_start = 2 + metric_len;
    let timestamp = i64::from_le_bytes(
        payload[timestamp_start..timestamp_start + 8]
            .try_into()
            .expect("fixed slice"),
    );
    let value_bits = u64::from_le_bytes(
        payload[timestamp_start + 8..timestamp_start + 16]
            .try_into()
            .expect("fixed slice"),
    );

    Ok(WalRecord {
        metric: MetricName::new(metric).map_err(|_| WalError::InvalidMetric)?,
        point: Point::new(timestamp, f64::from_bits(value_bits)),
    })
}

#[cfg(test)]
mod tests {
    use nova_types::{MetricName, Point};
    use tempfile::tempdir;

    use super::Wal;

    #[test]
    fn round_trips_records() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nova.wal");
        let metric = MetricName::new("cpu.usage").unwrap();
        let point = Point::new(1_700_000_000_000, 42.5);

        let mut wal = Wal::open(&path).unwrap();
        wal.append(&metric, &point).unwrap();
        drop(wal);

        let records = Wal::replay(path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].metric, metric);
        assert_eq!(records[0].point, point);
    }
}
