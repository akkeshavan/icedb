//! Synchronous PostgreSQL wire-protocol client (Simple Query sub-protocol).
//!
//! Used by `isql` when `--host` is supplied so the CLI connects to a running
//! `icedb-server` over TCP (with optional TLS) rather than opening the
//! embedded engine directly.
//!
//! Implements:
//!   - SSLRequest negotiation (sslmode = disable | require | verify-full)
//!   - Startup message + SCRAM-SHA-256 and cleartext password authentication
//!   - Simple Query flow: `Q` → `T`/`D`*/`C`/`E` → `Z`

use std::io::{self, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use rustls_pki_types::ServerName;

use crate::config::{Config, SslMode};
use crate::error::CliError;

// ── Public result type ────────────────────────────────────────────────────────

/// Result of a Simple Query execution.
pub struct PgResult {
    /// Column names (empty for non-SELECT statements).
    pub columns: Vec<String>,
    /// PostgreSQL type OIDs for each column (23=int4, 20=int8, 701=float8, 16=bool, 25=text, …).
    pub col_type_oids: Vec<u32>,
    /// Row data; each cell is `None` for SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
    /// CommandComplete tag (e.g. `"INSERT 0 1"`, `"SELECT 5"`).
    pub command_tag: String,
    /// Parsed row count from the command tag.
    pub rows_affected: u64,
}

// ── Internal stream ───────────────────────────────────────────────────────────

enum PgStream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for PgStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            PgStream::Plain(s) => s.read(buf),
            PgStream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for PgStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            PgStream::Plain(s) => s.write(buf),
            PgStream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            PgStream::Plain(s) => s.flush(),
            PgStream::Tls(s) => s.flush(),
        }
    }
}

// ── Client ────────────────────────────────────────────────────────────────────

pub struct PgClient {
    stream: PgStream,
    pub current_db: String,
    // Stored for reconnection via \c
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
    sslmode: SslMode,
    sslrootcert: Option<String>,
}

impl PgClient {
    /// Connect to an icedb (or any PostgreSQL-compatible) server.
    pub fn connect(config: &Config) -> Result<Self, CliError> {
        // Install ring as the process-default rustls CryptoProvider.
        // The call is idempotent; errors (already installed) are ignored.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let addr = format!("{}:{}", config.host, config.port);
        let tcp = TcpStream::connect(&addr)
            .map_err(|e| CliError::Network(format!("cannot connect to {}: {}", addr, e)))?;
        tcp.set_nodelay(true).ok();

        let stream = match &config.sslmode {
            SslMode::Disable => PgStream::Plain(tcp),
            SslMode::Require => {
                let tls = negotiate_tls(tcp, &config.host, no_verify_config()?)?;
                PgStream::Tls(Box::new(tls))
            }
            SslMode::VerifyFull => {
                let path = config.sslrootcert.as_deref().ok_or_else(|| {
                    CliError::Network(
                        "sslmode=verify-full requires --sslrootcert <path>".to_string(),
                    )
                })?;
                let tls = negotiate_tls(tcp, &config.host, verify_config(path)?)?;
                PgStream::Tls(Box::new(tls))
            }
        };

        let mut client = PgClient {
            stream,
            current_db: config.dbname.clone(),
            host: config.host.clone(),
            port: config.port,
            user: config.username.clone(),
            password: config.password.clone(),
            sslmode: config.sslmode.clone(),
            sslrootcert: config.sslrootcert.clone(),
        };
        client.startup()?;
        Ok(client)
    }

    /// Open a new connection to the same server but a different database.
    /// Used by the `\c` meta-command.
    pub fn reconnect_db(&self, dbname: &str) -> Result<PgClient, CliError> {
        let config = Config {
            data_dir: String::new(),
            host: self.host.clone(),
            port: self.port,
            username: self.user.clone(),
            dbname: dbname.to_string(),
            password: self.password.clone(),
            history_file: None,
            sslmode: self.sslmode.clone(),
            sslrootcert: self.sslrootcert.clone(),
            explicit_host: true,
        };
        PgClient::connect(&config)
    }

    /// Execute a SQL statement and collect all results.
    pub fn query(&mut self, sql: &str) -> Result<PgResult, CliError> {
        self.send_query(sql)?;
        self.read_response()
    }

    // ── Startup ───────────────────────────────────────────────────────────────

    fn startup(&mut self) -> Result<(), CliError> {
        // Startup message: no type byte — just length + payload.
        // Protocol version 3.0 = 196608 = 0x00030000
        let mut payload = Vec::new();
        payload.extend_from_slice(&196608u32.to_be_bytes());
        payload.extend_from_slice(b"user\0");
        payload.extend_from_slice(self.user.as_bytes());
        payload.push(0);
        payload.extend_from_slice(b"database\0");
        payload.extend_from_slice(self.current_db.as_bytes());
        payload.push(0);
        payload.push(0); // terminator

        let total_len = (4 + payload.len()) as u32;
        self.stream
            .write_all(&total_len.to_be_bytes())
            .map_err(io_err("write startup length"))?;
        self.stream
            .write_all(&payload)
            .map_err(io_err("write startup payload"))?;
        self.stream.flush().map_err(io_err("flush startup"))?;

        // Read messages until ReadyForQuery
        loop {
            let (msg_type, body) = self.read_message()?;
            match msg_type {
                b'R' => self.handle_auth(&body)?,
                b'S' | b'K' => {} // ParameterStatus / BackendKeyData — ignore
                b'Z' => return Ok(()), // ReadyForQuery — startup done
                b'E' => {
                    return Err(CliError::Network(format!(
                        "server rejected connection: {}",
                        parse_error_response(&body)
                    )))
                }
                other => {
                    return Err(CliError::Network(format!(
                        "unexpected message type '{}' during startup",
                        char::from(other)
                    )))
                }
            }
        }
    }

    // ── Authentication ────────────────────────────────────────────────────────

    fn handle_auth(&mut self, body: &[u8]) -> Result<(), CliError> {
        if body.len() < 4 {
            return Err(CliError::Network("truncated authentication message".to_string()));
        }
        let auth_type = u32::from_be_bytes(body[..4].try_into().unwrap());
        match auth_type {
            0 => Ok(()),  // AuthenticationOk — no credentials required (trust)
            3 => {
                // AuthenticationCleartextPassword
                let pwd = self.password.clone().unwrap_or_default();
                self.send_cleartext_password(&pwd)
            }
            5 => Err(CliError::Network(
                "server requested MD5 password auth; icedb uses SCRAM-SHA-256".to_string(),
            )),
            10 => {
                // AuthenticationSASL
                self.handle_scram_auth(&body[4..])
            }
            other => Err(CliError::Network(format!(
                "unsupported authentication method type {}",
                other
            ))),
        }
    }

    fn send_cleartext_password(&mut self, pwd: &str) -> Result<(), CliError> {
        let pwd_bytes = pwd.as_bytes();
        // 'p' + int32(4 + len + 1) + password + \0
        let len = (4 + pwd_bytes.len() + 1) as u32;
        let mut buf = Vec::with_capacity(5 + pwd_bytes.len());
        buf.push(b'p');
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(pwd_bytes);
        buf.push(0);
        self.stream.write_all(&buf).map_err(io_err("send password"))?;
        self.stream.flush().map_err(io_err("flush password"))?;
        // Expect AuthenticationOk
        let (msg_type, body) = self.read_message()?;
        match msg_type {
            b'R' if body.len() >= 4 && u32::from_be_bytes(body[..4].try_into().unwrap()) == 0 => {
                Ok(())
            }
            b'E' => Err(CliError::Network(format!(
                "authentication failed: {}",
                parse_error_response(&body)
            ))),
            _ => Err(CliError::Network("unexpected response to password".to_string())),
        }
    }

    fn handle_scram_auth(&mut self, mechanisms_body: &[u8]) -> Result<(), CliError> {
        // Verify the server offers SCRAM-SHA-256
        let mechanisms = read_cstrings(mechanisms_body);
        if !mechanisms.iter().any(|m| m == "SCRAM-SHA-256") {
            return Err(CliError::Network(format!(
                "server does not offer SCRAM-SHA-256; offered: {:?}",
                mechanisms
            )));
        }

        let pwd = self.password.clone().unwrap_or_default();
        let user = self.user.clone();

        // ── Step 1: ClientFirst ──────────────────────────────────────────────
        let client_nonce = auth::scram::generate_nonce();
        let client_first_bare = format!("n={},r={}", user, client_nonce);
        let client_first = format!("n,,{}", client_first_bare); // GS2 header: n,,

        self.send_sasl_initial_response(client_first.as_bytes())?;

        // ── Step 2: ServerFirst (AuthenticationSASLContinue) ─────────────────
        let (msg_type, body) = self.read_message()?;
        if msg_type != b'R' || body.len() < 4 {
            return Err(CliError::Network("expected AuthenticationSASLContinue".to_string()));
        }
        if u32::from_be_bytes(body[..4].try_into().unwrap()) != 11 {
            return Err(CliError::Network("expected AuthenticationSASLContinue (type 11)".to_string()));
        }
        let server_first = std::str::from_utf8(&body[4..])
            .map_err(|_| CliError::Network("invalid server-first-message encoding".to_string()))?
            .to_string();

        let (combined_nonce, salt_bytes, iterations) = parse_server_first(&server_first)?;

        if !combined_nonce.starts_with(&client_nonce) {
            return Err(CliError::Network(
                "SCRAM: server nonce does not begin with client nonce".to_string(),
            ));
        }

        // ── Step 3: ClientFinal ──────────────────────────────────────────────
        // channel-binding-value = base64("n,,") = "biws"
        let client_final_no_proof = format!("c=biws,r={}", combined_nonce);
        let auth_message = format!(
            "{},{},{}",
            client_first_bare, server_first, client_final_no_proof
        );

        let salted_password =
            auth::scram::pbkdf2_sha256(pwd.as_bytes(), &salt_bytes, iterations);

        let client_key = auth::scram::hmac_sha256(&salted_password, b"Client Key");
        let stored_key = auth::scram::sha256(&client_key);
        let server_key = auth::scram::hmac_sha256(&salted_password, b"Server Key");

        let client_sig = auth::scram::hmac_sha256(&stored_key, auth_message.as_bytes());
        let client_proof: Vec<u8> = client_key
            .iter()
            .zip(client_sig.iter())
            .map(|(k, s)| k ^ s)
            .collect();
        let server_sig = auth::scram::hmac_sha256(&server_key, auth_message.as_bytes());

        let client_final = format!(
            "{},p={}",
            client_final_no_proof,
            BASE64.encode(&client_proof)
        );

        self.send_sasl_response(client_final.as_bytes())?;

        // ── Step 4: ServerFinal (AuthenticationSASLFinal) ────────────────────
        let (msg_type, body) = self.read_message()?;
        if msg_type != b'R' || body.len() < 4 {
            return Err(CliError::Network("expected AuthenticationSASLFinal".to_string()));
        }
        let final_type = u32::from_be_bytes(body[..4].try_into().unwrap());
        if final_type == 12 {
            // Verify server signature
            if let Ok(server_final) = std::str::from_utf8(&body[4..]) {
                if let Some(v) = server_final.strip_prefix("v=") {
                    let expected = BASE64.encode(&server_sig);
                    if v != expected {
                        return Err(CliError::Network(
                            "SCRAM: server signature verification failed".to_string(),
                        ));
                    }
                }
            }
            // Expect AuthenticationOk to follow
            let (t, b) = self.read_message()?;
            if t == b'R'
                && b.len() >= 4
                && u32::from_be_bytes(b[..4].try_into().unwrap()) == 0
            {
                Ok(())
            } else if t == b'E' {
                Err(CliError::Network(format!(
                    "authentication failed: {}",
                    parse_error_response(&b)
                )))
            } else {
                Err(CliError::Network("expected AuthenticationOk after SCRAM".to_string()))
            }
        } else if final_type == 0 {
            Ok(()) // Some servers send AuthenticationOk directly
        } else {
            Err(CliError::Network(format!(
                "unexpected SASL final auth type {}",
                final_type
            )))
        }
    }

    fn send_sasl_initial_response(&mut self, data: &[u8]) -> Result<(), CliError> {
        // 'p' + int32(len) + "SCRAM-SHA-256\0" + int32(data_len) + data
        let mech = b"SCRAM-SHA-256\0";
        let body_len = 4 + mech.len() + 4 + data.len();
        let mut buf = Vec::with_capacity(1 + 4 + body_len);
        buf.push(b'p');
        buf.extend_from_slice(&(4 + body_len as u32).to_be_bytes());
        buf.extend_from_slice(mech);
        buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
        buf.extend_from_slice(data);
        self.stream.write_all(&buf).map_err(io_err("send SASL initial response"))?;
        self.stream.flush().map_err(io_err("flush SASL initial response"))
    }

    fn send_sasl_response(&mut self, data: &[u8]) -> Result<(), CliError> {
        // 'p' + int32(4 + data_len) + data
        let mut buf = Vec::with_capacity(1 + 4 + data.len());
        buf.push(b'p');
        buf.extend_from_slice(&(4 + data.len() as u32).to_be_bytes());
        buf.extend_from_slice(data);
        self.stream.write_all(&buf).map_err(io_err("send SASL response"))?;
        self.stream.flush().map_err(io_err("flush SASL response"))
    }

    // ── Simple Query ──────────────────────────────────────────────────────────

    fn send_query(&mut self, sql: &str) -> Result<(), CliError> {
        // 'Q' + int32(4 + sql_bytes + 1) + sql + \0
        let sql_bytes = sql.as_bytes();
        let len = (4 + sql_bytes.len() + 1) as u32;
        let mut buf = Vec::with_capacity(5 + sql_bytes.len());
        buf.push(b'Q');
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(sql_bytes);
        buf.push(0);
        self.stream.write_all(&buf).map_err(io_err("send query"))?;
        self.stream.flush().map_err(io_err("flush query"))
    }

    fn read_response(&mut self) -> Result<PgResult, CliError> {
        let mut columns: Vec<String> = Vec::new();
        let mut col_type_oids: Vec<u32> = Vec::new();
        let mut col_count = 0usize;
        let mut rows: Vec<Vec<Option<String>>> = Vec::new();
        let mut command_tag = String::new();
        let mut pending_error: Option<String> = None;

        loop {
            let (msg_type, body) = self.read_message()?;
            match msg_type {
                b'T' => {
                    let (cols, oids) = parse_row_description(&body);
                    columns = cols;
                    col_type_oids = oids;
                    col_count = columns.len();
                }
                b'D' => {
                    if let Some(row) = parse_data_row(&body, col_count) {
                        rows.push(row);
                    }
                }
                b'C' => {
                    command_tag = parse_command_complete(&body);
                }
                b'Z' => {
                    // ReadyForQuery — response stream is done
                    if let Some(err) = pending_error {
                        return Err(CliError::Network(err));
                    }
                    let rows_affected = parse_rows_affected(&command_tag);
                    return Ok(PgResult {
                        columns,
                        col_type_oids,
                        rows,
                        command_tag,
                        rows_affected,
                    });
                }
                b'E' => {
                    pending_error = Some(parse_error_response(&body));
                }
                b'N' | b'S' | b'I' => {} // NoticeResponse / ParameterStatus / EmptyQueryResponse
                _ => {}
            }
        }
    }

    // ── Low-level I/O ─────────────────────────────────────────────────────────

    fn read_message(&mut self) -> Result<(u8, Vec<u8>), CliError> {
        let mut header = [0u8; 5];
        self.stream
            .read_exact(&mut header)
            .map_err(io_err("read message header"))?;
        let msg_type = header[0];
        let length = u32::from_be_bytes(header[1..5].try_into().unwrap()) as usize;
        if length < 4 {
            return Err(CliError::Network(format!(
                "invalid message length {} for type '{}'",
                length,
                char::from(msg_type)
            )));
        }
        let body_len = length - 4;
        let mut body = vec![0u8; body_len];
        if body_len > 0 {
            self.stream
                .read_exact(&mut body)
                .map_err(io_err("read message body"))?;
        }
        Ok((msg_type, body))
    }
}

// ── TLS helpers ───────────────────────────────────────────────────────────────

fn negotiate_tls(
    mut tcp: TcpStream,
    hostname: &str,
    config: ClientConfig,
) -> Result<StreamOwned<ClientConnection, TcpStream>, CliError> {
    // SSLRequest: not a standard framed message — just 8 raw bytes.
    // Length = 8 (big-endian i32), magic = 80877103 (big-endian i32).
    let ssl_request = [0x00u8, 0x00, 0x00, 0x08, 0x04, 0xD2, 0x16, 0x2F];
    tcp.write_all(&ssl_request)
        .map_err(io_err("send SSLRequest"))?;
    tcp.flush().ok();

    let mut resp = [0u8; 1];
    tcp.read_exact(&mut resp)
        .map_err(io_err("read SSLRequest response"))?;

    match resp[0] {
        b'S' => {
            let server_name = ServerName::try_from(hostname.to_string()).map_err(|_| {
                CliError::Network(format!("'{}' is not a valid TLS hostname", hostname))
            })?;
            let conn = ClientConnection::new(Arc::new(config), server_name)
                .map_err(|e| CliError::Network(format!("TLS handshake failed: {}", e)))?;
            Ok(StreamOwned::new(conn, tcp))
        }
        b'N' => Err(CliError::Network(
            "server rejected TLS (responded 'N' to SSLRequest); \
             start the server with --tls-cert and --tls-key to enable TLS"
                .to_string(),
        )),
        other => Err(CliError::Network(format!(
            "unexpected byte 0x{:02x} in response to SSLRequest",
            other
        ))),
    }
}

/// Build a `ClientConfig` that encrypts but does **not** verify the server
/// certificate — safe for self-signed certs in development.
fn no_verify_config() -> Result<ClientConfig, CliError> {
    use rustls::client::danger::{
        HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
    };
    use rustls::{DigitallySignedStruct, Error, SignatureScheme};
    use rustls_pki_types::{CertificateDer, ServerName as PkiServerName, UnixTime};

    #[derive(Debug)]
    struct NoVerifier;

    impl ServerCertVerifier for NoVerifier {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &PkiServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::ED25519,
            ]
        }
    }

    Ok(ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth())
}

/// Build a `ClientConfig` that verifies the server certificate against a PEM
/// CA file at `root_cert_path`.  Used for `sslmode=verify-full`.
fn verify_config(root_cert_path: &str) -> Result<ClientConfig, CliError> {
    use std::fs::File;
    use rustls_pemfile::certs;

    let file = File::open(root_cert_path).map_err(|e| {
        CliError::Network(format!("cannot open sslrootcert '{}': {}", root_cert_path, e))
    })?;
    let certs: Vec<rustls_pki_types::CertificateDer<'static>> =
        certs(&mut BufReader::new(file))
            .collect::<Result<_, _>>()
            .map_err(|e| CliError::Network(format!("cannot parse sslrootcert: {}", e)))?;

    let mut root_store = RootCertStore::empty();
    for cert in certs {
        root_store
            .add(cert)
            .map_err(|e| CliError::Network(format!("invalid CA certificate: {}", e)))?;
    }

    Ok(ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth())
}

// ── Wire protocol parsers ─────────────────────────────────────────────────────

/// Read consecutive null-terminated C strings from `data` until an empty
/// string (just a null byte) or end of data.
fn read_cstrings(mut data: &[u8]) -> Vec<String> {
    let mut result = Vec::new();
    loop {
        if data.is_empty() || data[0] == 0 {
            break;
        }
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        result.push(String::from_utf8_lossy(&data[..end]).into_owned());
        data = &data[end.saturating_add(1)..];
    }
    result
}

/// Parse `r=<nonce>,s=<base64-salt>,i=<iterations>` from a SCRAM server-first
/// message.  Returns `(combined_nonce, salt_bytes, iterations)`.
fn parse_server_first(msg: &str) -> Result<(String, Vec<u8>, u32), CliError> {
    let mut nonce = None;
    let mut salt = None;
    let mut iters = None;

    for part in msg.split(',') {
        if let Some(r) = part.strip_prefix("r=") {
            nonce = Some(r.to_string());
        } else if let Some(s) = part.strip_prefix("s=") {
            salt = Some(
                BASE64
                    .decode(s)
                    .map_err(|_| CliError::Network("SCRAM: invalid salt encoding".to_string()))?,
            );
        } else if let Some(i) = part.strip_prefix("i=") {
            iters = Some(
                i.parse::<u32>()
                    .map_err(|_| CliError::Network("SCRAM: invalid iteration count".to_string()))?,
            );
        }
    }

    Ok((
        nonce.ok_or_else(|| CliError::Network("SCRAM server-first missing r=".to_string()))?,
        salt.ok_or_else(|| CliError::Network("SCRAM server-first missing s=".to_string()))?,
        iters.ok_or_else(|| CliError::Network("SCRAM server-first missing i=".to_string()))?,
    ))
}

/// Parse a RowDescription (`T`) message body into column names.
///
/// RowDescription format:
///   Int16   number of fields
///   For each field:
///     String   name (null-terminated)
///     Int32    table OID
///     Int16    column attribute number
///     Int32    data type OID
///     Int16    data type size
///     Int32    type modifier
///     Int16    format code
fn parse_row_description(body: &[u8]) -> (Vec<String>, Vec<u32>) {
    if body.len() < 2 {
        return (Vec::new(), Vec::new());
    }
    let n = u16::from_be_bytes(body[..2].try_into().unwrap()) as usize;
    let mut cols = Vec::with_capacity(n);
    let mut oids = Vec::with_capacity(n);
    let mut pos = 2;

    for _ in 0..n {
        if pos >= body.len() {
            break;
        }
        let name_end = body[pos..]
            .iter()
            .position(|&b| b == 0)
            .map(|i| pos + i)
            .unwrap_or(body.len());
        cols.push(String::from_utf8_lossy(&body[pos..name_end]).into_owned());
        // After name \0: table_oid(4) + col_attr(2) + type_oid(4) + type_size(2) + type_mod(4) + format(2) = 18
        let fixed_start = name_end + 1;
        let type_oid = if fixed_start + 6 <= body.len() {
            u32::from_be_bytes(body[fixed_start + 6..fixed_start + 10].try_into().unwrap_or([0; 4]))
        } else {
            0
        };
        oids.push(type_oid);
        pos = fixed_start + 18;
    }
    (cols, oids)
}

/// Parse a DataRow (`D`) message body.
///
/// DataRow format:
///   Int16   number of column values
///   For each column:
///     Int32  column value length (-1 = NULL)
///     Byte*  column data (text format)
fn parse_data_row(body: &[u8], col_count: usize) -> Option<Vec<Option<String>>> {
    if body.len() < 2 {
        return None;
    }
    let n = u16::from_be_bytes(body[..2].try_into().unwrap()) as usize;
    let mut values = Vec::with_capacity(n);
    let mut pos = 2;

    for _ in 0..n.min(col_count) {
        if pos + 4 > body.len() {
            values.push(None);
            continue;
        }
        let col_len = i32::from_be_bytes(body[pos..pos + 4].try_into().unwrap());
        pos += 4;
        if col_len < 0 {
            values.push(None); // SQL NULL
        } else {
            let len = col_len as usize;
            if pos + len > body.len() {
                values.push(None);
                break;
            }
            values.push(Some(
                String::from_utf8_lossy(&body[pos..pos + len]).into_owned(),
            ));
            pos += len;
        }
    }
    Some(values)
}

/// Extract the null-terminated command tag from a CommandComplete (`C`) body.
fn parse_command_complete(body: &[u8]) -> String {
    let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    String::from_utf8_lossy(&body[..end]).into_owned()
}

/// Extract the affected row count from a CommandComplete tag.
/// Tags look like: `INSERT 0 1`, `UPDATE 5`, `DELETE 3`, `SELECT 10`.
fn parse_rows_affected(tag: &str) -> u64 {
    tag.split_whitespace()
        .last()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Parse an ErrorResponse (`E`) body and return a string that embeds the
/// SQLSTATE code for downstream classification.
///
/// ErrorResponse format: repeated (field-type byte + null-terminated string),
/// terminated by a zero byte.  We extract severity ('S'), SQLSTATE ('C'),
/// and message ('M').
///
/// Returns `"SQLSTATE[CODE]: SEVERITY: message"` when a SQLSTATE is present,
/// or just the message/severity otherwise.
fn parse_error_response(body: &[u8]) -> String {
    let mut severity = String::new();
    let mut message = String::new();
    let mut sqlstate = String::new();
    let mut pos = 0;

    while pos < body.len() {
        let field_type = body[pos];
        pos += 1;
        if field_type == 0 {
            break;
        }
        let end = body[pos..]
            .iter()
            .position(|&b| b == 0)
            .map(|i| pos + i)
            .unwrap_or(body.len());
        let value = String::from_utf8_lossy(&body[pos..end]).into_owned();
        pos = end + 1;
        match field_type {
            b'S' => severity = value,
            b'C' => sqlstate = value,
            b'M' => message = value,
            _ => {}
        }
    }

    let body_str = if severity.is_empty() {
        message
    } else {
        format!("{}: {}", severity, message)
    };

    if sqlstate.is_empty() {
        body_str
    } else {
        format!("SQLSTATE[{}]: {}", sqlstate, body_str)
    }
}

// ── Utility ───────────────────────────────────────────────────────────────────

fn io_err(context: &'static str) -> impl Fn(io::Error) -> CliError {
    move |e| CliError::Network(format!("{}: {}", context, e))
}
