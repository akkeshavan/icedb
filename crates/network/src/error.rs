#[derive(thiserror::Error, Debug)]
pub enum NetworkError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQL error: {0}")]
    Sql(#[from] sql::SqlError),
    #[error("Auth error: {0}")]
    Auth(#[from] auth::AuthError),
    #[error("Protocol error: {0}")]
    Protocol(String),
}
