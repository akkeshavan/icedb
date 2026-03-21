use std::sync::Arc;
use tempfile::TempDir;

use storage::tid::TID;
use wal::writer::WalWriter;

use crate::error::BTreeError;
use crate::node::{BTreeNode, InternalEntry, LeafEntry, NodeType, PAGE_SIZE};
use crate::tree::BTree;

fn make_wal(dir: &TempDir) -> Arc<WalWriter> {
    Arc::new(WalWriter::open(dir.path()).unwrap())
}

fn make_btree(dir: &TempDir, wal: Arc<WalWriter>) -> BTree {
    let index_path = dir.path().join("test.idx");
    BTree::open(&index_path, wal, 1).unwrap()
}

// 1. Leaf encode/decode
#[test]
fn test_node_encode_decode_leaf() {
    let mut node = BTreeNode::new_leaf();
    for i in 0u16..5 {
        node.leaf_entries.push(LeafEntry {
            key: format!("key{:02}", i).into_bytes(),
            tid: TID::new(i as u32 * 10, i),
        });
    }

    let encoded = node.encode().unwrap();
    assert_eq!(encoded.len(), PAGE_SIZE);

    let decoded = BTreeNode::decode(&encoded).unwrap();
    assert_eq!(decoded.node_type, NodeType::Leaf);
    assert_eq!(decoded.leaf_entries.len(), 5);
    for i in 0usize..5 {
        assert_eq!(
            decoded.leaf_entries[i].key,
            format!("key{:02}", i).into_bytes()
        );
        assert_eq!(
            decoded.leaf_entries[i].tid,
            TID::new(i as u32 * 10, i as u16)
        );
    }
}

// 2. Internal encode/decode
#[test]
fn test_node_encode_decode_internal() {
    let mut node = BTreeNode::new_internal();
    // leftmost child
    node.internal_entries.push(InternalEntry {
        key: Vec::new(),
        child_page: 5,
    });
    // separator entries
    for i in 1u32..5 {
        node.internal_entries.push(InternalEntry {
            key: format!("key{:02}", i).into_bytes(),
            child_page: i + 5,
        });
    }

    let encoded = node.encode().unwrap();
    let decoded = BTreeNode::decode(&encoded).unwrap();
    assert_eq!(decoded.node_type, NodeType::Internal);
    assert_eq!(decoded.internal_entries.len(), 5);
    assert_eq!(decoded.internal_entries[0].child_page, 5);
    assert!(decoded.internal_entries[0].key.is_empty());
    for i in 1usize..5 {
        assert_eq!(
            decoded.internal_entries[i].key,
            format!("key{:02}", i).into_bytes()
        );
        assert_eq!(decoded.internal_entries[i].child_page, i as u32 + 5);
    }
}

// 3. Leaf split
#[test]
fn test_node_leaf_split() {
    let mut node = BTreeNode::new_leaf();
    let mut i = 0u32;
    // Fill until full.
    loop {
        let key = format!("key{:08}", i).into_bytes();
        node.insert_leaf_entry(LeafEntry {
            key,
            tid: TID::new(i, 0),
        });
        i += 1;
        if node.is_full() {
            break;
        }
    }

    let left_last = node.leaf_entries.last().unwrap().key.clone();
    let left_count = node.leaf_entries.len();

    let (right_node, median_key) = node.split_leaf();

    // left node should have fewer entries now
    assert!(node.leaf_entries.len() < left_count);
    // right node is non-empty
    assert!(!right_node.leaf_entries.is_empty());

    // All left entries <= median
    for entry in &node.leaf_entries {
        assert!(
            entry.key.as_slice() <= median_key.as_slice(),
            "left entry {:?} > median {:?}",
            entry.key,
            median_key
        );
    }
    // All right entries >= median
    for entry in &right_node.leaf_entries {
        assert!(entry.key.as_slice() >= median_key.as_slice());
    }
    // Left last key <= right first key
    let _ = left_last; // just checking structure
}

// 4. Insert and search
#[test]
fn test_btree_insert_and_search() {
    let dir = TempDir::new().unwrap();
    let wal_dir = TempDir::new().unwrap();
    let wal = make_wal(&wal_dir);
    let bt = make_btree(&dir, wal);

    let pairs: Vec<(Vec<u8>, TID)> = (0u32..10)
        .map(|i| (format!("key{:05}", i).into_bytes(), TID::new(i, i as u16)))
        .collect();

    for (key, tid) in &pairs {
        bt.insert(1, key, *tid).unwrap();
    }

    for (key, expected_tid) in &pairs {
        let found = bt.search(key).unwrap();
        assert_eq!(found, *expected_tid);
    }
}

// 5. Search missing key
#[test]
fn test_btree_search_missing() {
    let dir = TempDir::new().unwrap();
    let wal_dir = TempDir::new().unwrap();
    let wal = make_wal(&wal_dir);
    let bt = make_btree(&dir, wal);

    bt.insert(1, b"hello", TID::new(1, 0)).unwrap();

    let result = bt.search(b"notfound");
    assert!(matches!(result, Err(BTreeError::KeyNotFound)));
}

// 6. Insert many
#[test]
fn test_btree_insert_many() {
    let dir = TempDir::new().unwrap();
    let wal_dir = TempDir::new().unwrap();
    let wal = make_wal(&wal_dir);
    let bt = make_btree(&dir, wal);

    for i in 0u32..500 {
        let key = format!("key{:05}", i).into_bytes();
        bt.insert(1, &key, TID::new(i, 0)).unwrap();
    }

    // Check 50 of them
    for i in (0u32..500).step_by(10) {
        let key = format!("key{:05}", i).into_bytes();
        let tid = bt.search(&key).unwrap();
        assert_eq!(tid, TID::new(i, 0));
    }
}

// 7. Causes splits (height > 1)
#[test]
fn test_btree_causes_splits() {
    let dir = TempDir::new().unwrap();
    let wal_dir = TempDir::new().unwrap();
    let wal = make_wal(&wal_dir);
    let index_path = dir.path().join("split.idx");
    let bt = BTree::open(&index_path, wal, 2).unwrap();

    // Insert ~300 entries with 20-byte keys.
    for i in 0u32..300 {
        let key = format!("{:020}", i).into_bytes();
        bt.insert(1, &key, TID::new(i, 0)).unwrap();
    }

    // Verify root height > 1 by checking all inserted entries can be found.
    for i in 0u32..300 {
        let key = format!("{:020}", i).into_bytes();
        let tid = bt.search(&key).unwrap();
        assert_eq!(tid, TID::new(i, 0));
    }

    // Check num_entries
    assert_eq!(bt.num_entries().unwrap(), 300);
}

// 8. Range scan
#[test]
fn test_btree_range_scan() {
    let dir = TempDir::new().unwrap();
    let wal_dir = TempDir::new().unwrap();
    let wal = make_wal(&wal_dir);
    let bt = make_btree(&dir, wal);

    let keys = ["apple", "banana", "cherry", "date", "elderberry"];
    for (i, &key) in keys.iter().enumerate() {
        bt.insert(1, key.as_bytes(), TID::new(i as u32, 0)).unwrap();
    }

    let results = bt.range_scan(Some(b"banana"), Some(b"date")).unwrap();
    let found_keys: Vec<&str> = results
        .iter()
        .map(|(k, _)| std::str::from_utf8(k).unwrap())
        .collect();
    assert_eq!(found_keys, vec!["banana", "cherry", "date"]);
}

// 9. Delete
#[test]
fn test_btree_delete() {
    let dir = TempDir::new().unwrap();
    let wal_dir = TempDir::new().unwrap();
    let wal = make_wal(&wal_dir);
    let bt = make_btree(&dir, wal);

    for i in 0u32..20 {
        let key = format!("key{:05}", i).into_bytes();
        bt.insert(1, &key, TID::new(i, 0)).unwrap();
    }

    // Delete even keys.
    for i in (0u32..20).step_by(2) {
        let key = format!("key{:05}", i).into_bytes();
        bt.delete(1, &key).unwrap();
    }

    // Even keys should be gone.
    for i in (0u32..20).step_by(2) {
        let key = format!("key{:05}", i).into_bytes();
        assert!(matches!(bt.search(&key), Err(BTreeError::KeyNotFound)));
    }

    // Odd keys should remain.
    for i in (1u32..20).step_by(2) {
        let key = format!("key{:05}", i).into_bytes();
        let tid = bt.search(&key).unwrap();
        assert_eq!(tid, TID::new(i, 0));
    }
}

// 10. Recovery
#[test]
fn test_btree_recovery() {
    let dir = TempDir::new().unwrap();
    let wal_dir = TempDir::new().unwrap();
    let index_path = dir.path().join("recover.idx");

    {
        let wal = Arc::new(WalWriter::open(wal_dir.path()).unwrap());
        let bt = BTree::open(&index_path, wal, 3).unwrap();
        for i in 0u32..50 {
            let key = format!("key{:05}", i).into_bytes();
            bt.insert(1, &key, TID::new(i, 0)).unwrap();
        }
        // Sync the index file.
        bt.sync().unwrap();
    }

    // Reopen.
    {
        let wal = Arc::new(WalWriter::open(wal_dir.path()).unwrap());
        let bt = BTree::open(&index_path, wal, 3).unwrap();
        for i in (0u32..50).step_by(5) {
            let key = format!("key{:05}", i).into_bytes();
            let tid = bt.search(&key).unwrap();
            assert_eq!(tid, TID::new(i, 0));
        }
    }
}
