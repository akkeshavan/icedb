use std::fs::File;
use std::io::BufReader;

use rustls_pemfile::{certs, pkcs8_private_keys};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

/// Build a [`TlsAcceptor`] from PEM-encoded certificate and private key files.
///
/// `cert_path` must point to a PEM file containing one or more X.509
/// certificates (the server certificate followed by any intermediates).
/// `key_path` must point to a PEM file containing a PKCS#8 private key.
pub fn build_tls_acceptor(
    cert_path: &str,
    key_path: &str,
) -> Result<TlsAcceptor, Box<dyn std::error::Error>> {
    let cert_file = File::open(cert_path)?;
    let cert: Vec<CertificateDer<'static>> = certs(&mut BufReader::new(cert_file))
        .collect::<Result<_, _>>()?;

    let key_file = File::open(key_path)?;
    let key = pkcs8_private_keys(&mut BufReader::new(key_file))
        .map(|k| k.map(PrivateKeyDer::from))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .next()
        .ok_or("no private key found in key file")?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert, key)?;
    config.alpn_protocols = vec![b"postgresql".to_vec()];

    Ok(TlsAcceptor::from(std::sync::Arc::new(config)))
}
