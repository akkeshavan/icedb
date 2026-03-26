/// SQL Conformance Test Suite
///
/// Covers PostgreSQL regression test patterns, TPC-H queries, Hermitage
/// isolation tests, and SQL standard feature compliance.
///
/// Tests that require unimplemented features are marked #[ignore] with a
/// clear reason. Do NOT remove them — they document planned functionality.
pub mod select;
pub mod null_handling;
pub mod aggregates;
pub mod joins;
pub mod subqueries;
pub mod set_operations;
pub mod ctes;
pub mod dml;
pub mod ddl;
pub mod transactions;
pub mod error_handling;
pub mod tpch;
pub mod hermitage;
pub mod new_features;
pub mod catalog_views;
pub mod advanced_features;
pub mod boolean_type;
pub mod case_expr;
pub mod int_types;
pub mod float_types;
pub mod string_functions;
pub mod date_type;
pub mod timestamp_type;
pub mod limit_offset;
pub mod array_json;
