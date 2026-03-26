use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::FileExt;

use crate::error::BTreeError;
use crate::node::PAGE_SIZE;

pub struct BTreeFile {
    pub(crate) file: File,
    #[allow(dead_code)]
    pub(crate) path: PathBuf,
}

impl BTreeFile {
    /// Open or create a BTree index file at the given path.
    pub fn open(path: &Path) -> Result<BTreeFile, BTreeError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        Ok(BTreeFile {
            file,
            path: path.to_path_buf(),
        })
    }

    /// Read a full page from the file using positional I/O (no seek required).
    ///
    /// Takes `&self` (not `&mut self`) so concurrent reads can proceed under a
    /// shared `RwLock` without serializing through a write lock.
    pub fn read_page(&self, page_no: u32) -> Result<[u8; PAGE_SIZE], BTreeError> {
        let offset = page_no as u64 * PAGE_SIZE as u64;
        let mut buf = [0u8; PAGE_SIZE];
        #[cfg(unix)]
        self.file.read_at(&mut buf, offset)?;
        #[cfg(windows)]
        self.file.seek_read(&mut buf, offset)?;
        #[cfg(not(any(unix, windows)))]
        {
            // Fallback: this path requires &mut self; on unsupported platforms
            // callers must obtain a write guard.
            compile_error!("read_page requires unix or windows for positional I/O");
        }
        Ok(buf)
    }

    /// Write a full page to the file.
    pub fn write_page(&mut self, page_no: u32, data: &[u8; PAGE_SIZE]) -> Result<(), BTreeError> {
        let offset = page_no as u64 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)?;
        Ok(())
    }

    /// Extend the file by one page and return the new page number.
    pub fn allocate_page(&mut self) -> Result<u32, BTreeError> {
        let num = self.num_pages()?;
        // Seek to end and write a zeroed page.
        self.file.seek(SeekFrom::End(0))?;
        let zeroed = [0u8; PAGE_SIZE];
        self.file.write_all(&zeroed)?;
        Ok(num)
    }

    /// Return the total number of pages in the file.
    pub fn num_pages(&self) -> Result<u32, BTreeError> {
        let meta = self.file.metadata()?;
        Ok((meta.len() / PAGE_SIZE as u64) as u32)
    }

    /// Fsync the file to disk.
    pub fn sync(&self) -> Result<(), BTreeError> {
        self.file.sync_all()?;
        Ok(())
    }
}
