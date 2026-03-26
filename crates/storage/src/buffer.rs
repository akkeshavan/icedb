use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

use crate::error::BufferError;
use crate::heap::HeapFile;
use crate::page::Page;

/// Maximum number of buffer frames allowed (128 k frames × 8 kB = 1 GB).
pub const MAX_BUFFER_FRAMES: usize = 131_072;

// ── BufferDescriptor ──────────────────────────────────────────────────────────

/// Metadata for a single buffer frame.
#[derive(Debug)]
pub struct BufferDescriptor {
    /// Page currently loaded into this frame (`None` = free frame).
    pub page_no: Option<u32>,
    /// Whether the frame has been modified since it was last flushed.
    pub dirty: bool,
    /// Number of callers that have pinned this frame.
    pub pin_count: u32,
    /// Clock-sweep reference count; incremented on access, decremented on sweep.
    pub usage_count: u8,
}

impl BufferDescriptor {
    fn new() -> Self {
        Self {
            page_no: None,
            dirty: false,
            pin_count: 0,
            usage_count: 0,
        }
    }
}

// ── BufferPool ────────────────────────────────────────────────────────────────

/// A fixed-capacity buffer pool with Clock-sweep page replacement.
///
/// The pool maintains `num_frames` in-memory frames backed by a [`HeapFile`].
/// Callers pin a page before accessing it and unpin it afterwards; the pool
/// evicts pages with a usage count of zero when it needs a free frame.
pub struct BufferPool {
    /// In-memory page frames.
    frames: Vec<RwLock<Page>>,
    /// Per-frame metadata.
    descriptors: Vec<Mutex<BufferDescriptor>>,
    /// Current position of the clock hand (for the sweep eviction algorithm).
    clock_hand: AtomicUsize,
    /// The underlying heap file shared with other components.
    heap: Arc<Mutex<HeapFile>>,
    /// O(1) lookup: page_no → frame index.
    page_table: Mutex<HashMap<u32, usize>>,
}

impl BufferPool {
    /// Create a buffer pool with `num_frames` frames backed by `heap`.
    ///
    /// `num_frames` is silently capped at [`MAX_BUFFER_FRAMES`] so that a
    /// misconfigured caller cannot exhaust memory.
    pub fn new(heap: Arc<Mutex<HeapFile>>, num_frames: usize) -> Self {
        let num_frames = num_frames.min(MAX_BUFFER_FRAMES);
        assert!(num_frames > 0, "buffer pool must have at least one frame");
        let mut frames = Vec::with_capacity(num_frames);
        let mut descriptors = Vec::with_capacity(num_frames);
        for _ in 0..num_frames {
            frames.push(RwLock::new(Page::new()));
            descriptors.push(Mutex::new(BufferDescriptor::new()));
        }
        Self {
            frames,
            descriptors,
            clock_hand: AtomicUsize::new(0),
            heap,
            page_table: Mutex::new(HashMap::new()),
        }
    }

    /// Pin `page_no`, loading it from disk if it is not already cached.
    ///
    /// Returns the frame index.  The caller **must** call [`unpin_page`] when
    /// done.
    pub fn pin_page(&self, page_no: u32) -> Result<usize, BufferError> {
        // Fast path: O(1) lookup via page_table.
        {
            let pt = self.page_table.lock();
            if let Some(&i) = pt.get(&page_no) {
                let mut desc = self.descriptors[i].lock();
                if desc.page_no == Some(page_no) {
                    desc.pin_count += 1;
                    desc.usage_count = desc.usage_count.saturating_add(1);
                    return Ok(i);
                }
                // page_table is stale (evicted); fall through to slow path below
            }
        }

        // Slow path: need to load from disk — first find a victim frame.
        let frame_idx = self.evict_frame()?;

        // Load the page from disk.
        let page = {
            let mut heap = self.heap.lock();
            // If the heap file doesn't have this page yet, allocate it first.
            let existing = heap.num_pages();
            if page_no >= existing {
                while heap.num_pages() <= page_no {
                    heap.allocate_page()?;
                }
            }
            heap.read_page(page_no)?
        };

        // Install the page into the evicted frame.
        {
            let mut frame = self.frames[frame_idx].write();
            *frame = page;
        }
        {
            let mut desc = self.descriptors[frame_idx].lock();
            desc.page_no = Some(page_no);
            desc.dirty = false;
            desc.pin_count = 1;
            desc.usage_count = 1;
        }
        // Register in the page table for O(1) future lookups.
        {
            let mut pt = self.page_table.lock();
            pt.insert(page_no, frame_idx);
        }

        Ok(frame_idx)
    }

    /// Decrement the pin count for `frame_idx` and optionally mark it dirty.
    pub fn unpin_page(&self, frame_idx: usize, dirty: bool) {
        let mut desc = self.descriptors[frame_idx].lock();
        if desc.pin_count > 0 {
            desc.pin_count -= 1;
        }
        if dirty {
            desc.dirty = true;
        }
    }

    /// Flush the page in `frame_idx` to disk if it is dirty.
    pub fn flush_frame(&self, frame_idx: usize) -> Result<(), BufferError> {
        let (page_no, is_dirty) = {
            let desc = self.descriptors[frame_idx].lock();
            (desc.page_no, desc.dirty)
        };
        if is_dirty {
            if let Some(pno) = page_no {
                let frame = self.frames[frame_idx].read();
                let mut heap = self.heap.lock();
                heap.write_page(pno, &frame)?;
            }
            let mut desc = self.descriptors[frame_idx].lock();
            desc.dirty = false;
        }
        Ok(())
    }

    /// Clock-sweep eviction: find an unpinned frame to evict.
    ///
    /// On each tick the algorithm decrements `usage_count`; a frame with
    /// `usage_count == 0` and `pin_count == 0` is chosen as the victim.
    pub fn evict_frame(&self) -> Result<usize, BufferError> {
        let n = self.frames.len();
        // Two full sweeps to ensure every frame has a chance to be decremented.
        for _ in 0..n * 2 {
            let hand = self.clock_hand.fetch_add(1, Ordering::Relaxed) % n;
            let mut desc = self.descriptors[hand].lock();
            if desc.pin_count > 0 {
                continue;
            }
            if desc.usage_count > 0 {
                desc.usage_count -= 1;
                continue;
            }
            // Victim found — flush if dirty.
            if desc.dirty {
                if let Some(pno) = desc.page_no {
                    let frame = self.frames[hand].read();
                    let mut heap = self.heap.lock();
                    heap.write_page(pno, &frame).map_err(BufferError::Heap)?;
                }
                desc.dirty = false;
            }
            // Remove from page table.
            if let Some(pno) = desc.page_no {
                let mut pt = self.page_table.lock();
                pt.remove(&pno);
            }
            desc.page_no = None;
            return Ok(hand);
        }
        Err(BufferError::AllFramesPinned)
    }

    /// Flush every dirty frame to disk.
    pub fn flush_all_dirty(&self) -> Result<(), BufferError> {
        for i in 0..self.frames.len() {
            self.flush_frame(i)?;
        }
        Ok(())
    }

    /// Return the number of frames in the pool.
    pub fn num_frames(&self) -> usize {
        self.frames.len()
    }

    /// Provide direct read access to a frame's page (caller must hold a pin).
    pub fn read_frame<F, R>(&self, frame_idx: usize, f: F) -> R
    where
        F: FnOnce(&Page) -> R,
    {
        let guard = self.frames[frame_idx].read();
        f(&guard)
    }

    /// Provide direct write access to a frame's page (caller must hold a pin).
    pub fn write_frame<F, R>(&self, frame_idx: usize, f: F) -> R
    where
        F: FnOnce(&mut Page) -> R,
    {
        let mut guard = self.frames[frame_idx].write();
        f(&mut guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    use crate::heap::HeapFile;
    use crate::page::PAGE_SIZE;
    use crate::tuple::Tuple;

    fn make_pool(num_frames: usize) -> (BufferPool, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("buf.db");
        let heap = Arc::new(Mutex::new(HeapFile::open(&path).unwrap()));
        let pool = BufferPool::new(heap, num_frames);
        (pool, dir)
    }

    #[test]
    fn test_buffer_pool_pin_unpin() {
        let (pool, _dir) = make_pool(4);

        // Pre-allocate page 0 by inserting something through the heap.
        {
            let mut heap = pool.heap.lock();
            heap.insert_tuple(&Tuple::new(b"first".to_vec())).unwrap();
        }

        let frame = pool.pin_page(0).unwrap();

        // Write a marker into the frame directly.
        pool.write_frame(frame, |page| {
            page.as_bytes_mut()[PAGE_SIZE - 1] = 0xDE;
        });
        pool.unpin_page(frame, true); // mark dirty

        // Flush and verify the byte was persisted.
        pool.flush_frame(frame).unwrap();

        let mut heap = pool.heap.lock();
        let page = heap.read_page(0).unwrap();
        assert_eq!(page.as_bytes()[PAGE_SIZE - 1], 0xDE);
    }

    #[test]
    fn test_buffer_pool_eviction() {
        // Pool with only 2 frames.
        let dir = tempdir().unwrap();
        let path = dir.path().join("evict.db");
        let heap = Arc::new(Mutex::new(HeapFile::open(&path).unwrap()));

        // Pre-allocate 4 pages.
        {
            let mut h = heap.lock();
            for _ in 0..4 {
                h.allocate_page().unwrap();
            }
        }

        let pool = BufferPool::new(Arc::clone(&heap), 2);

        // Pin pages 0 and 1 — fills both frames.
        let f0 = pool.pin_page(0).unwrap();
        let f1 = pool.pin_page(1).unwrap();
        pool.unpin_page(f0, false);
        pool.unpin_page(f1, false);

        // Pin page 2 — must evict one of the above.
        let f2 = pool.pin_page(2).unwrap();
        pool.unpin_page(f2, false);

        // Pin page 3 — must evict another.
        let f3 = pool.pin_page(3).unwrap();
        pool.unpin_page(f3, false);

        // We just need this to complete without error.
        pool.flush_all_dirty().unwrap();
    }
}
