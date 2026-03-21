use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use auth::Authenticator;
use sql::engine::QueryEngine;

use crate::handler::{IceDbHandlerFactory, IceDbStartupHandler};

/// Default maximum number of concurrent client connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 100;

pub struct Server {
    pub engine: Arc<QueryEngine>,
    pub authenticator: Arc<Authenticator>,
    pub bind_addr: SocketAddr,
    pub max_connections: usize,
    pub tls_acceptor: Option<Arc<TlsAcceptor>>,
}

impl Server {
    pub fn new(
        engine: Arc<QueryEngine>,
        authenticator: Arc<Authenticator>,
        bind_addr: SocketAddr,
    ) -> Self {
        Self {
            engine,
            authenticator,
            bind_addr,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            tls_acceptor: None,
        }
    }

    pub fn with_max_connections(mut self, n: usize) -> Self {
        self.max_connections = n;
        self
    }

    /// Enable TLS on this server using the supplied [`TlsAcceptor`].
    pub fn with_tls(mut self, acceptor: TlsAcceptor) -> Self {
        self.tls_acceptor = Some(Arc::new(acceptor));
        self
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(self.bind_addr).await?;
        log::info!("icedb listening on {}", self.bind_addr);
        log::info!("max_connections = {}", self.max_connections);

        let startup_handler = Arc::new(IceDbStartupHandler::new(Arc::clone(&self.authenticator)));
        let factory = Arc::new(IceDbHandlerFactory {
            engine: self.engine,
            authenticator: self.authenticator,
            startup_handler,
        });

        let active = Arc::new(AtomicUsize::new(0));
        let max_conn = self.max_connections;
        let tls_acceptor = self.tls_acceptor;

        // Shutdown future: fires on SIGTERM or Ctrl-C
        let shutdown = async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = sigterm.recv() => {},
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            log::info!("Shutdown signal received — stopping accept loop");
        };

        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                // Bias toward the shutdown branch so a signal is never starved.
                biased;
                _ = &mut shutdown => break,

                accept = listener.accept() => {
                    let (socket, peer_addr) = match accept {
                        Ok(v) => v,
                        Err(e) => {
                            log::error!("accept error: {}", e);
                            continue;
                        }
                    };

                    let current = active.fetch_add(1, Ordering::Relaxed);
                    if current >= max_conn {
                        active.fetch_sub(1, Ordering::Relaxed);
                        log::warn!(
                            "connection limit ({}) reached — rejecting {}",
                            max_conn, peer_addr
                        );
                        // Drop socket; client will see a TCP RST.
                        drop(socket);
                        continue;
                    }

                    log::debug!("accepted connection from {} ({}/{})", peer_addr, current + 1, max_conn);
                    let factory_ref = factory.clone();
                    let active_ref = active.clone();
                    let tls_ref = tls_acceptor.clone();
                    tokio::spawn(async move {
                        if let Err(e) = pgwire::tokio::process_socket(socket, tls_ref, factory_ref).await {
                            log::debug!("connection closed from {}: {}", peer_addr, e);
                        }
                        active_ref.fetch_sub(1, Ordering::Relaxed);
                    });
                }
            }
        }

        // Drain: wait up to 30 s for in-flight connections to finish.
        let drain_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while active.load(Ordering::Relaxed) > 0 {
            if std::time::Instant::now() >= drain_deadline {
                log::warn!(
                    "drain timeout: {} connection(s) still active — forcing exit",
                    active.load(Ordering::Relaxed)
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        log::info!("icedb server shutdown complete");
        Ok(())
    }
}
