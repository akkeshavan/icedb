# Chapter 1: Introduction

## What is icedb?

icedb is a relational database management system written in Rust. It implements the PostgreSQL wire protocol version 3.0, which means that any client or tool that can talk to PostgreSQL — `psql`, DBeaver, pgAdmin, language drivers — can talk to icedb without modification. Under the hood, icedb is an original implementation: it does not wrap or embed PostgreSQL. Every byte of storage layout, every page of the B+ tree index, every WAL record, and every MVCC visibility rule was built from scratch.

The name reflects the layered, cold-storage-first philosophy: data lands on immutable 8 kB pages, change is tracked through versioned tuple headers, and nothing reaches disk until the WAL has made it safe.

## Why icedb Was Built

Production databases are extraordinarily complex, and most of that complexity is opaque. icedb exists to be a database whose entire source can be read and understood — where every design decision traces back to a clear specification. It is also a proving ground for Rust's suitability as a systems language for storage-heavy, concurrent, correctness-critical code.

The secondary goal is PostgreSQL compatibility. By conforming to the wire protocol and honoring standard SQL semantics, icedb can be dropped into existing toolchains without adapter layers. Applications built against PostgreSQL work against icedb with no code changes.

## Design Goals

**PostgreSQL compatibility.** icedb speaks wire protocol v3.0. Both Simple Query (the `Q` message) and Extended Query (`Parse` / `Bind` / `Execute` / `Sync`) are implemented. Server version is reported as `16.0 (icedb)`. Standard PostgreSQL tools connect without configuration.

**Rust safety.** The entire server is written in safe Rust (with narrow `unsafe` regions in the storage layer for raw page I/O). Memory safety, thread safety, and absence of data races are enforced at compile time. The buffer pool is a fixed-size allocation — there is no dynamic growth that could trigger out-of-memory conditions under load.

**ACID compliance.** Every committed transaction is durable to disk via the Write-Ahead Log. Isolation is provided by Multi-Version Concurrency Control: readers never block writers, writers never block readers. Three isolation levels are supported: Read Committed, Repeatable Read, and Serializable (with SSI infrastructure in place).

**Transparency of implementation.** The code maps directly to well-understood concepts: a slotted page layout that mirrors PostgreSQL's `PageHeaderData`, MVCC tuple headers with `t_xmin`/`t_xmax` fields, a B+ tree where every node is exactly one 8 kB page, and a WAL that enforces the write-ahead rule unconditionally.

**Cross-language reach.** Native embedded drivers are provided for Rust (direct in-process), Python (PyO3/Maturin), and Node.js (NAPI-RS). All three drivers bypass TCP and call the query engine directly, eliminating network overhead for embedded use cases.

## How icedb Differs From Other Databases

**vs. SQLite.** SQLite is an embedded database with no server process, no network protocol, and single-writer concurrency. icedb runs as a server process, speaks TCP, supports concurrent readers and writers through MVCC, and targets applications that outgrow SQLite's concurrency model. The embedded drivers give icedb SQLite-like convenience when a network server is not needed.

**vs. PostgreSQL.** PostgreSQL is a mature, production-hardened system with decades of development. icedb is not a replacement for PostgreSQL. It is a clean-room implementation of the same concepts, written in Rust, with full source transparency. icedb lacks PostgreSQL's extension ecosystem, replication, tablespaces, partitioning, and vacuum daemon. It is appropriate for embedded use, educational deployments, and applications that value Rust-native integration.

**vs. TiKV / FoundationDB / distributed systems.** These are distributed key-value stores. icedb is a single-node relational database with a full SQL engine. There is no sharding, no Raft consensus, and no horizontal scaling. icedb makes a different trade-off: it is simpler and fully understandable, at the cost of being constrained to a single machine.

**vs. SQLx / Diesel (Rust ORMs).** These are client libraries that talk to external database servers. icedb is the server itself, plus embedded driver libraries that can be used without a server process at all.

## High-Level Architecture

The request path from a connecting client to durable storage crosses nine layers, each implemented as a separate Rust crate:

```
Client (psql / app driver)
        │  TCP   PostgreSQL Wire Protocol v3.0
        ▼
  network/   — pgwire crate; Simple + Extended Query; message framing
        │
        ├──► auth/  — password verification (SCRAM-SHA-256 / cleartext)
        │            RBAC: check role privileges before executing
        ▼
  sql/   — Parser (sqlparser-rs, PostgreSQL dialect)
           Planner (logical plan from AST)
           Executor (Volcano/iterator model)
        │
        ▼
  txn/   — Transaction manager; XID allocator
           Snapshot isolation; MVCC visibility rules
           Two-phase locking (write-write conflict prevention)
           SSI read/write set tracking
        │
        ├──► catalog/  — pg_class, pg_attribute, pg_authid, pg_namespace
        │               in-memory schema cache; OID registry; index registry
        │
        ├──► btree/    — Persistent B+ tree index (8 kB pages)
        │               latch crabbing; WAL-logged splits
        │
        ▼
  storage/  — Slotted 8 kB pages; heap files; buffer pool
              Clock/Second-Chance eviction; FNV-1a page checksums
        │
        ▼
  wal/   — Append-only WAL writer; segment rotation (16 MiB per segment)
           fsync on commit; checkpoint.ctl; redo-only recovery
        │
        ▼
      Disk  (8 kB page files: <oid>.heap, idx_<oid>_<col>.btree, <seg>.wal)
```

The arrows show the direction of calls. Control flows downward; data flows back up. The WAL is the only layer that writes unconditionally before any other layer persists data to disk.

## Who This Guide Is For

This guide assumes you are comfortable with SQL and have used a relational database before. You do not need to know Rust to use icedb as a server or through the Python/Node.js drivers. If you want to use the Rust driver or contribute to icedb, familiarity with Rust's ownership model will help with the driver API sections and the architecture chapter.

System administrators responsible for running icedb in production will find Chapters 2, 11, and 12 most directly relevant. Application developers should focus on Chapters 3, 4, 5, and 9. Anyone curious about how a database actually works should read Chapter 10.

## How to Read This Book

- **First-time users**: Start at Chapter 2 (Installation), then work through Chapter 3 (Quick Start). You can read the SQL Reference (Chapter 4) alongside as a lookup resource.
- **Application developers**: After the quick start, focus on Chapter 4 (SQL), Chapter 5 (Transactions), and Chapter 9 (Drivers).
- **Operations engineers**: Chapter 2, Chapter 3 (just the server-start section), Chapter 11, and Chapter 12 are your core reading.
- **Contributors and advanced users**: Chapter 10 (Architecture) documents every design decision and internal data structure. Chapter 5 (Transactions) and Chapter 6 (Indexes) provide the conceptual foundation.

Each chapter is self-contained. You do not need to have read every preceding chapter to use a later one.

The chapters are ordered from "getting started" to "deep internals." Chapter 4 is a reference chapter you can bookmark and return to. Chapter 5 on transactions is the most important chapter for understanding why icedb behaves the way it does under concurrent load. Chapters 10, 11, and 12 assume you have used icedb and want to understand it more deeply or operate it reliably.

Every code example in this guide uses concrete table and column names. No `foo`/`bar` placeholder examples appear. Where command output is shown, it is the actual output of the current implementation.

Version information and the documentation coverage status are in the [README](README.md).
