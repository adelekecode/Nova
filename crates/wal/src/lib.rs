use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, ErrorKind, Read, Write},
    path::Path,
};

use crc32fast::Hasher;
use nova_types::{MetricName, Point};
use thiserror::Error;

const MAGIC: [u8; 4] = *b"NVW1";
const HEADER_BYTES: usize = 12;
/// Maximum WAL frame payload length, in bytes.
pub const MAX_PAYLOAD_BYTES: u32 = 1024;

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

impl WalError {
    /// A stable, machine-readable identifier for this error, suitable for wire responses and
    /// client-side matching. Unlike the [`std::fmt::Display`] message, this string does not
    /// change across releases.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "IO",
            Self::Corrupt(_) => "CORRUPT",
            Self::InvalidMetric => "INVALID_METRIC",
        }
    }
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
        self.flush()?;
        Ok(())
    }

    /// Flushes pending WAL writes through Nova's durability boundary.
    ///
    /// # Errors
    ///
    /// Returns an error if buffered data cannot be flushed or synchronized.
    pub fn flush(&mut self) -> Result<(), WalError> {
        self.file.flush()?;
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
        let mut valid_len = 0_u64;

        loop {
            let mut header = [0_u8; HEADER_BYTES];
            if !read_exact_or_repair_tail(&mut reader, &mut header, path, valid_len, true)? {
                break;
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
            if !read_exact_or_repair_tail(&mut reader, &mut payload, path, valid_len, false)? {
                break;
            }

            let mut hasher = Hasher::new();
            hasher.update(&payload);
            if hasher.finalize() != expected_checksum {
                return Err(WalError::Corrupt("checksum mismatch"));
            }
            records.push(decode_payload(&payload)?);
            valid_len += HEADER_BYTES as u64 + u64::from(payload_len);
        }

        Ok(records)
    }
}

fn read_exact_or_repair_tail(
    reader: &mut impl Read,
    buffer: &mut [u8],
    path: &Path,
    valid_len: u64,
    clean_eof_at_boundary: bool,
) -> Result<bool, WalError> {
    let mut read = 0;
    while read < buffer.len() {
        match reader.read(&mut buffer[read..]) {
            Ok(0) if read == 0 && clean_eof_at_boundary => return Ok(false),
            Ok(0) => {
                repair_tail(path, valid_len)?;
                return Ok(false);
            }
            Ok(bytes) => read += bytes,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

fn repair_tail(path: &Path, valid_len: u64) -> Result<(), WalError> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(valid_len)?;
    file.sync_data()?;
    Ok(())
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
    use std::{
        fs::{self, File, OpenOptions},
        io::{Seek, SeekFrom, Write},
    };

    use nova_types::{MetricName, Point};
    use tempfile::tempdir;

    use super::{HEADER_BYTES, MAGIC, MAX_PAYLOAD_BYTES, Wal, WalError};

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(WalError::Corrupt("bad frame").code(), "CORRUPT");
        assert_eq!(WalError::InvalidMetric.code(), "INVALID_METRIC");
        let io_error = std::io::Error::other("disk full");
        assert_eq!(WalError::Io(io_error).code(), "IO");
    }

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

    #[test]
    fn repairs_truncated_header_tail() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nova.wal");
        let metric = MetricName::new("cpu.usage").unwrap();

        let mut wal = Wal::open(&path).unwrap();
        wal.append(&metric, &Point::new(100, 1.5)).unwrap();
        wal.append(&metric, &Point::new(200, 2.5)).unwrap();
        drop(wal);
        let valid_len = fs::metadata(&path).unwrap().len();

        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&MAGIC[0..2])
            .unwrap();

        let records = Wal::replay(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(fs::metadata(&path).unwrap().len(), valid_len);

        let mut wal = Wal::open(&path).unwrap();
        wal.append(&metric, &Point::new(300, 3.5)).unwrap();
        drop(wal);

        let records = Wal::replay(&path).unwrap();
        assert_eq!(
            records
                .iter()
                .map(|record| record.point.clone())
                .collect::<Vec<_>>(),
            vec![
                Point::new(100, 1.5),
                Point::new(200, 2.5),
                Point::new(300, 3.5)
            ]
        );
    }

    #[test]
    fn repairs_truncated_payload_tail() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nova.wal");
        let metric = MetricName::new("cpu.usage").unwrap();

        let mut wal = Wal::open(&path).unwrap();
        wal.append(&metric, &Point::new(100, 1.5)).unwrap();
        drop(wal);
        let valid_len = fs::metadata(&path).unwrap().len();

        let mut wal = Wal::open(&path).unwrap();
        wal.append(&metric, &Point::new(200, 2.5)).unwrap();
        drop(wal);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(valid_len + HEADER_BYTES as u64 + 3)
            .unwrap();

        let records = Wal::replay(&path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].point, Point::new(100, 1.5));
        assert_eq!(fs::metadata(&path).unwrap().len(), valid_len);
    }

    #[test]
    fn complete_corrupt_frames_fail_without_repair() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nova.wal");
        let metric = MetricName::new("cpu.usage").unwrap();

        let mut wal = Wal::open(&path).unwrap();
        wal.append(&metric, &Point::new(100, 1.5)).unwrap();
        drop(wal);
        let valid_len = fs::metadata(&path).unwrap().len();

        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(HEADER_BYTES as u64)).unwrap();
        file.write_all(&[0xFF]).unwrap();
        file.sync_data().unwrap();
        drop(file);

        let error = Wal::replay(&path).unwrap_err();
        assert!(matches!(error, WalError::Corrupt("checksum mismatch")));
        assert_eq!(fs::metadata(&path).unwrap().len(), valid_len);
    }

    #[test]
    fn rejects_frames_over_the_payload_limit() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nova.wal");
        let mut file = File::create(&path).unwrap();
        file.write_all(&MAGIC).unwrap();
        file.write_all(&(MAX_PAYLOAD_BYTES + 1).to_le_bytes())
            .unwrap();
        file.write_all(&0_u32.to_le_bytes()).unwrap();
        file.sync_data().unwrap();
        drop(file);

        let error = Wal::replay(&path).unwrap_err();
        assert!(matches!(error, WalError::Corrupt("payload exceeds limit")));
    }
}
