# The icedb User Guide

**icedb** is a production-grade, PostgreSQL-compatible relational database built in Rust. It speaks the PostgreSQL wire protocol, enforces full ACID guarantees through WAL and MVCC, stores data in 8 kB slotted pages, and ships native drivers for Rust, Python, and Node.js.

This book is the complete reference for anyone using, operating, or building on icedb — from installing it for the first time to understanding the internals well enough to contribute.

---

## Table of Contents

| Chapter | Title | What it covers |
|---------|-------|----------------|
| [Chapter 1](ch01-introduction.md) | Introduction | What icedb is, design goals, how it differs from SQLite/PostgreSQL/TiKV, architecture overview |
| [Chapter 2](ch02-installation.md) | Installation & Building from Source | Prerequisites, cloning, building, running the test suite, smoke test |
| [Chapter 3](ch03-quickstart.md) | Quick Start: Your First Database | Starting the server and CLI, creating tables, inserting and querying data — using a complete bookstore example |
| [Chapter 4](ch04-sql-reference.md) | SQL Reference | All supported data types, DDL, DML, SELECT, JOINs, aggregates, transaction control, and a honest list of unsupported features |
| [Chapter 5](ch05-transactions-and-acid.md) | Transactions & ACID Guarantees | How atomicity, consistency, isolation, and durability work; MVCC explained; isolation levels with anomaly examples; WAL and crash recovery |
| [Chapter 6](ch06-indexes.md) | Indexes & Query Performance | The B+ tree index, creating and using indexes, range scans, the query planner, when not to index |
| [Chapter 7](ch07-security.md) | Authentication & Security | SCRAM-SHA-256, password storage format, role-based access control, privilege flags |
| [Chapter 8](ch08-cli-reference.md) | CLI Reference (nkv-psql) | All flags, environment variables, meta-commands, tab completion, output formatting |
| [Chapter 9](ch09-drivers.md) | Client Drivers | Rust, Python, and Node.js embedded drivers — connection, queries, transactions, type mapping |
| [Chapter 10](ch10-architecture.md) | Architecture Deep Dive | Every layer from wire protocol to disk; page layout diagrams; MVCC timeline; WAL write-ahead rule; recovery procedure |
| [Chapter 11](ch11-operations.md) | Running in Production | Data directory layout, systemd unit, backup, monitoring, shutdown, known limitations |
| [Chapter 12](ch12-troubleshooting.md) | Troubleshooting | Common problems and solutions, debug logging, WAL inspection, index rebuilding |
| [Chapter 13](ch13-admin-ui.md) | Admin UI | The web-based administration interface: setup, role management, schema browser, query console, REST API reference |
| [Chapter 14](ch14-roadmap.md) | Roadmap & Known Limitations | Every unimplemented feature, why it is absent, what is required to add it, and a priority order for contributors |

---

## Version

Last updated: 2026-03-19. This documentation covers the current implementation state: storage, WAL, transactions, B+ tree indexes, system catalog, SQL engine, CLI, cross-language drivers, and the Admin UI. 325 tests pass with 0 failures. The wire protocol (Phase 7) and production hardening (Phase 10) are the primary remaining milestones. See [Chapter 14](ch14-roadmap.md) for a detailed breakdown of what is not yet implemented and why.

For audience descriptions, prerequisites, and role-based reading paths, see [Chapter 1](ch01-introduction.md).
