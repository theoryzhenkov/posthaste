use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use base64::Engine;
use posthaste_domain_model::{GatewayError, ProviderAuthKind, TransportSecurity};
use posthaste_domain_service::SecretResolver;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use super::*;
use crate::mailbox::examine_selected_mailbox;

// --- a connect-counting, scriptable mock IMAP server ------------------------
//
// Serves the session-manager command set (greeting + CAPABILITY + AUTHENTICATE
// + EXAMINE/SELECT + NOOP + IDLE/DONE + LOGOUT) over real TCP so the real
// `imap-client` protocol stack is exercised. Observability the tests assert
// on: accepted-connection count, the secret presented at each AUTHENTICATE,
// and how many IDLE commands were issued.

const MOCK_CAPS: &str = "IMAP4rev1 IDLE UIDPLUS";

#[derive(Default)]
struct MockObservations {
    connections: AtomicUsize,
    auth_secrets: StdMutex<Vec<String>>,
    idles: AtomicUsize,
    /// Set to make every live and future connection die (server-side close).
    kill: AtomicBool,
}

struct MockImap {
    addr: SocketAddr,
    observed: Arc<MockObservations>,
    idle_seen: Arc<tokio::sync::Notify>,
}

impl MockImap {
    async fn spawn() -> Self {
        Self::spawn_with(false).await
    }

    async fn spawn_stalling_in_idle() -> Self {
        Self::spawn_with(true).await
    }

    async fn spawn_with(stall_in_idle: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock imap");
        let addr = listener.local_addr().expect("mock imap addr");
        let observed = Arc::new(MockObservations::default());
        let idle_seen = Arc::new(tokio::sync::Notify::new());

        let accept_observed = Arc::clone(&observed);
        let accept_idle_seen = Arc::clone(&idle_seen);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                accept_observed.connections.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve_connection(
                    stream,
                    Arc::clone(&accept_observed),
                    Arc::clone(&accept_idle_seen),
                    stall_in_idle,
                ));
            }
        });

        Self {
            addr,
            observed,
            idle_seen,
        }
    }

    fn config(&self) -> ImapConnectionConfig {
        ImapConnectionConfig {
            host: "127.0.0.1".to_string(),
            port: self.addr.port(),
            security: TransportSecurity::Plain,
            username: "user@example.test".to_string(),
            // Ignored: the manager resolves the real secret via the resolver.
            secret: "placeholder".to_string(),
            auth: ProviderAuthKind::Password,
        }
    }

    fn connections(&self) -> usize {
        self.observed.connections.load(Ordering::SeqCst)
    }

    fn idles(&self) -> usize {
        self.observed.idles.load(Ordering::SeqCst)
    }

    fn auth_secrets(&self) -> Vec<String> {
        self.observed
            .auth_secrets
            .lock()
            .expect("auth secrets")
            .clone()
    }

    /// Server-side kill switch: every live connection closes at its next
    /// command, emulating a Gmail idle-kill / network drop.
    fn kill_connections(&self) {
        self.observed.kill.store(true, Ordering::SeqCst);
    }

    fn revive(&self) {
        self.observed.kill.store(false, Ordering::SeqCst);
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    observed: Arc<MockObservations>,
    idle_seen: Arc<tokio::sync::Notify>,
    stall_in_idle: bool,
) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    if writer
        .write_all(format!("* OK [CAPABILITY {MOCK_CAPS}] mock ready\r\n").as_bytes())
        .await
        .is_err()
    {
        return;
    }

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if observed.kill.load(Ordering::SeqCst) {
            // Close abruptly: the client sees EOF on its next read.
            break;
        }
        let cmd = line.trim_end_matches(['\r', '\n']).to_string();
        let mut parts = cmd.split_whitespace();
        let tag = parts.next().unwrap_or("A1").to_string();
        let verb = parts.next().unwrap_or("").to_ascii_uppercase();

        let write_result = match verb.as_str() {
            "CAPABILITY" => {
                writer
                    .write_all(
                        format!("* CAPABILITY {MOCK_CAPS}\r\n{tag} OK CAPABILITY completed\r\n")
                            .as_bytes(),
                    )
                    .await
            }
            "AUTHENTICATE" => {
                let _mechanism = parts.next();
                let inline = parts.next().map(str::to_string);
                let payload = match inline {
                    Some(payload) => payload,
                    None => {
                        if writer.write_all(b"+ \r\n").await.is_err() {
                            break;
                        }
                        let mut response = String::new();
                        if reader.read_line(&mut response).await.unwrap_or(0) == 0 {
                            break;
                        }
                        response.trim_end_matches(['\r', '\n']).to_string()
                    }
                };
                if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(payload) {
                    // SASL PLAIN: \0user\0secret
                    if let Some(secret) = decoded.rsplit(|byte| *byte == 0).next() {
                        observed
                            .auth_secrets
                            .lock()
                            .expect("auth secrets")
                            .push(String::from_utf8_lossy(secret).into_owned());
                    }
                }
                writer
                    .write_all(format!("{tag} OK AUTHENTICATE completed\r\n").as_bytes())
                    .await
            }
            "SELECT" | "EXAMINE" => {
                let body = format!(
                    "* FLAGS (\\Seen)\r\n* 0 EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY 7]\r\n* OK [UIDNEXT 1]\r\n{tag} OK [READ-WRITE] {verb} completed\r\n"
                );
                writer.write_all(body.as_bytes()).await
            }
            "IDLE" => {
                observed.idles.fetch_add(1, Ordering::SeqCst);
                idle_seen.notify_waiters();
                if writer.write_all(b"+ idling\r\n").await.is_err() {
                    break;
                }
                if stall_in_idle {
                    // Half-open socket: accept IDLE, then never answer again
                    // (not even the DONE). The client's outer bound must fire.
                    std::future::pending::<()>().await;
                }
                // Wait for DONE (or EOF).
                let mut done_line = String::new();
                if reader.read_line(&mut done_line).await.unwrap_or(0) == 0 {
                    break;
                }
                if observed.kill.load(Ordering::SeqCst) {
                    break;
                }
                writer
                    .write_all(format!("{tag} OK IDLE terminated\r\n").as_bytes())
                    .await
            }
            "NOOP" | "ENABLE" | "ID" => {
                writer
                    .write_all(format!("{tag} OK {verb} completed\r\n").as_bytes())
                    .await
            }
            "LOGOUT" => {
                let _ = writer
                    .write_all(
                        format!("* BYE mock logging out\r\n{tag} OK LOGOUT completed\r\n")
                            .as_bytes(),
                    )
                    .await;
                break;
            }
            _ => {
                writer
                    .write_all(format!("{tag} BAD unknown command\r\n").as_bytes())
                    .await
            }
        };
        if write_result.is_err() {
            break;
        }
    }
}

// --- secret resolvers --------------------------------------------------------

#[derive(Debug)]
struct RotatingResolver {
    secret: StdMutex<String>,
    resolutions: AtomicUsize,
}

impl RotatingResolver {
    fn new(secret: &str) -> Arc<Self> {
        Arc::new(Self {
            secret: StdMutex::new(secret.to_string()),
            resolutions: AtomicUsize::new(0),
        })
    }

    fn rotate_to(&self, secret: &str) {
        *self.secret.lock().expect("secret") = secret.to_string();
    }
}

#[async_trait::async_trait]
impl SecretResolver for RotatingResolver {
    async fn resolve_secret(&self) -> Result<String, GatewayError> {
        self.resolutions.fetch_add(1, Ordering::SeqCst);
        Ok(self.secret.lock().expect("secret").clone())
    }
}

fn manager_for(server: &MockImap, resolver: Arc<RotatingResolver>) -> Arc<ImapSessionManager> {
    ImapSessionManager::new(server.config(), resolver)
}

/// Run a restore closure on scope exit (panic-safe).
fn scopeguard(restore: impl FnOnce()) -> impl Drop {
    struct Guard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for Guard<F> {
        fn drop(&mut self) {
            if let Some(restore) = self.0.take() {
                restore();
            }
        }
    }
    Guard(Some(restore))
}

async fn run_examine_op(manager: &ImapSessionManager) -> Result<(), ImapAdapterError> {
    let mut lease = manager.acquire("test_examine").await?;
    let result = examine_selected_mailbox(lease.client(), "INBOX")
        .await
        .map(|_selected| ());
    lease.finish(result)
}

// --- the D92/O3 regression suite ---------------------------------------------

// spec: docs/testing/L1#provider-observation-matrix
// The C4 fix: N operations ride ONE authenticated session (no per-operation
// connect), proven by the server's accepted-connection count.
#[tokio::test]
async fn multiple_operations_reuse_a_single_authenticated_session() {
    let server = MockImap::spawn().await;
    let resolver = RotatingResolver::new("secret-a");
    let manager = manager_for(&server, resolver);

    for _ in 0..5 {
        run_examine_op(&manager)
            .await
            .expect("operation on shared session");
    }

    assert_eq!(
        manager.connect_count(),
        1,
        "one connect for five operations"
    );
    assert_eq!(server.connections(), 1, "server saw a single connection");
    assert_eq!(server.auth_secrets().len(), 1, "authenticated exactly once");
}

// spec: docs/testing/L1#provider-observation-matrix
// Reconnect-on-drop: a server-side close (Gmail idle-kill, network blip)
// surfaces one failed operation, and the next use transparently reconnects.
#[tokio::test]
async fn a_dropped_session_reconnects_on_next_use() {
    let server = MockImap::spawn().await;
    let resolver = RotatingResolver::new("secret-a");
    let manager = manager_for(&server, resolver);

    run_examine_op(&manager).await.expect("first operation");
    assert_eq!(manager.connect_count(), 1);

    server.kill_connections();
    let error = run_examine_op(&manager)
        .await
        .expect_err("operation on a killed connection must fail");
    assert!(
        matches!(
            error,
            ImapAdapterError::Client(_) | ImapAdapterError::Timeout { .. }
        ),
        "expected a transport-level failure, got {error:?}"
    );

    server.revive();
    run_examine_op(&manager)
        .await
        .expect("next use reconnects transparently");
    assert_eq!(manager.connect_count(), 2, "exactly one reconnect");
    assert_eq!(server.connections(), 2);
}

// spec: docs/testing/L1#provider-observation-matrix
// OAuth rotation awareness: a token rotated mid-session does NOT tear down the
// live authenticated session (IMAP auth happens once per connection), and the
// next reconnect authenticates with the rotated token — never a stale one.
#[tokio::test]
async fn token_rotation_keeps_the_live_session_and_reauths_on_reconnect() {
    let server = MockImap::spawn().await;
    let resolver = RotatingResolver::new("token-old");
    let manager = manager_for(&server, Arc::clone(&resolver));

    run_examine_op(&manager)
        .await
        .expect("operation before rotation");
    resolver.rotate_to("token-new");
    run_examine_op(&manager)
        .await
        .expect("operation after rotation");

    assert_eq!(
        manager.connect_count(),
        1,
        "token rotation must not drop the live session"
    );
    assert_eq!(server.auth_secrets(), vec!["token-old".to_string()]);

    // The session eventually drops (server-side kill); the reconnect must use
    // the rotated token.
    server.kill_connections();
    let _ = run_examine_op(&manager).await;
    server.revive();
    run_examine_op(&manager)
        .await
        .expect("reconnect after rotation");

    assert_eq!(
        server.auth_secrets(),
        vec!["token-old".to_string(), "token-new".to_string()],
        "reconnect authenticates with the rotated token"
    );
}

// spec: docs/testing/L1#provider-observation-matrix
// C2: IDLE is re-issued (DONE + fresh IDLE) before the ~29-minute server
// timeout, on the same session, without surfacing spurious activity. The
// 24-minute re-issue interval is shrunk through its declared test seam
// (paused-clock virtual time races real-socket IO, so the seam is the
// deterministic route).
#[tokio::test]
async fn idle_reissues_before_the_server_timeout_on_one_session() {
    let _seam = crate::timeout::seam_test_lock().await;
    let restore = set_idle_reissue_ms_for_testing(200);
    let _restore = scopeguard(restore);
    let server = MockImap::spawn().await;
    let resolver = RotatingResolver::new("secret-a");
    let manager = manager_for(&server, resolver);

    for cycle in 0..2 {
        let outcome = manager.idle_wait("INBOX").await.expect("idle hold");
        assert_eq!(
            outcome,
            IdleWaitOutcome::ReissueTick,
            "quiet cycle {cycle} re-issues without an activity event"
        );
    }

    assert_eq!(server.idles(), 2, "IDLE was issued once per re-issue cycle");
    assert_eq!(manager.connect_count(), 1, "re-issue reuses the session");
}

// spec: docs/testing/L1#provider-observation-matrix
// C2's dead-socket half: a server that accepts IDLE and then never answers
// again (half-open socket) must trip the bounded hold, not hang forever.
#[tokio::test]
async fn a_half_open_idle_socket_times_out_instead_of_hanging() {
    let _seam = crate::timeout::seam_test_lock().await;
    let restore_reissue = set_idle_reissue_ms_for_testing(150);
    let _restore_reissue = scopeguard(restore_reissue);
    let restore_op = crate::timeout::set_op_timeout_ms_for_testing(150);
    let _restore_op = scopeguard(restore_op);
    let server = MockImap::spawn_stalling_in_idle().await;
    let resolver = RotatingResolver::new("secret-a");
    let manager = manager_for(&server, resolver);

    let error = manager
        .idle_wait("INBOX")
        .await
        .expect_err("a half-open idle socket must time out");
    assert!(
        matches!(error, ImapAdapterError::Timeout { .. }),
        "expected a typed timeout, got {error:?}"
    );

    // The poisoned session is discarded: the next hold reconnects (and, on
    // this always-stalling server, times out again — bounded, not hanging).
    let _ = manager.idle_wait("INBOX").await;
    assert_eq!(
        manager.connect_count(),
        2,
        "the dead session is replaced on the next hold"
    );
}

// spec: docs/testing/L1#provider-observation-matrix
// The D92c interaction: IDLE cannot hold the account's only session hostage —
// an operation recalls it (DONE + release) and proceeds on the same
// connection; no side connection is opened.
#[tokio::test]
async fn an_operation_recalls_an_in_flight_idle_hold() {
    let server = MockImap::spawn().await;
    let resolver = RotatingResolver::new("secret-a");
    let manager = manager_for(&server, resolver);

    let idle_seen = Arc::clone(&server.idle_seen);
    let idle_manager = Arc::clone(&manager);
    let notified = idle_seen.notified();
    let idle_task = tokio::spawn(async move { idle_manager.idle_wait("INBOX").await });
    notified.await;

    run_examine_op(&manager)
        .await
        .expect("operation proceeds after recalling IDLE");

    let outcome = idle_task
        .await
        .expect("idle task completes")
        .expect("recalled hold is not an error");
    assert_eq!(outcome, IdleWaitOutcome::Recalled);
    assert_eq!(
        manager.connect_count(),
        1,
        "IDLE and the operation shared one session"
    );
    assert_eq!(server.connections(), 1);
}
