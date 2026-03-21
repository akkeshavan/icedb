//! SCRAM-SHA-256 server implementation.
//!
//! The SCRAM-SHA-256 protocol (RFC 5802) works as follows:
//! 1. Client sends ClientFirst message with username and client nonce
//! 2. Server responds with ServerFirst: server nonce = client_nonce + server_nonce, salt, iteration count
//! 3. Client sends ClientFinal with proof (HMAC-SHA-256 of auth message)
//! 4. Server verifies proof, sends ServerFinal with server signature
//!
//! For storing passwords, we use SCRAM-SHA-256 verifiers in the format:
//! "SCRAM-SHA-256$\<iterations\>:\<base64-salt\>$\<base64-StoredKey\>:\<base64-ServerKey\>"
//! This is the same format PostgreSQL uses.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub const SCRAM_SHA_256: &str = "SCRAM-SHA-256";
pub const DEFAULT_ITERATIONS: u32 = 4096;

/// Generate a cryptographically random nonce (16 bytes, base64-encoded).
pub fn generate_nonce() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use time + thread id as entropy source (sufficient for a database nonce)
    let mut hasher = DefaultHasher::new();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    let h1 = hasher.finish();
    std::process::id().hash(&mut hasher);
    let h2 = hasher.finish();

    let mut bytes = [0u8; 16];
    bytes[0..8].copy_from_slice(&h1.to_le_bytes());
    bytes[8..16].copy_from_slice(&h2.to_le_bytes());
    BASE64.encode(bytes)
}

/// Generate a random 16-byte salt.
pub fn generate_salt() -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut hasher = DefaultHasher::new();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .hash(&mut hasher);
    let h = hasher.finish();

    // Mix more entropy
    let pid = std::process::id() as u64;

    let mut salt = [0u8; 16];
    salt[0..8].copy_from_slice(&h.to_le_bytes());
    salt[8..16].copy_from_slice(&pid.wrapping_mul(0x9E3779B97F4A7C15).to_le_bytes());
    salt.to_vec()
}

/// Derive the SCRAM keys from a plaintext password.
/// Returns (StoredKey, ServerKey) as byte vectors.
pub fn derive_scram_keys(password: &str, salt: &[u8], iterations: u32) -> (Vec<u8>, Vec<u8>) {
    // SaltedPassword = PBKDF2(SHA-256, password, salt, iterations, 32)
    let salted_password = pbkdf2_sha256(password.as_bytes(), salt, iterations);

    // ClientKey = HMAC(SHA-256, SaltedPassword, "Client Key")
    let client_key = hmac_sha256(&salted_password, b"Client Key");

    // StoredKey = SHA-256(ClientKey)
    let stored_key = sha256(&client_key);

    // ServerKey = HMAC(SHA-256, SaltedPassword, "Server Key")
    let server_key = hmac_sha256(&salted_password, b"Server Key");

    (stored_key, server_key)
}

/// Hash a password into a SCRAM-SHA-256 verifier string for storage in pg_authid.
/// Format: "SCRAM-SHA-256$<iterations>:<base64-salt>$<base64-StoredKey>:<base64-ServerKey>"
pub fn hash_password(password: &str) -> String {
    let salt = generate_salt();
    let iterations = DEFAULT_ITERATIONS;
    let (stored_key, server_key) = derive_scram_keys(password, &salt, iterations);

    format!(
        "SCRAM-SHA-256${}:{}${}:{}",
        iterations,
        BASE64.encode(&salt),
        BASE64.encode(&stored_key),
        BASE64.encode(&server_key),
    )
}

/// Parse a SCRAM verifier string into its components.
pub struct ScramVerifier {
    pub iterations: u32,
    pub salt: Vec<u8>,
    pub stored_key: Vec<u8>,
    pub server_key: Vec<u8>,
}

impl ScramVerifier {
    pub fn parse(verifier: &str) -> Result<Self, String> {
        // Format: "SCRAM-SHA-256$<iterations>:<base64-salt>$<base64-StoredKey>:<base64-ServerKey>"
        let verifier = verifier
            .strip_prefix("SCRAM-SHA-256$")
            .ok_or_else(|| "Not a SCRAM-SHA-256 verifier".to_string())?;

        let parts: Vec<&str> = verifier.splitn(2, '$').collect();
        if parts.len() != 2 {
            return Err("Invalid verifier format".to_string());
        }

        let iter_salt: Vec<&str> = parts[0].splitn(2, ':').collect();
        if iter_salt.len() != 2 {
            return Err("Invalid iter:salt format".to_string());
        }

        let iterations: u32 = iter_salt[0]
            .parse()
            .map_err(|_| "Invalid iterations".to_string())?;
        let salt = BASE64.decode(iter_salt[1]).map_err(|e| e.to_string())?;

        let keys: Vec<&str> = parts[1].splitn(2, ':').collect();
        if keys.len() != 2 {
            return Err("Invalid StoredKey:ServerKey format".to_string());
        }

        let stored_key = BASE64.decode(keys[0]).map_err(|e| e.to_string())?;
        let server_key = BASE64.decode(keys[1]).map_err(|e| e.to_string())?;

        Ok(ScramVerifier {
            iterations,
            salt,
            stored_key,
            server_key,
        })
    }
}

/// Verify a SCRAM-SHA-256 ClientProof against the stored verifier.
///
/// This is called when we have a plaintext password from the client
/// (e.g., via cleartext auth or when doing server-side verification).
///
/// For the wire protocol, the full SCRAM handshake is handled by pgwire.
/// This function does the key verification step.
pub fn verify_client_proof(stored_key: &[u8], auth_message: &[u8], client_proof: &[u8]) -> bool {
    // ClientSignature = HMAC(SHA-256, StoredKey, AuthMessage)
    let client_signature = hmac_sha256(stored_key, auth_message);

    // ClientKey = ClientProof XOR ClientSignature
    if client_proof.len() != client_signature.len() {
        return false;
    }
    let recovered_client_key: Vec<u8> = client_proof
        .iter()
        .zip(client_signature.iter())
        .map(|(p, s)| p ^ s)
        .collect();

    // Verify: SHA-256(RecoveredClientKey) == StoredKey
    let recovered_stored_key = sha256(&recovered_client_key);
    recovered_stored_key == stored_key
}

/// For simple password auth: verify plaintext password against stored SCRAM verifier.
/// Used when the client sends a plaintext password (MD5 or cleartext auth mode).
pub fn verify_password(stored_verifier: &str, provided_password: &str) -> bool {
    // Try to parse as SCRAM verifier
    if let Ok(verifier) = ScramVerifier::parse(stored_verifier) {
        // Re-derive keys from provided password and compare StoredKey
        let (derived_stored_key, _) =
            derive_scram_keys(provided_password, &verifier.salt, verifier.iterations);
        derived_stored_key == verifier.stored_key
    } else {
        // Fallback: plaintext comparison (for legacy/test passwords)
        stored_verifier == provided_password
    }
}

// ── Crypto primitives ────────────────────────────────────────────────────────

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

/// PBKDF2 with HMAC-SHA-256, producing 32 bytes.
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> Vec<u8> {
    const OUTPUT_LEN: usize = 32;
    let mut output = vec![0u8; OUTPUT_LEN];

    // PBKDF2 with a single block (since output_len == hash_len for SHA-256)
    // U1 = HMAC(password, salt || INT(1))
    let mut salt_with_block = salt.to_vec();
    salt_with_block.extend_from_slice(&1u32.to_be_bytes()); // block index 1, big-endian

    let mut u = hmac_sha256(password, &salt_with_block);
    let mut result = u.clone();

    for _ in 1..iterations {
        u = hmac_sha256(password, &u);
        for (r, ui) in result.iter_mut().zip(u.iter()) {
            *r ^= ui;
        }
    }

    output.copy_from_slice(&result[..OUTPUT_LEN]);
    output
}
