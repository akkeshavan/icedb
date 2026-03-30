//! Network server lifecycle management for integration tests.
//!
//! Starts a real `icedb-server` process on a free port and provides helpers
//! to create `PgClient` connections to it.  Two global server instances are
//! managed via `OnceLock`: one plain TCP and one TLS.
//!
//! Each test that uses network mode gets its own isolated database (named
//! after the test function) so tests do not interfere with each other.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use once_cell::sync::OnceCell;
use tempfile::TempDir;

use cli::config::{Config, SslMode};
use cli::pg_client::PgClient;

// ── NetServer ────────────────────────────────────────────────────────────────

pub struct NetServer {
    pub port: u16,
    pub use_tls: bool,
    /// Cert dir is kept alive as long as the server is running.
    _cert_dir: Option<TempDir>,
    /// Data dir is kept alive as long as the server is running.
    _data_dir: TempDir,
    _process: std::sync::Mutex<Child>,
}

impl NetServer {
    /// Start an icedb-server on a free port.  Blocks until the server is
    /// ready to accept connections (up to 10 seconds).
    pub fn start(use_tls: bool) -> Self {
        let port = free_port();
        let data_dir = TempDir::new().expect("TempDir for net server data");

        let server_bin = find_server_binary();

        let mut cmd = Command::new(&server_bin);
        cmd.arg("--port").arg(port.to_string());
        cmd.arg("--data-dir").arg(data_dir.path());

        let cert_dir: Option<TempDir> = if use_tls {
            let cdir = generate_self_signed_cert();
            cmd.arg("--tls-cert")
                .arg(cdir.path().join("server.crt"))
                .arg("--tls-key")
                .arg(cdir.path().join("server.key"));
            Some(cdir)
        } else {
            None
        };

        // Redirect server stdout/stderr to null so they don't inherit the test
        // process's pipe and block `| tail -N` style test invocations.
        cmd.stdout(std::process::Stdio::null())
           .stderr(std::process::Stdio::null());

        let child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("Failed to start server binary '{}': {}", server_bin.display(), e));

        let server = NetServer {
            port,
            use_tls,
            _cert_dir: cert_dir,
            _data_dir: data_dir,
            _process: std::sync::Mutex::new(child),
        };

        // Wait for the server to be ready
        server.wait_for_ready(Duration::from_secs(15));
        server
    }

    /// Wait until the server accepts a TCP connection and returns a valid
    /// PostgreSQL response, up to `timeout`.
    fn wait_for_ready(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let addr = format!("127.0.0.1:{}", self.port);

        loop {
            if Instant::now() > deadline {
                panic!("icedb-server did not become ready on port {} within {:?}", self.port, timeout);
            }

            // Try to open a TCP connection
            if std::net::TcpStream::connect(&addr).is_ok() {
                // Now try a real PgClient connect
                let config = self.make_config("icedb");
                if PgClient::connect(&config).is_ok() {
                    return;
                }
            }

            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn make_config(&self, dbname: &str) -> Config {
        Config {
            data_dir: String::new(),
            host: "127.0.0.1".to_string(),
            port: self.port,
            username: "icedb".to_string(),
            dbname: dbname.to_string(),
            password: None,
            history_file: None,
            sslmode: if self.use_tls { SslMode::Require } else { SslMode::Disable },
            sslrootcert: None,
            explicit_host: true,
        }
    }

    /// Create a new PgClient connected to the default `icedb` database.
    pub fn new_client(&self) -> PgClient {
        PgClient::connect(&self.make_config("icedb"))
            .unwrap_or_else(|e| panic!("Failed to connect to test server on port {}: {}", self.port, e))
    }

    /// Create a new PgClient connected to an isolated database for a test.
    pub fn new_isolated_client(&self, db_name: &str) -> PgClient {
        PgClient::connect(&self.make_config(db_name))
            .unwrap_or_else(|e| panic!("Failed to connect to database '{}' on port {}: {}", db_name, self.port, e))
    }
}

impl Drop for NetServer {
    fn drop(&mut self) {
        let mut child = self._process.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
}

// ── Global server instances ───────────────────────────────────────────────────

static PLAIN_SERVER: OnceCell<NetServer> = OnceCell::new();
static TLS_SERVER: OnceCell<NetServer> = OnceCell::new();

pub fn plain_server() -> &'static NetServer {
    PLAIN_SERVER.get_or_init(|| NetServer::start(false))
}

pub fn tls_server() -> &'static NetServer {
    TLS_SERVER.get_or_init(|| NetServer::start(true))
}

// ── Backend factories ─────────────────────────────────────────────────────────

/// Create a `Backend::Network` connected to the plain TCP server, using an
/// isolated database named after the test.
pub fn plain_backend(test_name: &str) -> crate::common::Backend {
    let server = plain_server();
    let db_name = sanitize_test_name(test_name);
    provision_database(server, &db_name);
    let client = server.new_isolated_client(&db_name);
    crate::common::Backend::Network(std::sync::Mutex::new(client))
}

/// Create a `Backend::Network` connected to the TLS server, using an
/// isolated database named after the test.
pub fn tls_backend(test_name: &str) -> crate::common::Backend {
    let server = tls_server();
    let db_name = sanitize_test_name(test_name);
    provision_database(server, &db_name);
    let client = server.new_isolated_client(&db_name);
    crate::common::Backend::Network(std::sync::Mutex::new(client))
}

/// Drop (if exists) and recreate an isolated database for a test.
fn provision_database(server: &NetServer, db_name: &str) {
    let mut admin = server.new_client();
    // Best-effort drop
    let _ = admin.query(&format!("DROP DATABASE IF EXISTS {}", db_name));
    admin.query(&format!("CREATE DATABASE {}", db_name))
        .unwrap_or_else(|e| panic!("Failed to CREATE DATABASE {}: {}", db_name, e));
}

/// Convert a test name into a valid SQL identifier (lowercase alphanumeric + underscores).
pub fn sanitize_test_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .to_lowercase();
    // Prefix with "t_" to ensure it starts with a letter and doesn't clash
    // with reserved words (e.g. "select" would be a reserved word).
    format!("t_{}", s)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Find the icedb-server binary relative to the tests crate.
fn find_server_binary() -> PathBuf {
    // Try debug build first, then release
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| ".".to_string());
    let base = PathBuf::from(&manifest_dir).join("..").join("target");

    let debug = base.join("debug").join("icedb-server");
    if debug.exists() {
        return debug.canonicalize().unwrap_or(debug);
    }

    let release = base.join("release").join("icedb-server");
    if release.exists() {
        return release.canonicalize().unwrap_or(release);
    }

    // Fallback: search PATH
    PathBuf::from("icedb-server")
}

/// Pick a free TCP port by binding to port 0 and reading back the assigned port.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to port 0");
    listener.local_addr().expect("local_addr").port()
}

/// Generate a self-signed TLS certificate for testing using `openssl` or
/// `rcgen`.  Falls back to a panic with a helpful message if neither is
/// available.
///
/// Returns a TempDir containing `server.crt` and `server.key`.
fn generate_self_signed_cert() -> TempDir {
    let dir = TempDir::new().expect("TempDir for TLS certs");
    let cert_path = dir.path().join("server.crt");
    let key_path = dir.path().join("server.key");

    // Try openssl command
    let status = Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey", "rsa:2048",
            "-keyout", key_path.to_str().unwrap(),
            "-out", cert_path.to_str().unwrap(),
            "-days", "1",
            "-nodes",
            "-subj", "/CN=localhost",
        ])
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {}
        _ => {
            panic!(
                "TLS network tests require `openssl` on PATH to generate test certificates.\n\
                 Install openssl or skip TLS tests with: cargo test -- --skip _net_tls"
            );
        }
    }

    dir
}
