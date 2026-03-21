use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::WalError;
use crate::lsn::{Lsn, INVALID_LSN};
use crate::record::WalRecord;
use crate::writer::{segment_filename, WalWriter};

/// Sequential WAL reader for crash-recovery or replication replay.
pub struct WalReader {
    log_dir: PathBuf,
    /// Ordered list of all known segment numbers.
    segments: Vec<u64>,
    /// Index into `segments` for the segment we are currently reading.
    seg_idx: usize,
    current_file: Option<File>,
    /// Byte offset within the current segment file.
    position: u64,
}

impl WalReader {
    /// Open a reader positioned at the first record whose LSN >= `start_lsn`.
    ///
    /// If `start_lsn` is `INVALID_LSN` (0), reading starts from the very
    /// beginning of the first segment.
    pub fn open(log_dir: &Path, start_lsn: Lsn) -> Result<Self, WalError> {
        let mut segments = WalWriter::list_segments(log_dir)?;
        segments.sort_unstable();

        if segments.is_empty() {
            return Ok(Self {
                log_dir: log_dir.to_path_buf(),
                segments,
                seg_idx: 0,
                current_file: None,
                position: 0,
            });
        }

        // When start_lsn == INVALID_LSN, begin at segment 0 (first segment).
        let seg_idx = if start_lsn == INVALID_LSN {
            0
        } else {
            // Scan each segment to see which one contains the start_lsn.
            // We do a quick forward scan: open each segment, read records until
            // we find one whose lsn >= start_lsn, note the file position before
            // that record.
            Self::find_segment_for_lsn(log_dir, &segments, start_lsn)?
        };

        let mut reader = Self {
            log_dir: log_dir.to_path_buf(),
            segments,
            seg_idx,
            current_file: None,
            position: 0,
        };

        if reader.seg_idx < reader.segments.len() {
            reader.open_segment(reader.seg_idx)?;

            // Skip forward until we reach start_lsn if needed.
            if start_lsn != INVALID_LSN {
                reader.skip_to_lsn(start_lsn)?;
            }
        }

        Ok(reader)
    }

    /// Find the index in `segments` that is most likely to contain `start_lsn`.
    fn find_segment_for_lsn(
        log_dir: &Path,
        segments: &[u64],
        start_lsn: Lsn,
    ) -> Result<usize, WalError> {
        // Walk segments from the end; the first segment whose first LSN <= start_lsn is our target.
        // Simplified: scan all segments, pick the latest one whose records
        // include at least one lsn <= start_lsn.
        let mut best_idx = 0usize;
        for (i, &seg) in segments.iter().enumerate() {
            let path = log_dir.join(segment_filename(seg));
            let mut f = match File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            // Read just the first record to get the first LSN in this segment.
            if let Ok(Some(rec)) = Self::read_one_record_raw(&mut f) {
                if rec.lsn <= start_lsn {
                    best_idx = i;
                }
            }
        }
        Ok(best_idx)
    }

    fn open_segment(&mut self, idx: usize) -> Result<(), WalError> {
        let seg = self.segments[idx];
        let path = self.log_dir.join(segment_filename(seg));
        let mut f = File::open(&path)?;
        f.seek(SeekFrom::Start(self.position))?;
        self.current_file = Some(f);
        Ok(())
    }

    /// Skip records in the current segment until position is at a record with
    /// lsn >= start_lsn or EOF.
    fn skip_to_lsn(&mut self, start_lsn: Lsn) -> Result<(), WalError> {
        loop {
            let f = match &mut self.current_file {
                Some(f) => f,
                None => return Ok(()),
            };
            let pos_before = f.stream_position()?;
            match Self::read_one_record_raw(f) {
                Ok(Some(rec)) => {
                    if rec.lsn >= start_lsn {
                        // Rewind so next_record() will re-read this one.
                        f.seek(SeekFrom::Start(pos_before))?;
                        self.position = pos_before;
                        return Ok(());
                    }
                    // Record is before start_lsn, keep skipping.
                    self.position = f.stream_position()?;
                }
                Ok(None) => return Ok(()), // EOF
                Err(_) => return Ok(()),
            }
        }
    }

    /// Read the next WAL record, crossing segment boundaries automatically.
    ///
    /// Returns `Ok(None)` when the end of all segments is reached.
    pub fn next_record(&mut self) -> Result<Option<WalRecord>, WalError> {
        loop {
            if self.seg_idx >= self.segments.len() {
                return Ok(None);
            }

            // Ensure a file is open.
            if self.current_file.is_none() {
                self.open_segment(self.seg_idx)?;
            }

            let f = self.current_file.as_mut().unwrap();

            // Record current position before the read attempt.
            let pos_before = f.stream_position()?;

            match Self::read_one_record_with_crc(f, pos_before) {
                Ok(Some(rec)) => {
                    self.position = f.stream_position()?;
                    return Ok(Some(rec));
                }
                Ok(None) => {
                    // EOF on this segment; move to the next.
                    self.current_file = None;
                    self.position = 0;
                    self.seg_idx += 1;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Read and validate one record from `f`.  Returns `Ok(None)` at EOF.
    fn read_one_record_with_crc(f: &mut File, pos: u64) -> Result<Option<WalRecord>, WalError> {
        let mut len_buf = [0u8; 4];
        match f.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(WalError::Io(e)),
        }

        let total_len = u32::from_le_bytes(len_buf) as usize;
        if total_len < 4 {
            return Err(WalError::CorruptRecord {
                lsn: INVALID_LSN,
                reason: format!("total_len {} at offset {} is too small", total_len, pos),
            });
        }

        let remaining = total_len - 4;
        let mut rest = vec![0u8; remaining];
        match f.read_exact(&mut rest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(WalError::Io(e)),
        }

        let mut all = Vec::with_capacity(total_len);
        all.extend_from_slice(&len_buf);
        all.extend_from_slice(&rest);

        WalRecord::decode(&all).map(Some)
    }

    /// Read one record without CRC validation (used during initial scan).
    fn read_one_record_raw(f: &mut File) -> Result<Option<WalRecord>, WalError> {
        Self::read_one_record_with_crc(f, 0)
    }
}
