use std::io::{Seek, SeekFrom, Write};
use std::sync::Arc;

use tempfile::tempdir;

use storage::heap::HeapFile;
use storage::page::Page;
use storage::tuple::Tuple;

use crate::checkpoint::CheckpointManager;
use crate::lsn::INVALID_LSN;
use crate::reader::WalReader;
use crate::record::{WalRecord, WalRecordType};
use crate::recovery::RecoveryManager;
use crate::writer::WalWriter;

// ── helper ───────────────────────────────────────────────────────────────────

fn make_insert_data(slot: u16, tuple_bytes: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&slot.to_le_bytes());
    d.extend_from_slice(tuple_bytes);
    d
}

fn make_delete_data(slot: u16) -> Vec<u8> {
    slot.to_le_bytes().to_vec()
}

// ── test 1: append and read ───────────────────────────────────────────────────

#[test]
fn test_wal_append_and_read() {
    let dir = tempdir().unwrap();
    let log_dir = dir.path().join("wal");

    let writer = WalWriter::open(&log_dir).unwrap();

    // Append 5 records of different types.
    let lsn1 = writer
        .append(1, WalRecordType::Insert, 0, make_insert_data(0, b"hello"))
        .unwrap();
    let lsn2 = writer
        .append(1, WalRecordType::Delete, 0, make_delete_data(0))
        .unwrap();
    let lsn3 = writer
        .append(1, WalRecordType::Update, 0, {
            let mut d = Vec::new();
            d.extend_from_slice(&0u16.to_le_bytes()); // old_slot
            d.extend_from_slice(&1u32.to_le_bytes()); // new_page_no
            d.extend_from_slice(&1u16.to_le_bytes()); // new_slot
            d
        })
        .unwrap();
    let lsn4 = writer.append(1, WalRecordType::Commit, 0, vec![]).unwrap();
    let lsn5 = writer.append(2, WalRecordType::Abort, 0, vec![]).unwrap();

    writer.flush().unwrap();

    // Verify LSNs are monotonically increasing.
    assert!(lsn1 < lsn2 && lsn2 < lsn3 && lsn3 < lsn4 && lsn4 < lsn5);

    // Read them back.
    let mut reader = WalReader::open(&log_dir, INVALID_LSN).unwrap();

    let r1 = reader.next_record().unwrap().unwrap();
    assert_eq!(r1.lsn, lsn1);
    assert_eq!(r1.record_type, WalRecordType::Insert);
    assert_eq!(r1.xid, 1);
    assert_eq!(r1.page_no, 0);

    let r2 = reader.next_record().unwrap().unwrap();
    assert_eq!(r2.lsn, lsn2);
    assert_eq!(r2.record_type, WalRecordType::Delete);

    let r3 = reader.next_record().unwrap().unwrap();
    assert_eq!(r3.lsn, lsn3);
    assert_eq!(r3.record_type, WalRecordType::Update);

    let r4 = reader.next_record().unwrap().unwrap();
    assert_eq!(r4.lsn, lsn4);
    assert_eq!(r4.record_type, WalRecordType::Commit);

    let r5 = reader.next_record().unwrap().unwrap();
    assert_eq!(r5.lsn, lsn5);
    assert_eq!(r5.record_type, WalRecordType::Abort);
    assert_eq!(r5.xid, 2);

    // No more records.
    assert!(reader.next_record().unwrap().is_none());
}

// ── test 2: flush and durability ─────────────────────────────────────────────

#[test]
fn test_wal_flush_and_durability() {
    let dir = tempdir().unwrap();
    let log_dir = dir.path().join("wal");

    let lsn_commit = {
        let writer = WalWriter::open(&log_dir).unwrap();
        writer
            .append(
                42,
                WalRecordType::Insert,
                0,
                make_insert_data(0, b"durable"),
            )
            .unwrap();
        writer
            .append_and_flush(42, WalRecordType::Commit, 0, vec![])
            .unwrap()
    }; // writer dropped here

    // Re-open; LSN should have advanced past the commit.
    let writer2 = WalWriter::open(&log_dir).unwrap();
    // The next LSN assigned must be > lsn_commit.
    assert!(writer2.next_lsn() > lsn_commit);
}

// ── test 3: encode/decode round-trip ─────────────────────────────────────────

#[test]
fn test_wal_record_encode_decode() {
    let cases: &[(WalRecordType, u32, Vec<u8>)] = &[
        (WalRecordType::Insert, 5, make_insert_data(3, b"round-trip")),
        (WalRecordType::Delete, 5, make_delete_data(3)),
        (WalRecordType::Update, 2, {
            let mut d = Vec::new();
            d.extend_from_slice(&7u16.to_le_bytes());
            d.extend_from_slice(&9u32.to_le_bytes());
            d.extend_from_slice(&2u16.to_le_bytes());
            d
        }),
        (WalRecordType::Commit, 0, vec![]),
        (WalRecordType::Abort, 0, vec![]),
        (WalRecordType::Checkpoint, 0, vec![]),
        (WalRecordType::PageImage, 11, vec![0xABu8; 8192]),
    ];

    for (rt, page_no, data) in cases {
        let mut rec = WalRecord::new(99, *rt, *page_no, data.clone());
        rec.lsn = 42;
        rec.prev_lsn = 41;

        let encoded = rec.encode();
        let decoded = WalRecord::decode(&encoded).unwrap();

        assert_eq!(decoded.lsn, 42, "lsn mismatch for {rt:?}");
        assert_eq!(decoded.prev_lsn, 41, "prev_lsn mismatch for {rt:?}");
        assert_eq!(decoded.xid, 99, "xid mismatch for {rt:?}");
        assert_eq!(decoded.record_type, *rt, "record_type mismatch");
        assert_eq!(decoded.page_no, *page_no, "page_no mismatch for {rt:?}");
        assert_eq!(&decoded.data, data, "data mismatch for {rt:?}");
    }
}

// ── test 4: corrupt record detection ─────────────────────────────────────────

#[test]
fn test_wal_corrupt_record() {
    let dir = tempdir().unwrap();
    let log_dir = dir.path().join("wal");

    let writer = WalWriter::open(&log_dir).unwrap();
    writer
        .append(
            1,
            WalRecordType::Insert,
            0,
            make_insert_data(0, b"corrupt me"),
        )
        .unwrap();
    writer.flush().unwrap();
    drop(writer);

    // Find and corrupt a byte in the WAL segment file.
    let seg_path = log_dir.join("0000000000000001.wal");
    {
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&seg_path)
            .unwrap();
        // Flip a byte somewhere in the middle of the record (past the header).
        f.seek(SeekFrom::Start(20)).unwrap();
        let mut byte = [0u8; 1];
        f.read_exact(&mut byte).unwrap();

        // Use Read from std::io
        use std::io::Read;
        let _ = byte; // suppress warning

        // Rewind and write a corrupted byte.
        f.seek(SeekFrom::Start(20)).unwrap();
        f.write_all(&[0xFFu8]).unwrap();
    }

    let mut reader = WalReader::open(&log_dir, INVALID_LSN).unwrap();
    let result = reader.next_record();
    assert!(
        matches!(result, Err(crate::WalError::CorruptRecord { .. })),
        "expected CorruptRecord, got: {:?}",
        result
    );
}

// ── test 5: checkpoint write and read ────────────────────────────────────────

#[test]
fn test_checkpoint_write_and_read() {
    let dir = tempdir().unwrap();
    let log_dir = dir.path().join("wal");

    let writer = Arc::new(WalWriter::open(&log_dir).unwrap());
    let mut ckpt_mgr = CheckpointManager::new(&log_dir, Arc::clone(&writer)).unwrap();

    assert_eq!(ckpt_mgr.last_checkpoint_lsn(), INVALID_LSN);

    // Build 2 dirty pages.
    let mut page0 = Page::new();
    page0.insert_tuple(b"page zero tuple").unwrap();
    let mut page1 = Page::new();
    page1.insert_tuple(b"page one tuple").unwrap();

    let dirty = vec![(0u32, &page0), (1u32, &page1)];
    let ckpt_lsn = ckpt_mgr.perform_checkpoint(&dirty).unwrap();

    assert!(ckpt_lsn > INVALID_LSN);
    assert_eq!(ckpt_mgr.last_checkpoint_lsn(), ckpt_lsn);

    // Re-open a fresh CheckpointManager; it should read the saved LSN.
    let writer2 = Arc::new(WalWriter::open(&log_dir).unwrap());
    let ckpt_mgr2 = CheckpointManager::new(&log_dir, Arc::clone(&writer2)).unwrap();
    assert_eq!(ckpt_mgr2.last_checkpoint_lsn(), ckpt_lsn);
}

// ── test 6: recovery replay ───────────────────────────────────────────────────

#[test]
fn test_recovery_replay() {
    let dir = tempdir().unwrap();
    let log_dir = dir.path().join("wal");
    let heap_path = dir.path().join("heap.db");

    // ── Phase 1: write data + WAL ────────────────────────────────────────────
    let (tid1, tid3, del_page_no, del_slot) = {
        let writer = Arc::new(WalWriter::open(&log_dir).unwrap());
        let mut heap = HeapFile::open(&heap_path).unwrap();

        // Insert 3 tuples.
        let t1 = Tuple::new(b"tuple one".to_vec());
        let t2 = Tuple::new(b"tuple two".to_vec());
        let t3 = Tuple::new(b"tuple three".to_vec());

        let tid1 = heap.insert_tuple(&t1).unwrap();
        let tid2 = heap.insert_tuple(&t2).unwrap();
        let tid3 = heap.insert_tuple(&t3).unwrap();

        // Write PageImage WAL records for all touched pages.
        let num_pages = heap.num_pages();
        for p in 0..num_pages {
            let page = heap.read_page(p).unwrap();
            writer
                .append(1, WalRecordType::PageImage, p, page.as_bytes().to_vec())
                .unwrap();
        }

        // Checkpoint.
        let mut ckpt_mgr = CheckpointManager::new(&log_dir, Arc::clone(&writer)).unwrap();
        {
            let pages: Vec<(u32, Page)> = (0..num_pages)
                .map(|p| (p, heap.read_page(p).unwrap()))
                .collect();
            let refs: Vec<(u32, &Page)> = pages.iter().map(|(p, pg)| (*p, pg)).collect();
            ckpt_mgr.perform_checkpoint(&refs).unwrap();
        }

        // Delete tuple 2 after checkpoint.
        let del_page_no = tid2.page_no;
        let del_slot = tid2.slot;

        heap.delete_tuple(tid2).unwrap();

        // WAL PageImage record for the modified page (correctness: PageImage-only recovery).
        let del_page = heap.read_page(del_page_no).unwrap();
        writer
            .append_and_flush(
                1,
                WalRecordType::PageImage,
                del_page_no,
                del_page.as_bytes().to_vec(),
            )
            .unwrap();

        // Simulate crash: drop without a clean shutdown.
        (tid1, tid3, del_page_no, del_slot)
    };

    // ── Phase 2: recovery ────────────────────────────────────────────────────
    // Erase in-memory state by re-opening everything.
    let mut heap = HeapFile::open(&heap_path).unwrap();
    let recovery = RecoveryManager::new(&log_dir);
    let last_lsn = recovery.recover(&mut heap).unwrap();
    assert!(
        last_lsn > INVALID_LSN,
        "expected at least one record replayed"
    );

    // Tuple 1 and 3 must be readable.
    assert!(
        heap.get_tuple(tid1).is_ok(),
        "tuple 1 should be readable after recovery"
    );
    assert!(
        heap.get_tuple(tid3).is_ok(),
        "tuple 3 should be readable after recovery"
    );

    // Tuple 2 must be deleted.
    let page = heap.read_page(del_page_no).unwrap();
    let slot_result = page.get_tuple(del_slot);
    assert!(
        slot_result.is_err(),
        "tuple 2 slot should be dead after recovery, got: {:?}",
        slot_result
    );
}

// ── test 7: segment rotation ─────────────────────────────────────────────────

#[test]
fn test_segment_rotation() {
    let dir = tempdir().unwrap();
    let log_dir = dir.path().join("wal");

    // Use a tiny segment (1 KiB) to force rotation quickly.
    let writer = WalWriter::open_with_segment_size(&log_dir, 1024).unwrap();

    // Write enough records to span >3 segments.
    // Each record is roughly 33 + 100 + 4 = 137 bytes, so ~8 records per 1 KiB segment.
    let mut lsns = Vec::new();
    for i in 0u32..40 {
        let data = make_insert_data(i as u16, &[0xCCu8; 100]);
        let lsn = writer.append(i, WalRecordType::Insert, i, data).unwrap();
        lsns.push(lsn);
    }
    writer.flush().unwrap();

    // Verify at least 3 segment files were created.
    let segments = WalWriter::list_segments(&log_dir).unwrap();
    assert!(
        segments.len() >= 3,
        "expected >= 3 segments, got {}",
        segments.len()
    );

    // Read all records back and verify we see all 40.
    let mut reader = WalReader::open(&log_dir, INVALID_LSN).unwrap();
    let mut count = 0usize;
    while let Some(rec) = reader.next_record().unwrap() {
        assert_eq!(rec.lsn, lsns[count], "LSN mismatch at record {}", count);
        count += 1;
    }
    assert_eq!(count, 40, "expected 40 records, read {}", count);
}
