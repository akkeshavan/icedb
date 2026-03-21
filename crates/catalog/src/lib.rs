pub mod encoding;
pub mod error;
pub mod manager;
pub mod oids;
pub mod pg_attribute;
pub mod pg_authid;
pub mod pg_class;
pub mod pg_namespace;
pub mod schema;
pub mod types;

pub use error::CatalogError;
pub use manager::CatalogManager;
pub use manager::ColumnStats;
pub use oids::*;
pub use pg_attribute::PgAttributeRow;
pub use pg_authid::PgAuthidRow;
pub use pg_class::PgClassRow;
pub use pg_namespace::PgNamespaceRow;
pub use schema::{ColumnDef, TableSchema};
pub use types::DataType;

#[cfg(test)]
mod tests;
