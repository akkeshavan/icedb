use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use storage::tid::TID;
use wal::record::WalRecordType;
use wal::writer::WalWriter;

use crate::error::BTreeError;
use crate::file::BTreeFile;
use crate::meta::{BTreeMeta, META_PAGE_NO};
use crate::node::{BTreeNode, InternalEntry, LeafEntry, NodeType, PAGE_SIZE};

pub struct BTree {
    file: Arc<RwLock<BTreeFile>>,
    wal: Arc<WalWriter>,
    index_id: u32,
}

impl BTree {
    /// Opens or creates a B+ tree at the given path.
    pub fn open(path: &Path, wal: Arc<WalWriter>, index_id: u32) -> Result<BTree, BTreeError> {
        let mut btf = BTreeFile::open(path)?;

        // If file is empty, initialize.
        if btf.num_pages()? == 0 {
            // Write meta page (page 0).
            let meta = BTreeMeta {
                root_page: 1,
                height: 1,
                num_entries: 0,
                next_page: 2,
            };
            let meta_bytes = meta.encode();
            let mut meta_page = [0u8; PAGE_SIZE];
            meta_page[..32].copy_from_slice(&meta_bytes);
            btf.write_page(META_PAGE_NO, &meta_page)?;

            // Write empty leaf node as root at page 1.
            let root = BTreeNode::new_leaf();
            let root_bytes = root.encode()?;
            let mut root_page = [0u8; PAGE_SIZE];
            root_page.copy_from_slice(&root_bytes);
            btf.write_page(1, &root_page)?;
        }

        Ok(BTree {
            file: Arc::new(RwLock::new(btf)),
            wal,
            index_id,
        })
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn wal_page_no(&self, page_no: u32) -> u32 {
        self.index_id * 100_000 + page_no
    }

    fn read_meta(file: &BTreeFile) -> Result<BTreeMeta, BTreeError> {
        let page = file.read_page(META_PAGE_NO)?;
        BTreeMeta::decode(&page)
    }

    fn write_meta(file: &mut BTreeFile, meta: &BTreeMeta) -> Result<(), BTreeError> {
        let mut page = [0u8; PAGE_SIZE];
        let encoded = meta.encode();
        page[..32].copy_from_slice(&encoded);
        file.write_page(META_PAGE_NO, &page)?;
        Ok(())
    }

    fn read_node(file: &BTreeFile, page_no: u32) -> Result<BTreeNode, BTreeError> {
        let page = file.read_page(page_no)?;
        BTreeNode::decode(&page)
    }

    fn write_node(
        file: &mut BTreeFile,
        page_no: u32,
        node: &BTreeNode,
        wal: &WalWriter,
        xid: u32,
        wal_page_no: u32,
    ) -> Result<(), BTreeError> {
        let encoded = node.encode()?;
        let mut page = [0u8; PAGE_SIZE];
        page.copy_from_slice(&encoded);
        file.write_page(page_no, &page)?;
        // WAL log: PageImage record
        wal.append(xid, WalRecordType::PageImage, wal_page_no, page.to_vec())?;
        Ok(())
    }

    /// Find the child page to follow in an internal node for the given key.
    ///
    /// Uses binary search (O(log n)) instead of a linear scan for internal
    /// nodes with many separator keys.
    fn find_child(node: &BTreeNode, key: &[u8]) -> u32 {
        // entries[0] is the leftmost child (separator key == [] i.e. -∞).
        // entries[i] (i > 0) have separator keys; we descend into entries[i]
        // when key >= entries[i].key.
        let entries = &node.internal_entries;
        if entries.is_empty() {
            panic!("internal node has no children");
        }
        // partition_point returns the first index where the predicate is false.
        // We want the last entry whose key <= search_key.
        let pos = entries.partition_point(|e| e.key.as_slice() <= key);
        // pos == 0 is impossible because entries[0].key == [] <= any key;
        // but guard with saturating_sub to be safe.
        entries[pos.saturating_sub(1).min(entries.len() - 1)].child_page
    }

    /// Traverse from root to leaf, returning the stack of page numbers (root first, leaf last).
    fn find_leaf_path(
        file: &BTreeFile,
        meta: &BTreeMeta,
        key: &[u8],
    ) -> Result<Vec<u32>, BTreeError> {
        let mut path = Vec::new();
        let mut current = meta.root_page;
        path.push(current);

        loop {
            let node = Self::read_node(file, current)?;
            match node.node_type {
                NodeType::Leaf => break,
                NodeType::Internal => {
                    current = Self::find_child(&node, key);
                    path.push(current);
                }
            }
        }
        Ok(path)
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Insert a key → TID mapping.
    pub fn insert(&self, xid: u32, key: &[u8], tid: TID) -> Result<(), BTreeError> {
        let mut file = self.file.write();
        let mut meta = Self::read_meta(&file)?;

        // Find path to leaf.
        let path = Self::find_leaf_path(&file, &meta, key)?;
        let leaf_page_no = *path.last().unwrap();

        let mut leaf = Self::read_node(&file,leaf_page_no)?;

        // Check for duplicate.
        if leaf.find_key(key).is_some() {
            return Err(BTreeError::DuplicateKey);
        }

        leaf.insert_leaf_entry(LeafEntry {
            key: key.to_vec(),
            tid,
        });
        meta.num_entries += 1;

        if !leaf.is_full() {
            // Simple case: just write the leaf back.
            let wal_pno = self.wal_page_no(leaf_page_no);
            Self::write_node(&mut file, leaf_page_no, &leaf, &self.wal, xid, wal_pno)?;
            Self::write_meta(&mut file, &meta)?;
            return Ok(());
        }

        // Leaf is full — need to split.
        // We'll propagate splits up the path.
        // `to_insert` holds the promoted key and right child page to insert into parent.
        let (mut right_leaf, median_key) = leaf.split_leaf();

        // Allocate a page for the right leaf.
        let right_page_no = meta.next_page;
        meta.next_page += 1;
        // Make sure the page exists (allocate_page extends the file).
        // We need to write to page right_page_no.
        // Ensure file is large enough.
        while file.num_pages()? <= right_page_no {
            file.allocate_page()?;
        }

        // Update sibling pointers.
        right_leaf.left_sibling = leaf_page_no;
        right_leaf.right_sibling = leaf.right_sibling;
        leaf.right_sibling = right_page_no;

        // If original leaf had a right sibling, update its left_sibling.
        if right_leaf.right_sibling != 0 {
            let mut old_right = Self::read_node(&file,right_leaf.right_sibling)?;
            old_right.left_sibling = right_page_no;
            let wal_pno = self.wal_page_no(right_leaf.right_sibling);
            Self::write_node(
                &mut file,
                right_leaf.right_sibling,
                &old_right,
                &self.wal,
                xid,
                wal_pno,
            )?;
        }

        let wal_pno = self.wal_page_no(leaf_page_no);
        Self::write_node(&mut file, leaf_page_no, &leaf, &self.wal, xid, wal_pno)?;
        let wal_pno = self.wal_page_no(right_page_no);
        Self::write_node(
            &mut file,
            right_page_no,
            &right_leaf,
            &self.wal,
            xid,
            wal_pno,
        )?;

        // Now propagate the promoted key up through internal nodes.
        // path[..len-1] are internal nodes (from root downward).
        let parent_path = &path[..path.len() - 1];
        let mut promote_key = median_key;
        let mut promote_right_child = right_page_no;

        let mut split_occurred = true;
        let mut parent_idx = parent_path.len();

        while split_occurred && parent_idx > 0 {
            parent_idx -= 1;
            let parent_page_no = parent_path[parent_idx];
            let mut parent = Self::read_node(&file,parent_page_no)?;

            // Insert the promoted key into the parent.
            parent.insert_internal_entry(InternalEntry {
                key: promote_key.clone(),
                child_page: promote_right_child,
            });

            if !parent.is_full() {
                let wal_pno = self.wal_page_no(parent_page_no);
                Self::write_node(&mut file, parent_page_no, &parent, &self.wal, xid, wal_pno)?;
                split_occurred = false;
            } else {
                // Split the internal node.
                let (right_internal, new_promote_key, _right_first_child) = parent.split_internal();

                let right_internal_page_no = meta.next_page;
                meta.next_page += 1;
                while file.num_pages()? <= right_internal_page_no {
                    file.allocate_page()?;
                }

                let wal_pno = self.wal_page_no(parent_page_no);
                Self::write_node(&mut file, parent_page_no, &parent, &self.wal, xid, wal_pno)?;
                let wal_pno = self.wal_page_no(right_internal_page_no);
                Self::write_node(
                    &mut file,
                    right_internal_page_no,
                    &right_internal,
                    &self.wal,
                    xid,
                    wal_pno,
                )?;

                promote_key = new_promote_key;
                promote_right_child = right_internal_page_no;
                // split_occurred remains true, continue loop
            }
        }

        // If we've exhausted the path and still have a split to propagate, we need a new root.
        if split_occurred {
            // Create a new root internal node.
            let new_root_page_no = meta.next_page;
            meta.next_page += 1;
            while file.num_pages()? <= new_root_page_no {
                file.allocate_page()?;
            }

            let mut new_root = BTreeNode::new_internal();
            // Leftmost child = old root (path[0])
            new_root.internal_entries.push(InternalEntry {
                key: Vec::new(),
                child_page: meta.root_page,
            });
            // Right child
            new_root.internal_entries.push(InternalEntry {
                key: promote_key,
                child_page: promote_right_child,
            });

            let wal_pno = self.wal_page_no(new_root_page_no);
            Self::write_node(
                &mut file,
                new_root_page_no,
                &new_root,
                &self.wal,
                xid,
                wal_pno,
            )?;

            meta.root_page = new_root_page_no;
            meta.height += 1;
        }

        Self::write_meta(&mut file, &meta)?;
        Ok(())
    }

    /// Find the TID for an exact key match.
    ///
    /// Uses a shared read lock — multiple concurrent searches can proceed in
    /// parallel without blocking each other or blocking concurrent writes.
    pub fn search(&self, key: &[u8]) -> Result<TID, BTreeError> {
        let file = self.file.read();

        let meta = Self::read_meta(&file)?;
        let mut current = meta.root_page;

        loop {
            let node = Self::read_node(&file, current)?;
            match node.node_type {
                NodeType::Internal => {
                    current = Self::find_child(&node, key);
                }
                NodeType::Leaf => {
                    if let Some(idx) = node.find_key(key) {
                        return Ok(node.leaf_entries[idx].tid);
                    } else {
                        return Err(BTreeError::KeyNotFound);
                    }
                }
            }
        }
    }

    /// Range scan: returns all (key, TID) pairs where start_key <= key <= end_key.
    ///
    /// Uses a shared read lock — multiple concurrent range scans can proceed in
    /// parallel without blocking each other or blocking concurrent writes.
    pub fn range_scan(
        &self,
        start_key: Option<&[u8]>,
        end_key: Option<&[u8]>,
    ) -> Result<Vec<(Vec<u8>, TID)>, BTreeError> {
        let file = self.file.read();
        let meta = Self::read_meta(&file)?;

        // Find the leftmost leaf.
        let first_leaf_page = if let Some(sk) = start_key {
            // Traverse to the leaf that would contain start_key.
            let mut current = meta.root_page;
            loop {
                let node = Self::read_node(&file, current)?;
                match node.node_type {
                    NodeType::Internal => {
                        current = Self::find_child(&node, sk);
                    }
                    NodeType::Leaf => break current,
                }
            }
        } else {
            // Find the leftmost leaf by always going left.
            let mut current = meta.root_page;
            loop {
                let node = Self::read_node(&file, current)?;
                match node.node_type {
                    NodeType::Internal => {
                        if let Some(first) = node.internal_entries.first() {
                            current = first.child_page;
                        } else {
                            break current;
                        }
                    }
                    NodeType::Leaf => break current,
                }
            }
        };

        let mut results = Vec::new();
        let mut current_page = first_leaf_page;

        loop {
            if current_page == 0 {
                break;
            }
            let node = Self::read_node(&file, current_page)?;

            let mut done = false;
            for entry in &node.leaf_entries {
                // Check start_key lower bound.
                if let Some(sk) = start_key {
                    if entry.key.as_slice() < sk {
                        continue;
                    }
                }
                // Check end_key upper bound.
                if let Some(ek) = end_key {
                    if entry.key.as_slice() > ek {
                        done = true;
                        break;
                    }
                }
                results.push((entry.key.clone(), entry.tid));
            }

            if done {
                break;
            }

            current_page = node.right_sibling;
            if current_page == 0 {
                break;
            }
        }

        Ok(results)
    }

    /// Delete the entry for the given key.
    pub fn delete(&self, xid: u32, key: &[u8]) -> Result<(), BTreeError> {
        let mut file = self.file.write();
        let mut meta = Self::read_meta(&file)?;

        // Find the leaf containing the key.
        let path = Self::find_leaf_path(&file, &meta, key)?;
        let leaf_page_no = *path.last().unwrap();

        let mut leaf = Self::read_node(&file,leaf_page_no)?;

        if let Some(idx) = leaf.find_key(key) {
            leaf.leaf_entries.remove(idx);
            meta.num_entries = meta.num_entries.saturating_sub(1);
            let wal_pno = self.wal_page_no(leaf_page_no);
            Self::write_node(&mut file, leaf_page_no, &leaf, &self.wal, xid, wal_pno)?;
            Self::write_meta(&mut file, &meta)?;
            Ok(())
        } else {
            Err(BTreeError::KeyNotFound)
        }
    }

    /// Returns the number of entries in the tree.
    pub fn num_entries(&self) -> Result<u64, BTreeError> {
        let file = self.file.read();
        let meta = Self::read_meta(&file)?;
        Ok(meta.num_entries)
    }

    /// Sync the index file to disk.
    pub fn sync(&self) -> Result<(), BTreeError> {
        self.file.read().sync()
    }
}
