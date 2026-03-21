use crate::error::AuthError;
use catalog::manager::CatalogManager;
use std::sync::Arc;

pub struct Authenticator {
    catalog: Arc<CatalogManager>,
}

impl Authenticator {
    pub fn new(catalog: Arc<CatalogManager>) -> Self {
        Self { catalog }
    }

    /// Authenticate a user by username and plaintext password.
    /// Returns Ok(()) on success, Err(AuthError) on failure.
    pub fn authenticate(&self, username: &str, password: &str) -> Result<(), AuthError> {
        let role = self
            .catalog
            .get_role(username)
            .map_err(|_| AuthError::AuthFailed(username.to_string()))?;
        if !role.rolcanlogin {
            return Err(AuthError::NoLoginPrivilege(username.to_string()));
        }
        // No password set → trust mode (accept any password, including empty)
        if let Some(verifier) = role.rolpassword.as_deref() {
            if !crate::scram::verify_password(verifier, password) {
                return Err(AuthError::AuthFailed(username.to_string()));
            }
        }
        Ok(())
    }
}
