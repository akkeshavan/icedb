use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::HeapError;
use crate::page::{Page, PAGE_SIZE};
use crate::tid::TID;
use crate::tuple::Tuple;

/// A heap file: a flat sequence of fixed-size (8 kB) pages stored on disk.
///
/// Higher layers (buffer pool, executor) use `HeapFile` to perform physical
/// I/O; concurrency control and logging are handled elsewhere.
pub struct HeapFile {
    /// Canonical path to the file on disk.
    pub path: PathBuf,
    /// Open file handle (read + write; created if absent).
    file: File,
}

impl HeapFile {
    /// Open (or create) a heap file at `path`.
    pub fn open(path: &Path) -> Result<Self, HeapError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }

    /// Return the number of pages currently in the file.
    pub fn num_pages(&self) -> u32 {
        let len = self.file.metadata().map(|m| m.len()).unwrap_or(0);
        (len / PAGE_SIZE as u64) as u32
    }

    /// Read page `page_no` from disk into a new `Page`.
    pub fn read_page(&mut self, page_no: u32) -> Result<Page, HeapError> {
        let n = self.num_pages();
        if page_no >= n {
            return Err(HeapError::PageNotFound(page_no));
        }
        let offset = page_no as u64 * PAGE_SIZE as u64;
        let mut buf = [0u8; PAGE_SIZE];
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut buf)?;
        let page = Page::from_bytes(buf);
        // Verify checksum only if the stored checksum is non-zero (0 means
        // the page predates checksumming or has never been written with one).
        if page.checksum() != 0 && !page.verify_checksum() {
            return Err(HeapError::Page(crate::error::PageError::ChecksumMismatch));
        }
        Ok(page)
    }

    /// Write `page` to position `page_no` on disk.
    ///
    /// Does NOT flush — callers that need durability should call [`flush`].
    pub fn write_page(&mut self, page_no: u32, page: &Page) -> Result<(), HeapError> {
        let offset = page_no as u64 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(page.as_bytes())?;
        // Note: no flush here — callers manage durability
        Ok(())
    }

    /// Flush buffered writes to the OS (call after `write_page` when durability is needed).
    pub fn flush(&mut self) -> Result<(), HeapError> {
        self.file.flush()?;
        Ok(())
    }

    /// Extend the file with one new zeroed page and return its page number.
    pub fn allocate_page(&mut self) -> Result<u32, HeapError> {
        let page_no = self.num_pages();
        let page = Page::new();
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(page.as_bytes())?;
        self.file.flush()?;
        Ok(page_no)
    }

    /// Insert a tuple into the first page with enough free space, allocating a
    /// new page if necessary.  Returns the [`TID`] of the inserted tuple.
    pub fn insert_tuple(&mut self, tuple: &Tuple) -> Result<TID, HeapError> {
        let encoded = tuple.encode();
        let needed = encoded.len() + crate::page::ITEM_ID_SIZE; // space for tuple + slot entry

        let n = self.num_pages();
        for page_no in 0..n {
            let mut page = self.read_page(page_no)?;
            if page.free_space() as usize >= needed {
                let slot = page.insert_tuple(&encoded)?;
                self.write_page(page_no, &page)?;
                return Ok(TID::new(page_no, slot));
            }
        }

        // No existing page has enough room — allocate a fresh one.
        let page_no = self.allocate_page()?;
        let mut page = self.read_page(page_no)?;
        let slot = page.insert_tuple(&encoded)?;
        self.write_page(page_no, &page)?;
        Ok(TID::new(page_no, slot))
    }

    /// Find a page with enough free space, insert the encoded tuple into the
    /// in-memory page buffer (WITHOUT writing to disk), and return
    /// `(page_no, modified_page, slot)`.
    ///
    /// This enables callers to write a WAL record BEFORE persisting the page.
    pub fn find_and_prepare_insert(
        &mut self,
        encoded: &[u8],
    ) -> Result<(u32, Page, u16), HeapError> {
        let needed = encoded.len() + crate::page::ITEM_ID_SIZE;
        let n = self.num_pages();
        for page_no in 0..n {
            let mut page = self.read_page(page_no)?;
            if page.free_space() as usize >= needed {
                let slot = page.insert_tuple(encoded)?;
                return Ok((page_no, page, slot));
            }
        }
        // No existing page has room — allocate a fresh one.
        let page_no = self.allocate_page()?;
        let mut page = self.read_page(page_no)?;
        let slot = page.insert_tuple(encoded)?;
        Ok((page_no, page, slot))
    }

    /// Read and decode the tuple at `tid`.
    pub fn get_tuple(&mut self, tid: TID) -> Result<Tuple, HeapError> {
        let page = self.read_page(tid.page_no)?;
        let bytes = page.get_tuple(tid.slot)?;
        Ok(Tuple::decode(bytes)?)
    }

    /// Logically delete the tuple at `tid` (marks its slot as dead).
    pub fn delete_tuple(&mut self, tid: TID) -> Result<(), HeapError> {
        let mut page = self.read_page(tid.page_no)?;
        page.delete_tuple(tid.slot)?;
        self.write_page(tid.page_no, &page)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_heap_insert_and_get() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("heap.db");
        let mut heap = HeapFile::open(&path).unwrap();

        let mut tids = Vec::new();
        for i in 0u32..100 {
            let data = format!("tuple-{i:04}").into_bytes();
            let t = Tuple::new(data);
            tids.push(heap.insert_tuple(&t).unwrap());
        }

        for (i, tid) in tids.iter().enumerate() {
            let got = heap.get_tuple(*tid).unwrap();
            let expected = format!("tuple-{i:04}").into_bytes();
            assert_eq!(got.data, expected, "mismatch at tid {tid}");
        }
    }

    #[test]
    fn test_heap_crosses_page_boundary() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("heap_multi.db");
        let mut heap = HeapFile::open(&path).unwrap();

        // Use 400-byte payloads — about 20 fit per page.
        let payload = vec![0x42u8; 400];
        let mut tids = Vec::new();
        for _ in 0..60 {
            let t = Tuple::new(payload.clone());
            tids.push(heap.insert_tuple(&t).unwrap());
        }

        // Must have spanned at least 2 pages.
        assert!(heap.num_pages() >= 2, "expected multiple pages");

        for tid in &tids {
            let got = heap.get_tuple(*tid).unwrap();
            assert_eq!(got.data, payload);
        }
    }

    #[test]
    fn test_heap_delete() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("heap_del.db");
        let mut heap = HeapFile::open(&path).unwrap();

        let tid = heap
            .insert_tuple(&Tuple::new(b"delete me".to_vec()))
            .unwrap();
        heap.delete_tuple(tid).unwrap();
        assert!(heap.get_tuple(tid).is_err());
    }
}
