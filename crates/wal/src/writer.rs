use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use crate::error::WalError;
use crate::lsn::{Lsn, INVALID_LSN};
use crate::record::{WalRecord, WalRecordType};

/// Default maximum size of a single WAL segment file: 16 MiB.
pub const DEFAULT_SEGMENT_SIZE: u64 = 16 * 1024 * 1024;

/// Format a segment number into a WAL file name.
pub fn segment_filename(seg: u64) -> String {
    format!("{:016}.wal", seg)
}

// ── Inner state (protected by Mutex) ─────────────────────────────────────────

struct Inner {
    current_file: File,
    current_lsn: Lsn,
    prev_lsn: Lsn,
    current_segment: u64,
    segment_size: u64,
    log_dir: PathBuf,
}

impl Inner {
    /// Rotate to a new segment if the current file has exceeded `segment_size`.
    fn rotate_segment_if_needed(&mut self) -> Result<(), WalError> {
        let pos = self.current_file.stream_position()?;
        if pos >= self.segment_size {
            self.current_segment += 1;
            let new_path = self.log_dir.join(segment_filename(self.current_segment));
            let new_file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&new_path)?;
            self.current_file = new_file;
            log::debug!("WAL segment rotated → {}", new_path.display());
        }
        Ok(())
    }

    fn append_inner(
        &mut self,
        xid: u32,
        record_type: WalRecordType,
        page_no: u32,
        data: Vec<u8>,
    ) -> Result<Lsn, WalError> {
        self.rotate_segment_if_needed()?;

        let lsn = self.current_lsn + 1;
        let mut record = WalRecord {
            lsn,
            prev_lsn: self.prev_lsn,
            xid,
            record_type,
            page_no,
            data,
        };
        record.lsn = lsn;

        let encoded = record.encode();
        self.current_file.write_all(&encoded)?;

        self.current_lsn = lsn;
        self.prev_lsn = lsn;
        Ok(lsn)
    }
}

// ── WalWriter ─────────────────────────────────────────────────────────────────

/// Sequential, append-only WAL writer.
///
/// All public methods are thread-safe (`Send + Sync`) — concurrent callers
/// serialise through the internal `Mutex`.
pub struct WalWriter {
    inner: Mutex<Inner>,
}

impl WalWriter {
    /// Open (or create) the WAL at `log_dir`.
    ///
    /// Scans existing segment files to find the highest LSN so that new records
    /// receive monotonically increasing sequence numbers.
    pub fn open(log_dir: &Path) -> Result<Self, WalError> {
        fs::create_dir_all(log_dir)?;

        // Enumerate existing segment files, sorted ascending.
        let mut segments: Vec<u64> = Self::list_segments(log_dir)?;

        let (current_segment, current_lsn, prev_lsn, file) = if segments.is_empty() {
            // Start fresh at segment 1.
            let seg = 1u64;
            let path = log_dir.join(segment_filename(seg));
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)?;
            (seg, INVALID_LSN, INVALID_LSN, f)
        } else {
            // Scan through all segments in order; scan the last one in full to
            // find the highest LSN.
            segments.sort_unstable();
            let last_seg = *segments
                .last()
                .expect("segments is non-empty; checked above");

            // Scan all segments to find the highest LSN.
            let (max_lsn, prev_lsn) = Self::scan_segments(log_dir, &segments)?;

            let path = log_dir.join(segment_filename(last_seg));
            let mut f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)?;
            // Seek to end so new records are appended.
            f.seek(SeekFrom::End(0))?;

            (last_seg, max_lsn, prev_lsn, f)
        };

        Ok(Self {
            inner: Mutex::new(Inner {
                current_file: file,
                current_lsn,
                prev_lsn,
                current_segment,
                segment_size: DEFAULT_SEGMENT_SIZE,
                log_dir: log_dir.to_path_buf(),
            }),
        })
    }

    /// Open with an explicit segment size (useful for testing rotation).
    pub fn open_with_segment_size(log_dir: &Path, segment_size: u64) -> Result<Self, WalError> {
        let writer = Self::open(log_dir)?;
        writer.inner.lock().segment_size = segment_size;
        Ok(writer)
    }

    /// Returns the sorted list of segment numbers in `log_dir`.
    pub fn list_segments(log_dir: &Path) -> Result<Vec<u64>, WalError> {
        let mut segs = Vec::new();
        for entry in fs::read_dir(log_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".wal") {
                if let Ok(n) = name_str.trim_end_matches(".wal").parse::<u64>() {
                    segs.push(n);
                }
            }
        }
        segs.sort_unstable();
        Ok(segs)
    }

    /// Scan all segment files in order to find the highest LSN and the
    /// second-highest LSN (which becomes `prev_lsn` for the first new record).
    fn scan_segments(log_dir: &Path, segments: &[u64]) -> Result<(Lsn, Lsn), WalError> {
        let mut max_lsn = INVALID_LSN;
        let mut prev_lsn = INVALID_LSN;

        for &seg in segments {
            let path = log_dir.join(segment_filename(seg));
            let mut f = match File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            loop {
                match Self::read_next_record_from(&mut f) {
                    Ok(Some(rec)) => {
                        prev_lsn = max_lsn;
                        max_lsn = rec.lsn;
                    }
                    Ok(None) => break,
                    Err(_) => break, // stop at first corrupt/incomplete record
                }
            }
        }

        Ok((max_lsn, prev_lsn))
    }

    /// Attempt to read one record from `f` without using the full WalReader.
    fn read_next_record_from(f: &mut File) -> Result<Option<WalRecord>, WalError> {
        // Read the 4-byte total_len prefix.
        let mut len_buf = [0u8; 4];
        match f.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(WalError::Io(e)),
        }
        let total_len = u32::from_le_bytes(len_buf) as usize;
        if total_len < 4 {
            return Ok(None);
        }

        let remaining = total_len - 4;
        let mut rest = vec![0u8; remaining];
        f.read_exact(&mut rest)?;

        let mut all = Vec::with_capacity(total_len);
        all.extend_from_slice(&len_buf);
        all.extend_from_slice(&rest);

        match WalRecord::decode(&all) {
            Ok(rec) => Ok(Some(rec)),
            Err(_) => Ok(None),
        }
    }

    /// Append a WAL record and return its assigned LSN.
    ///
    /// The record is written to the OS page cache but NOT necessarily fsynced.
    /// Call [`flush`] or [`append_and_flush`] for durable writes.
    pub fn append(
        &self,
        xid: u32,
        record_type: WalRecordType,
        page_no: u32,
        data: Vec<u8>,
    ) -> Result<Lsn, WalError> {
        self.inner
            .lock()
            .append_inner(xid, record_type, page_no, data)
    }

    /// Fsync the current WAL segment to disk.
    pub fn flush(&self) -> Result<(), WalError> {
        self.inner.lock().current_file.sync_all()?;
        Ok(())
    }

    /// Append + fsync in one call.  Use this for COMMIT records.
    pub fn append_and_flush(
        &self,
        xid: u32,
        record_type: WalRecordType,
        page_no: u32,
        data: Vec<u8>,
    ) -> Result<Lsn, WalError> {
        let lsn = self
            .inner
            .lock()
            .append_inner(xid, record_type, page_no, data)?;
        self.flush()?;
        Ok(lsn)
    }

    /// Return the LSN that will be assigned to the next appended record.
    pub fn next_lsn(&self) -> Lsn {
        self.inner.lock().current_lsn + 1
    }

    /// Return the highest LSN written so far.
    pub fn current_lsn(&self) -> Lsn {
        self.inner.lock().current_lsn
    }
}
