//! Optional in-daemon TLS (`[tls]` config): load a cert+key pair, build a
//! `rustls::ServerConfig` (ring crypto provider, no client auth), and expose a
//! [`TlsListener`] that wraps `tokio::net::TcpListener` so `axum::serve` speaks
//! HTTPS. Opt-in: when no `[tls]` config is present the daemon serves plaintext
//! (loopback) exactly as before.
//!
//! The ring crypto provider is pinned **explicitly** via
//! `builder_with_provider`, so the daemon's server TLS is independent of
//! whatever process-wide default provider reqwest/lettre installed for their
//! *client* TLS — no `install_default` global-state hazard.
//!
//! @spec docs/eph/DESIGN-L1-trust-model

use std::fs::File;
use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::serve::Listener;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use rustls_pemfile as pemfile;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio_rustls::{server::TlsStream, TlsAcceptor};
use tracing::{error, warn};

use posthaste_config::TlsConfig;

/// A TLS handshake must complete within this window or the connection is
/// dropped. Bounds the lifetime of a half-open connection so a client that
/// connects but never negotiates cannot tie up resources.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum number of TLS handshakes running concurrently in
/// [`TlsListener`]'s `JoinSet`. Previously unbounded: a connection flood grew
/// the set without limit, each entry a task with up to `HANDSHAKE_TIMEOUT`'s
/// worth of lifetime (N6 / RFC-L2-lifecycle D67(e) / M27 sub-unit (e)). Once
/// `handshakes.len()` reaches this cap, `accept` stops polling
/// `TcpListener::accept()` until a handshake finishes and is drained via
/// `join_next` — new connections queue in the OS accept backlog rather than
/// each spawning its own handshake task. **Review** (picked sane, not
/// measured).
const MAX_CONCURRENT_HANDSHAKES: usize = 64;

/// Build a [`TlsAcceptor`] from the `[tls]` cert+key paths. Reads PEM-encoded
/// certificate(s) and a PKCS#1/PKCS#8/SEC1 private key. The first suitable key
/// in the file is used; a missing key or empty cert chain is a hard error (fail
/// closed at startup, not silent plaintext).
pub fn build_tls_acceptor(tls: &TlsConfig) -> Result<TlsAcceptor, String> {
    let certs = load_certs(&tls.cert_path)
        .map_err(|e| format!("read TLS cert at {}: {e}", tls.cert_path.display()))?;
    if certs.is_empty() {
        return Err(format!(
            "no certificates found in {}",
            tls.cert_path.display()
        ));
    }
    let key = load_private_key(&tls.key_path)
        .map_err(|e| format!("read TLS key at {}: {e}", tls.key_path.display()))?
        .ok_or_else(|| format!("no private key found in {}", tls.key_path.display()))?;

    let provider = rustls::crypto::ring::default_provider();
    let config = ServerConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("invalid TLS protocol versions: {e}"))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("invalid TLS cert/key pair: {e}"))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

fn load_certs(path: &std::path::Path) -> io::Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(File::open(path)?);
    pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()
}

fn load_private_key(path: &std::path::Path) -> io::Result<Option<PrivateKeyDer<'static>>> {
    let mut reader = BufReader::new(File::open(path)?);
    pemfile::private_key(&mut reader)
}

/// A [`tokio::net::TcpListener`] that yields handshaked `TlsStream` connections
/// for `axum::serve` to speak HTTPS.
///
/// Handshakes run **off the accept path**: each accepted TCP connection is
/// handshaked on its own task (bounded by [`HANDSHAKE_TIMEOUT`]) and tracked in
/// a [`JoinSet`], while `accept` keeps taking new connections. A slow or stalled
/// handshake therefore cannot block other clients or wedge the listener — the
/// failure mode the naive "handshake inline in the accept loop" design has.
/// Per-connection errors (handshake failures, timeouts, resets) are logged and
/// the connection dropped; only fatal TCP-accept errors pause the loop briefly,
/// mirroring hyper/axum's `TcpListener`.
pub struct TlsListener {
    inner: TcpListener,
    acceptor: TlsAcceptor,
    handshakes: JoinSet<Option<(TlsStream<TcpStream>, SocketAddr)>>,
}

impl TlsListener {
    pub fn new(inner: TcpListener, acceptor: TlsAcceptor) -> Self {
        Self {
            inner,
            acceptor,
            handshakes: JoinSet::new(),
        }
    }
}

impl TlsListener {
    /// One iteration of the accept-or-drain `select!`. Split out of
    /// [`Listener::accept`] so a test can step through iterations and inspect
    /// `handshakes.len()` between them (N6 / M27 sub-unit (e)) instead of
    /// blocking on the whole, potentially long-running `accept` loop.
    /// Returns `Some` exactly when a handshake completed successfully;
    /// `None` means "loop again" (a connection was accepted-and-spawned, an
    /// accept error was logged, or a failed/timed-out handshake was drained).
    async fn accept_once(&mut self) -> Option<(TlsStream<TcpStream>, SocketAddr)> {
        tokio::select! {
            // Take a new TCP connection and start its handshake on a task —
            // but only while under the concurrency cap (N6): at the cap this
            // branch is not polled at all, so new connections queue in the OS
            // accept backlog instead of spawning an unbounded handshake task.
            accepted = self.inner.accept(), if self.handshakes.len() < MAX_CONCURRENT_HANDSHAKES => match accepted {
                Ok((stream, addr)) => {
                    let acceptor = self.acceptor.clone();
                    self.handshakes.spawn(async move {
                        match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream))
                            .await
                        {
                            Ok(Ok(tls)) => Some((tls, addr)),
                            Ok(Err(e)) => {
                                warn!(error = %e, %addr, "tls handshake failed; dropped");
                                None
                            }
                            Err(_) => {
                                warn!(%addr, "tls handshake timed out; dropped");
                                None
                            }
                        }
                    });
                    None
                }
                Err(e) => {
                    if !is_connection_error(&e) {
                        // Transient resource exhaustion (e.g. EMFILE): log + sleep.
                        error!("tcp accept error: {e}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    None
                }
            },
            // A handshake finished. Return it if it succeeded; otherwise the
            // connection was already logged + dropped, so the caller loops.
            // The guard keeps `select!` from busy-spinning on an empty set.
            Some(joined) = self.handshakes.join_next(), if !self.handshakes.is_empty() => {
                match joined {
                    Ok(Some(ready)) => Some(ready),
                    _ => None,
                }
            }
        }
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            if let Some(ready) = self.accept_once().await {
                return ready;
            }
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}

fn is_connection_error(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A self-signed cert+key pair generated once at dev time and checked into
    /// the test fixtures (see `tests/tls/`). We don't generate at runtime — keep
    /// TLS tests hermetic and offline.
    const FIXTURE_CERT: &str = "tests/tls/localhost.crt";
    const FIXTURE_KEY: &str = "tests/tls/localhost.key";

    fn fixture_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn build_acceptor_from_self_signed_pair() {
        let cert = fixture_dir().join(FIXTURE_CERT);
        let key = fixture_dir().join(FIXTURE_KEY);
        if !cert.exists() || !key.exists() {
            // Fixture not generated yet (dev task). Skip rather than fail —
            // generation is a separate step, not part of the unit test.
            eprintln!("skipping: TLS fixture missing at {}", cert.display());
            return;
        }
        let acceptor = build_tls_acceptor(&TlsConfig {
            cert_path: cert,
            key_path: key,
        });
        assert!(
            acceptor.is_ok(),
            "acceptor build failed: {:?}",
            acceptor.err()
        );
    }

    #[test]
    fn build_acceptor_rejects_missing_files() {
        let err = build_tls_acceptor(&TlsConfig {
            cert_path: std::path::PathBuf::from("/nonexistent/cert"),
            key_path: std::path::PathBuf::from("/nonexistent/key"),
        })
        .err()
        .expect("expected an error for missing files");
        assert!(err.contains("read TLS cert"), "unexpected error: {err}");
    }

    #[test]
    fn build_acceptor_rejects_empty_cert() {
        let dir = crate::test_support::temp_root("tls-empty-cert");
        let cert = dir.join("empty.crt");
        let key = dir.join("empty.key");
        let mut f = std::fs::File::create(&cert).unwrap();
        let _ = f.write_all(b"# no certs here\n");
        let mut f = std::fs::File::create(&key).unwrap();
        let _ = f.write_all(b"# no key here\n");
        let err = build_tls_acceptor(&TlsConfig {
            cert_path: cert,
            key_path: key,
        })
        .err()
        .expect("expected an error for empty cert");
        assert!(err.contains("no certificates"), "unexpected error: {err}");
    }

    // spec: docs/eph/RFC-L2-lifecycle-and-errors#d67 (N6 / M27 sub-unit (e))
    #[tokio::test]
    async fn handshake_joinset_stays_bounded_under_a_connection_flood() {
        let cert = fixture_dir().join(FIXTURE_CERT);
        let key = fixture_dir().join(FIXTURE_KEY);
        if !cert.exists() || !key.exists() {
            eprintln!("skipping: TLS fixture missing at {}", cert.display());
            return;
        }
        let acceptor = build_tls_acceptor(&TlsConfig {
            cert_path: cert,
            key_path: key,
        })
        .expect("acceptor should build");
        let inner = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = inner.local_addr().expect("local addr");
        let mut listener = TlsListener::new(inner, acceptor);

        // Flood: open more raw TCP connections than the handshake cap, none
        // of which ever sends a byte of TLS — each just connects and sits
        // idle, standing in for a connection flood that never negotiates
        // (N6's failure mode: an unbounded `JoinSet` of up-to-10s-lived
        // handshake tasks).
        let flood_count = MAX_CONCURRENT_HANDSHAKES + 32;
        let mut clients = Vec::with_capacity(flood_count);
        for _ in 0..flood_count {
            clients.push(
                TcpStream::connect(addr)
                    .await
                    .expect("flood client should connect"),
            );
        }

        // Drive enough accept-loop iterations to give every flooded
        // connection a chance to be accepted, sampling the JoinSet's size
        // between iterations. None of the flood clients ever completes a
        // handshake within this test, so each iteration is capped with a
        // short timeout rather than blocking indefinitely on `join_next`.
        let mut observed_peak = 0usize;
        for _ in 0..(flood_count * 2) {
            let _ = tokio::time::timeout(Duration::from_millis(20), listener.accept_once()).await;
            observed_peak = observed_peak.max(listener.handshakes.len());
        }

        assert!(
            observed_peak <= MAX_CONCURRENT_HANDSHAKES,
            "handshake JoinSet peaked at {observed_peak}, exceeding the cap of {MAX_CONCURRENT_HANDSHAKES}"
        );
        assert_eq!(
            observed_peak, MAX_CONCURRENT_HANDSHAKES,
            "the flood should have filled the JoinSet up to the cap, not stalled below it"
        );

        drop(clients);
    }
}
