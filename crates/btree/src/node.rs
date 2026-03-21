use crate::error::BTreeError;
use storage::tid::TID;

pub const PAGE_SIZE: usize = 8192;
pub const NODE_HEADER_SIZE: usize = 11; // type(1) + num_entries(2) + right_sib(4) + left_sib(4)

/// Threshold below which we consider a page "full" (conservative).
const FULL_THRESHOLD: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Internal = 0,
    Leaf = 1,
}

#[derive(Debug, Clone)]
pub struct InternalEntry {
    pub key: Vec<u8>,    // separator key (empty for leftmost child)
    pub child_page: u32, // page number of child
}

#[derive(Debug, Clone)]
pub struct LeafEntry {
    pub key: Vec<u8>,
    pub tid: TID,
}

#[derive(Debug, Clone)]
pub struct BTreeNode {
    pub node_type: NodeType,
    pub right_sibling: u32, // 0 = none
    pub left_sibling: u32,  // 0 = none
    pub internal_entries: Vec<InternalEntry>,
    pub leaf_entries: Vec<LeafEntry>,
}

impl BTreeNode {
    pub fn new_leaf() -> Self {
        BTreeNode {
            node_type: NodeType::Leaf,
            right_sibling: 0,
            left_sibling: 0,
            internal_entries: Vec::new(),
            leaf_entries: Vec::new(),
        }
    }

    pub fn new_internal() -> Self {
        BTreeNode {
            node_type: NodeType::Internal,
            right_sibling: 0,
            left_sibling: 0,
            internal_entries: Vec::new(),
            leaf_entries: Vec::new(),
        }
    }

    /// Size of a single entry: [key_len: u16][key: ...][child_page: u32] for internal,
    /// [key_len: u16][key: ...][tid_page: u32][tid_slot: u16] for leaf.
    pub fn entry_size(key_len: usize, is_leaf: bool) -> usize {
        if is_leaf {
            2 + key_len + 4 + 2 // key_len(2) + key + tid_page(4) + tid_slot(2)
        } else {
            2 + key_len + 4 // key_len(2) + key + child_page(4)
        }
    }

    /// Compute the total encoded size of this node.
    pub fn encoded_size(&self) -> usize {
        let mut size = NODE_HEADER_SIZE;
        match self.node_type {
            NodeType::Leaf => {
                for entry in &self.leaf_entries {
                    size += Self::entry_size(entry.key.len(), true);
                }
            }
            NodeType::Internal => {
                for entry in &self.internal_entries {
                    size += Self::entry_size(entry.key.len(), false);
                }
            }
        }
        size
    }

    /// Returns true if adding one more entry would likely exceed PAGE_SIZE.
    pub fn is_full(&self) -> bool {
        PAGE_SIZE - self.encoded_size() < FULL_THRESHOLD
    }

    /// Encode this node into a PAGE_SIZE buffer.
    pub fn encode(&self) -> Result<Vec<u8>, BTreeError> {
        let encoded_size = self.encoded_size();
        if encoded_size > PAGE_SIZE {
            return Err(BTreeError::InvalidPage(0));
        }

        let mut buf = vec![0u8; PAGE_SIZE];
        let mut pos = 0;

        // node_type: u8
        buf[pos] = self.node_type as u8;
        pos += 1;

        // num_entries: u16
        let num_entries = match self.node_type {
            NodeType::Leaf => self.leaf_entries.len() as u16,
            NodeType::Internal => self.internal_entries.len() as u16,
        };
        buf[pos..pos + 2].copy_from_slice(&num_entries.to_le_bytes());
        pos += 2;

        // right_sibling: u32
        buf[pos..pos + 4].copy_from_slice(&self.right_sibling.to_le_bytes());
        pos += 4;

        // left_sibling: u32
        buf[pos..pos + 4].copy_from_slice(&self.left_sibling.to_le_bytes());
        pos += 4;

        // entries
        match self.node_type {
            NodeType::Leaf => {
                for entry in &self.leaf_entries {
                    let key_len = entry.key.len() as u16;
                    buf[pos..pos + 2].copy_from_slice(&key_len.to_le_bytes());
                    pos += 2;
                    buf[pos..pos + entry.key.len()].copy_from_slice(&entry.key);
                    pos += entry.key.len();
                    buf[pos..pos + 4].copy_from_slice(&entry.tid.page_no.to_le_bytes());
                    pos += 4;
                    buf[pos..pos + 2].copy_from_slice(&entry.tid.slot.to_le_bytes());
                    pos += 2;
                }
            }
            NodeType::Internal => {
                for entry in &self.internal_entries {
                    let key_len = entry.key.len() as u16;
                    buf[pos..pos + 2].copy_from_slice(&key_len.to_le_bytes());
                    pos += 2;
                    buf[pos..pos + entry.key.len()].copy_from_slice(&entry.key);
                    pos += entry.key.len();
                    buf[pos..pos + 4].copy_from_slice(&entry.child_page.to_le_bytes());
                    pos += 4;
                }
            }
        }

        Ok(buf)
    }

    /// Decode a node from a PAGE_SIZE buffer.
    pub fn decode(bytes: &[u8]) -> Result<BTreeNode, BTreeError> {
        if bytes.len() < NODE_HEADER_SIZE {
            return Err(BTreeError::InvalidPage(0));
        }

        let mut pos = 0;

        let node_type_byte = bytes[pos];
        pos += 1;
        let node_type = match node_type_byte {
            0 => NodeType::Internal,
            1 => NodeType::Leaf,
            _ => return Err(BTreeError::InvalidPage(0)),
        };

        let num_entries = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;

        let right_sibling = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
        pos += 4;

        let left_sibling = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
        pos += 4;

        let mut node = BTreeNode {
            node_type,
            right_sibling,
            left_sibling,
            internal_entries: Vec::new(),
            leaf_entries: Vec::new(),
        };

        match node_type {
            NodeType::Leaf => {
                for _ in 0..num_entries {
                    if pos + 2 > bytes.len() {
                        return Err(BTreeError::InvalidPage(0));
                    }
                    let key_len =
                        u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap()) as usize;
                    pos += 2;
                    if pos + key_len + 6 > bytes.len() {
                        return Err(BTreeError::InvalidPage(0));
                    }
                    let key = bytes[pos..pos + key_len].to_vec();
                    pos += key_len;
                    let tid_page = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    let tid_slot = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
                    pos += 2;
                    node.leaf_entries.push(LeafEntry {
                        key,
                        tid: TID::new(tid_page, tid_slot),
                    });
                }
            }
            NodeType::Internal => {
                for _ in 0..num_entries {
                    if pos + 2 > bytes.len() {
                        return Err(BTreeError::InvalidPage(0));
                    }
                    let key_len =
                        u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap()) as usize;
                    pos += 2;
                    if pos + key_len + 4 > bytes.len() {
                        return Err(BTreeError::InvalidPage(0));
                    }
                    let key = bytes[pos..pos + key_len].to_vec();
                    pos += key_len;
                    let child_page = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    node.internal_entries
                        .push(InternalEntry { key, child_page });
                }
            }
        }

        Ok(node)
    }

    /// Binary search for a key in a leaf node; returns Some(index) if found.
    pub fn find_key(&self, key: &[u8]) -> Option<usize> {
        let entries = &self.leaf_entries;
        let mut lo = 0usize;
        let mut hi = entries.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match entries[mid].key.as_slice().cmp(key) {
                std::cmp::Ordering::Equal => return Some(mid),
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
            }
        }
        None
    }

    /// Insert a leaf entry, maintaining sorted order.
    pub fn insert_leaf_entry(&mut self, entry: LeafEntry) {
        let pos = self
            .leaf_entries
            .partition_point(|e| e.key.as_slice() < entry.key.as_slice());
        self.leaf_entries.insert(pos, entry);
    }

    /// Insert an internal entry, maintaining sorted order by key.
    /// For internal nodes, the leftmost child has key == [] and must stay first.
    pub fn insert_internal_entry(&mut self, entry: InternalEntry) {
        // Leftmost entry (empty key) must stay at index 0.
        // All other entries are sorted by key.
        let pos = if entry.key.is_empty() {
            0
        } else {
            // Find insertion point among non-empty keys (skip index 0 if it's the leftmost)
            let start = if self
                .internal_entries
                .first()
                .is_some_and(|e| e.key.is_empty())
            {
                1
            } else {
                0
            };
            let offset = self.internal_entries[start..]
                .partition_point(|e| e.key.as_slice() < entry.key.as_slice());
            start + offset
        };
        self.internal_entries.insert(pos, entry);
    }

    /// Split a leaf node: self retains left half, returns (right_node, median_key).
    /// The median_key is the first key of the right node (used to promote to parent).
    pub fn split_leaf(&mut self) -> (BTreeNode, Vec<u8>) {
        let mid = self.leaf_entries.len() / 2;
        let right_entries = self.leaf_entries.split_off(mid);
        let median_key = right_entries[0].key.clone();

        let right_node = BTreeNode {
            node_type: NodeType::Leaf,
            right_sibling: self.right_sibling,
            left_sibling: 0, // will be set by caller
            internal_entries: Vec::new(),
            leaf_entries: right_entries,
        };

        (right_node, median_key)
    }

    /// Split an internal node: self retains left half, returns (right_node, promoted_key, right_child_of_promoted).
    /// The promoted key moves up to the parent. The right child of the promoted key goes
    /// into right_node as its leftmost child.
    pub fn split_internal(&mut self) -> (BTreeNode, Vec<u8>, u32) {
        let n = self.internal_entries.len();
        let mid = n / 2;

        // The entry at mid is promoted; entries after mid go to the right node.
        let right_entries = self.internal_entries.split_off(mid);
        // right_entries[0] is the promoted entry.
        let promoted = right_entries.into_iter().collect::<Vec<_>>();
        let promoted_entry = promoted[0].clone();
        let remaining_right: Vec<InternalEntry> = promoted.into_iter().skip(1).collect();

        let promoted_key = promoted_entry.key.clone();
        let right_first_child = promoted_entry.child_page;

        // The right node's leftmost child is right_first_child (stored with empty key).
        let mut right_internal = vec![InternalEntry {
            key: Vec::new(),
            child_page: right_first_child,
        }];
        right_internal.extend(remaining_right);

        let right_node = BTreeNode {
            node_type: NodeType::Internal,
            right_sibling: 0,
            left_sibling: 0,
            internal_entries: right_internal,
            leaf_entries: Vec::new(),
        };

        (right_node, promoted_key, right_first_child)
    }
}
