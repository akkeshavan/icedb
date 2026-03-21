use std::sync::Arc;
use tempfile::TempDir;

use crate::{hash_password, verify_password, AuthError, Authenticator};
use catalog::manager::CatalogManager;
use txn::manager::TransactionManager;
use wal::writer::WalWriter;

fn setup_catalog(dir: &std::path::Path) -> Arc<CatalogManager> {
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn = Arc::new(TransactionManager::new(Arc::clone(&wal)));
    Arc::new(CatalogManager::open(dir, wal, txn).unwrap())
}

#[test]
fn test_authenticate_success() {
    let dir = TempDir::new().unwrap();
    let catalog = setup_catalog(dir.path());

    // Create a role with a SCRAM-hashed password
    let hashed = hash_password("secret");
    let xid = catalog.txn_manager_begin();
    catalog
        .create_role(xid, "alice", false, true, Some(hashed))
        .unwrap();
    catalog.txn_manager_commit(xid).unwrap();

    let auth = Authenticator::new(Arc::clone(&catalog));
    assert!(auth.authenticate("alice", "secret").is_ok());
}

#[test]
fn test_authenticate_wrong_password() {
    let dir = TempDir::new().unwrap();
    let catalog = setup_catalog(dir.path());

    let hashed = hash_password("correct");
    let xid = catalog.txn_manager_begin();
    catalog
        .create_role(xid, "bob", false, true, Some(hashed))
        .unwrap();
    catalog.txn_manager_commit(xid).unwrap();

    let auth = Authenticator::new(Arc::clone(&catalog));
    let result = auth.authenticate("bob", "wrong");
    assert!(matches!(result, Err(AuthError::AuthFailed(_))));
}

#[test]
fn test_authenticate_no_login() {
    let dir = TempDir::new().unwrap();
    let catalog = setup_catalog(dir.path());

    let hashed = hash_password("pass");
    let xid = catalog.txn_manager_begin();
    // rolcanlogin = false
    catalog
        .create_role(xid, "nologin", false, false, Some(hashed))
        .unwrap();
    catalog.txn_manager_commit(xid).unwrap();

    let auth = Authenticator::new(Arc::clone(&catalog));
    let result = auth.authenticate("nologin", "pass");
    assert!(matches!(result, Err(AuthError::NoLoginPrivilege(_))));
}

#[test]
fn test_scram_verify() {
    // Test verify_password with SCRAM verifiers
    let hashed = hash_password("mypassword");
    assert!(verify_password(&hashed, "mypassword"));
    assert!(!verify_password(&hashed, "wrong"));

    // Test plaintext fallback (legacy passwords stored as-is)
    assert!(verify_password("mypassword", "mypassword"));
    assert!(!verify_password("mypassword", "wrong"));
    assert!(!verify_password("", "something"));
    assert!(verify_password("", ""));
}

#[test]
fn test_hash_password() {
    let hashed = hash_password("testpass");
    // SCRAM verifier must start with SCRAM-SHA-256 prefix
    assert!(hashed.starts_with("SCRAM-SHA-256$"));
    // Must verify correctly
    assert!(verify_password(&hashed, "testpass"));
    assert!(!verify_password(&hashed, "wrongpass"));
}

#[test]
fn test_scram_roundtrip() {
    let password = "complex_p@ssw0rd!";
    let verifier = hash_password(password);
    assert!(verify_password(&verifier, password));
    assert!(!verify_password(&verifier, "not_the_password"));
}
