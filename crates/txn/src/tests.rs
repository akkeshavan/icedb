use std::sync::Arc;

use storage::heap::HeapFile;
use tempfile::TempDir;
use wal::writer::WalWriter;

use crate::lock::LockManager;
use crate::manager::TransactionManager;
use crate::transaction::IsolationLevel;
fn setup(tmp: &TempDir) -> (TransactionManager, HeapFile) {
    let wal_dir = tmp.path().join("wal");
    let heap_path = tmp.path().join("heap.db");
    let wal = Arc::new(WalWriter::open(&wal_dir).unwrap());
    let tm = TransactionManager::new(wal);
    let heap = HeapFile::open(&heap_path).unwrap();
    (tm, heap)
}

// ── 1. begin/commit ───────────────────────────────────────────────────────────

#[test]
fn test_begin_commit() {
    let tmp = TempDir::new().unwrap();
    let (tm, _heap) = setup(&tmp);

    let xid = tm.begin(IsolationLevel::ReadCommitted);
    assert!(tm.is_active(xid));
    assert!(!tm.is_committed(xid));

    tm.commit(xid).unwrap();
    assert!(!tm.is_active(xid));
    assert!(tm.is_committed(xid));
}

// ── 2. begin/abort ────────────────────────────────────────────────────────────

#[test]
fn test_begin_abort() {
    let tmp = TempDir::new().unwrap();
    let (tm, _heap) = setup(&tmp);

    let xid = tm.begin(IsolationLevel::ReadCommitted);
    assert!(tm.is_active(xid));

    tm.abort(xid).unwrap();
    assert!(!tm.is_active(xid));
    assert!(tm.is_aborted(xid));
}

// ── 3. snapshot visibility ────────────────────────────────────────────────────

#[test]
fn test_snapshot_visibility() {
    let tmp = TempDir::new().unwrap();
    let (tm, _heap) = setup(&tmp);

    let t1 = tm.begin(IsolationLevel::ReadCommitted);
    let t2 = tm.begin(IsolationLevel::ReadCommitted);

    // Commit T1 before taking a snapshot for T2
    tm.commit(t1).unwrap();

    // T2 is ReadCommitted — fresh snapshot sees T1 as committed
    let snap = tm.current_snapshot(t2);
    // T1 committed so it's no longer in active; snapshot.xid_is_visible(t1) → depends
    // on whether t1 < snap.xmin.  Since both were started before commit, t1 may be < xmin now.
    // The key invariant: t1 is committed so it should be visible.
    // committed set is not part of snapshot; visibility uses the snapshot + committed set.
    // For ReadCommitted: all XIDs < xmax that are not in_progress are treated as committed.
    // T1 was removed from active on commit, so it's NOT in snap.in_progress.
    // t1 < snap.xmax (snapshot.xmax = next_xid at snapshot time, which is > t1 and t2).
    assert!(
        !snap.in_progress.contains(&t1),
        "T1 should be committed and not in-progress"
    );
    assert!(snap.in_progress.contains(&t2), "T2 should be in-progress");
}

// ── 4. insert and scan ────────────────────────────────────────────────────────

#[test]
fn test_insert_and_scan() {
    let tmp = TempDir::new().unwrap();
    let (tm, mut heap) = setup(&tmp);

    let t1 = tm.begin(IsolationLevel::ReadCommitted);
    for i in 0u32..5 {
        let data = format!("row-{i}").into_bytes();
        tm.insert_tuple(t1, &mut heap, &data).unwrap();
    }
    tm.commit(t1).unwrap();

    let t2 = tm.begin(IsolationLevel::ReadCommitted);
    let visible = tm.scan_visible_tuples(t2, &mut heap).unwrap();
    assert_eq!(
        visible.len(),
        5,
        "T2 should see all 5 tuples committed by T1"
    );
    tm.commit(t2).unwrap();
}

// ── 5. dirty read prevention ──────────────────────────────────────────────────

#[test]
fn test_dirty_read_prevention() {
    let tmp = TempDir::new().unwrap();
    let (tm, mut heap) = setup(&tmp);

    // T1 inserts but does NOT commit
    let t1 = tm.begin(IsolationLevel::ReadCommitted);
    tm.insert_tuple(t1, &mut heap, b"dirty row").unwrap();
    // T1 still active (no commit)

    // T2 scans — should NOT see T1's uncommitted tuple
    let t2 = tm.begin(IsolationLevel::ReadCommitted);
    let visible = tm.scan_visible_tuples(t2, &mut heap).unwrap();
    assert_eq!(visible.len(), 0, "T2 must not see T1's uncommitted tuple");

    tm.abort(t1).unwrap();
    tm.commit(t2).unwrap();
}

// ── 6. delete visibility ──────────────────────────────────────────────────────

#[test]
fn test_delete_visibility() {
    let tmp = TempDir::new().unwrap();
    let (tm, mut heap) = setup(&tmp);

    // T1 inserts and commits a tuple
    let t1 = tm.begin(IsolationLevel::ReadCommitted);
    let tid = tm.insert_tuple(t1, &mut heap, b"live row").unwrap();
    tm.commit(t1).unwrap();

    // T2 deletes but does NOT commit
    let t2 = tm.begin(IsolationLevel::ReadCommitted);
    tm.delete_tuple(t2, &mut heap, tid).unwrap();
    // T2 still active

    // T3 scans — should still see the tuple (T2's delete not committed)
    let t3 = tm.begin(IsolationLevel::ReadCommitted);
    let visible = tm.scan_visible_tuples(t3, &mut heap).unwrap();
    assert_eq!(
        visible.len(),
        1,
        "T3 should still see the tuple (T2's delete is uncommitted)"
    );

    tm.abort(t2).unwrap();
    tm.commit(t3).unwrap();
}

// ── 7. repeatable read ────────────────────────────────────────────────────────

#[test]
fn test_repeatable_read() {
    let tmp = TempDir::new().unwrap();
    let (tm, mut heap) = setup(&tmp);

    // T1 starts with Repeatable Read — snapshot is frozen at BEGIN
    let t1 = tm.begin(IsolationLevel::RepeatableRead);

    // First scan: nothing yet
    let first_scan = tm.scan_visible_tuples(t1, &mut heap).unwrap();
    assert_eq!(first_scan.len(), 0);

    // T2 inserts and commits
    let t2 = tm.begin(IsolationLevel::ReadCommitted);
    tm.insert_tuple(t2, &mut heap, b"new row").unwrap();
    tm.commit(t2).unwrap();

    // T1 scans again — should NOT see T2's tuple (snapshot frozen at T1 begin)
    let second_scan = tm.scan_visible_tuples(t1, &mut heap).unwrap();
    assert_eq!(
        second_scan.len(),
        0,
        "T1 (RepeatableRead) must not see T2's tuple committed after T1 started"
    );

    tm.commit(t1).unwrap();
}

// ── 8. write-write conflict ───────────────────────────────────────────────────

#[test]
fn test_write_write_conflict() {
    let tmp = TempDir::new().unwrap();
    let (tm, mut heap) = setup(&tmp);

    // Insert a tuple to get a TID
    let t0 = tm.begin(IsolationLevel::ReadCommitted);
    let tid = tm.insert_tuple(t0, &mut heap, b"shared row").unwrap();
    tm.commit(t0).unwrap();

    let lm = LockManager::new();
    let t1 = tm.begin(IsolationLevel::ReadCommitted);
    let t2 = tm.begin(IsolationLevel::ReadCommitted);

    // T1 acquires write lock
    lm.acquire_write_lock(t1, tid).unwrap();

    // T2 tries to acquire same lock — should fail with WriteWriteConflict
    let result = lm.acquire_write_lock(t2, tid);
    assert!(
        matches!(result, Err(crate::error::TxnError::WriteWriteConflict { holder }) if holder == t1),
        "Expected WriteWriteConflict from T1, got: {:?}",
        result
    );

    tm.commit(t1).unwrap();
    tm.commit(t2).unwrap();
}

// ── 9. bank transfer atomicity ────────────────────────────────────────────────

#[test]
fn test_bank_transfer_atomicity() {
    let tmp = TempDir::new().unwrap();
    let (tm, mut heap) = setup(&tmp);

    // T0: insert two accounts (balance as 4-byte LE u32)
    let t0 = tm.begin(IsolationLevel::ReadCommitted);
    let account_a_data = 1000u32.to_le_bytes();
    let account_b_data = 1000u32.to_le_bytes();
    let tid_a = tm.insert_tuple(t0, &mut heap, &account_a_data).unwrap();
    let tid_b = tm.insert_tuple(t0, &mut heap, &account_b_data).unwrap();
    tm.commit(t0).unwrap();

    // T1: transfer 500 from A to B
    let t1 = tm.begin(IsolationLevel::ReadCommitted);

    // Debit A: delete old, insert new with 500
    tm.delete_tuple(t1, &mut heap, tid_a).unwrap();
    let new_a_data = 500u32.to_le_bytes();
    tm.insert_tuple(t1, &mut heap, &new_a_data).unwrap();

    // Credit B: delete old, insert new with 1500
    tm.delete_tuple(t1, &mut heap, tid_b).unwrap();
    let new_b_data = 1500u32.to_le_bytes();
    tm.insert_tuple(t1, &mut heap, &new_b_data).unwrap();

    tm.commit(t1).unwrap();

    // Scan: verify total is still 2000
    let t2 = tm.begin(IsolationLevel::ReadCommitted);
    let visible = tm.scan_visible_tuples(t2, &mut heap).unwrap();

    let total: u32 = visible
        .iter()
        .map(|(_, tuple)| u32::from_le_bytes(tuple.data[..4].try_into().unwrap()))
        .sum();
    assert_eq!(total, 2000, "Total balance must be preserved");

    let balances: Vec<u32> = visible
        .iter()
        .map(|(_, tuple)| u32::from_le_bytes(tuple.data[..4].try_into().unwrap()))
        .collect();
    assert!(balances.contains(&500), "Account A should have 500");
    assert!(balances.contains(&1500), "Account B should have 1500");
    assert_eq!(visible.len(), 2, "Should see exactly 2 account rows");

    tm.commit(t2).unwrap();
}

// ── 10. read committed sees latest ────────────────────────────────────────────

#[test]
fn test_read_committed_sees_latest() {
    let tmp = TempDir::new().unwrap();
    let (tm, mut heap) = setup(&tmp);

    // T2 starts before T1 commits
    let t2 = tm.begin(IsolationLevel::ReadCommitted);

    // T1 inserts and commits
    let t1 = tm.begin(IsolationLevel::ReadCommitted);
    tm.insert_tuple(t1, &mut heap, b"committed data").unwrap();
    tm.commit(t1).unwrap();

    // T2 scans AFTER T1 commits — RC gets a fresh snapshot per scan
    // so it should see T1's committed data
    let visible = tm.scan_visible_tuples(t2, &mut heap).unwrap();
    assert_eq!(
        visible.len(),
        1,
        "T2 (ReadCommitted) should see T1's committed tuple via fresh snapshot"
    );

    tm.commit(t2).unwrap();
}
