#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("Authentication failed for user '{0}'")]
    AuthFailed(String),
    #[error("Role '{0}' does not have login privilege")]
    NoLoginPrivilege(String),
    #[error("Catalog error: {0}")]
    Catalog(#[from] catalog::CatalogError),
    #[error("SCRAM error: {0}")]
    Scram(String),
}
