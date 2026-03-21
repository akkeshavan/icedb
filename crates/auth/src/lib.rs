pub mod authenticator;
pub mod error;
pub mod scram;

#[cfg(test)]
mod tests;

pub use authenticator::Authenticator;
pub use error::AuthError;
pub use scram::{hash_password, verify_password};
