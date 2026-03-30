#[derive(thiserror::Error, Debug)]
pub enum CliError {
    #[error("SQL error: {0}")]
    Sql(#[from] sql::SqlError),
    #[error("Catalog error: {0}")]
    Catalog(#[from] catalog::CatalogError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Readline error: {0}")]
    Readline(String),
    #[error("Unknown meta-command: {0}")]
    UnknownMetaCommand(String),
    #[error("Network error: {0}")]
    Network(String),
}
