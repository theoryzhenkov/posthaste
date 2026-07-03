//! The process teardown sequence (RFC D60 / migration M20).
//!
//! [`ShutdownSequence`] is the one named component the composition root owns to
//! shut the process down in order, each phase bounded by a deadline so an overrun
//! logs and proceeds rather than hanging the process. It is driven by a single
//! signal handler ([`wait_for_shutdown_signal`]: `ctrl_c` + unix `SIGTERM`) that
//! fires the shared [`CancellationToken`]; that same token is what
//! [`crate::serve`] hands to axum's `.with_graceful_shutdown`, so the first phase
//! (stop accepting + drain in-flight) is axum's own graceful drain.
//!
//! The ordered contract (D60) and the ratified per-phase budget (RFC §7 ruling 1
//! — 8s total inside the ~10s SIGTERM window):
//!   (a) HTTP graceful drain           — [`HTTP_DRAIN_DEADLINE`]      (~3s)
//!   (b) runtime + supervisor stop      — [`SUPERVISOR_STOP_DEADLINE`] (~3s)
//!   (c) store close + WAL checkpoint   — [`STORE_CLOSE_DEADLINE`]     (~2s)
//!
//! Each long-lived component exposes exactly one stop surface; the sequence
//! sequences them (tenet XVI — no component reaches across the boundary). The
//! supervisor and store are reached through the [`SupervisorStop`] / [`StoreClose`]
//! seams so this near crate does not depend on the far-node/store crates — the
//! composition root wires the concrete implementations.
//!
//! @spec docs/eph/RFC-L2-lifecycle-and-errors#d60

use std::time::Duration;

use async_trait::async_trait;
use posthaste_runtime::RuntimeShutdownHandle;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use tracing_appender::non_blocking::WorkerGuard;

/// Phase (a): stop accepting + drain in-flight HTTP/SSE. Ratified at ~3s.
pub const HTTP_DRAIN_DEADLINE: Duration = Duration::from_secs(3);
/// Phase (b): stop the account supervisor + the runtime's owned tasks. ~3s.
pub const SUPERVISOR_STOP_DEADLINE: Duration = Duration::from_secs(3);
/// Phase (c): checkpoint + close the store. ~2s; the checkpoint may NOT overrun
/// (a missed checkpoint costs a WAL replay, not data — RFC §7 ruling 1).
pub const STORE_CLOSE_DEADLINE: Duration = Duration::from_secs(2);
/// The whole teardown budget inside the ~10s SIGTERM→SIGKILL window. The sum of
/// the three per-phase deadlines; asserted in tests so the split cannot drift
/// out of the window (tenet XXIV).
pub const TOTAL_SHUTDOWN_BUDGET: Duration = Duration::from_secs(8);

/// Teardown step (b), supervisor half: cooperatively stop every account runtime.
///
/// The composition root wires the concrete `AccountSupervisor` behind this seam.
/// M20 calls the placeholder abort loop that exists today; M21 replaces the
/// supervisor internals (cooperative cancel + join + panic-surfacing watchdog)
/// without touching this seam.
#[async_trait]
pub trait SupervisorStop: Send + Sync {
    /// Stop all account runtimes. Bounded by the sequence, so implementations
    /// need not enforce their own deadline.
    async fn stop_all(&self);
}

/// Teardown step (c): close the store, checkpointing the WAL.
///
/// The composition root wires the concrete `DatabaseStore` behind this seam. The
/// WAL checkpoint itself lands in M22 (inside `DatabaseStore::close`).
#[async_trait]
pub trait StoreClose: Send + Sync {
    /// Close the store cleanly.
    async fn close(&self);
}

/// The ordered, deadline-bounded process teardown (D60). Built by the composition
/// root from the pieces it owns; run once, at shutdown.
pub struct ShutdownSequence {
    token: CancellationToken,
    server_join: JoinHandle<()>,
    runtime_shutdown: Option<RuntimeShutdownHandle>,
    supervisor_stop: Option<Box<dyn SupervisorStop>>,
    store_close: Option<Box<dyn StoreClose>>,
    /// Held so log output survives until the very end of teardown.
    log_guard: Option<WorkerGuard>,
}

impl ShutdownSequence {
    /// Start from the two pieces every role has: the shared cancellation token
    /// (already wired into axum's graceful shutdown) and the serve task's join
    /// handle (whose completion is the HTTP drain finishing).
    pub fn new(token: CancellationToken, server_join: JoinHandle<()>) -> Self {
        Self {
            token,
            server_join,
            runtime_shutdown: None,
            supervisor_stop: None,
            store_close: None,
            log_guard: None,
        }
    }

    /// Wire the runtime shutdown handle (teardown step (b): stops the runtime's
    /// owned tasks, incl. the N7 down-channel bridge). Absent for a role with no
    /// runtime near node (the standalone authority server).
    #[must_use]
    pub fn with_runtime_shutdown(mut self, handle: RuntimeShutdownHandle) -> Self {
        self.runtime_shutdown = Some(handle);
        self
    }

    /// Wire the supervisor stop seam (teardown step (b)). Absent for a lean near
    /// node (no in-process supervisor).
    #[must_use]
    pub fn with_supervisor_stop(mut self, stop: Box<dyn SupervisorStop>) -> Self {
        self.supervisor_stop = Some(stop);
        self
    }

    /// Wire the store close seam (teardown step (c)). Absent for a lean near node
    /// (no local store).
    #[must_use]
    pub fn with_store_close(mut self, close: Box<dyn StoreClose>) -> Self {
        self.store_close = Some(close);
        self
    }

    /// Keep the logging guard alive for the duration of teardown.
    #[must_use]
    pub fn with_log_guard(mut self, guard: WorkerGuard) -> Self {
        self.log_guard = Some(guard);
        self
    }

    /// Block until a shutdown signal arrives, then run the ordered teardown. This
    /// is what the role binaries `.await` in place of the old
    /// `join_handle.await`.
    pub async fn run_until_signal(self) {
        wait_for_shutdown_signal().await;
        info!("shutdown signal received; beginning ordered teardown");
        self.run().await;
    }

    /// Run the ordered, deadline-bounded teardown now (signal already observed, or
    /// invoked directly by a host/test). Cancels the token to start the HTTP
    /// drain, then walks the phases. Never hangs: every phase overrun logs and
    /// proceeds.
    pub async fn run(self) {
        let Self {
            token,
            server_join,
            runtime_shutdown,
            supervisor_stop,
            store_close,
            log_guard,
        } = self;

        // Phase (a): stop accepting + drain in-flight. Cancelling the token trips
        // axum's `.with_graceful_shutdown`, which stops accepting and lets
        // in-flight requests/SSE finish; the serve task then returns and its join
        // handle resolves. Bound the wait so a stuck stream cannot hang teardown.
        token.cancel();
        match timeout(HTTP_DRAIN_DEADLINE, server_join).await {
            Ok(Ok(())) => info!("http drain complete"),
            Ok(Err(join_error)) => {
                warn!(error = %join_error, "server task ended abnormally during drain");
            }
            Err(_) => warn!(
                deadline_ms = HTTP_DRAIN_DEADLINE.as_millis() as u64,
                "http drain exceeded its deadline; proceeding with teardown"
            ),
        }

        // Phase (b): stop the account supervisor, then the runtime's owned tasks,
        // together bounded by the supervisor-join budget.
        let stop_runtime = async {
            if let Some(stop) = supervisor_stop {
                // M21: this calls today's placeholder abort loop; M21 swaps in
                // cooperative cancel + join + watchdog behind this same seam.
                stop.stop_all().await;
            }
            if let Some(runtime_shutdown) = runtime_shutdown {
                if let Err(error) = runtime_shutdown.shutdown().await {
                    warn!(%error, "runtime shutdown reported an error");
                }
            }
        };
        match timeout(SUPERVISOR_STOP_DEADLINE, stop_runtime).await {
            Ok(()) => info!("runtime + supervisor stopped"),
            Err(_) => warn!(
                deadline_ms = SUPERVISOR_STOP_DEADLINE.as_millis() as u64,
                "runtime/supervisor stop exceeded its deadline; proceeding with teardown"
            ),
        }

        // Phase (c): close the store (WAL checkpoint lands in M22). The checkpoint
        // may not overrun — a missed one is a replay, not data loss — so this is
        // the tightest budget.
        if let Some(close) = store_close {
            match timeout(STORE_CLOSE_DEADLINE, close.close()).await {
                Ok(()) => info!("store closed"),
                Err(_) => warn!(
                    deadline_ms = STORE_CLOSE_DEADLINE.as_millis() as u64,
                    "store close exceeded its deadline; proceeding"
                ),
            }
        }

        info!("teardown complete");
        drop(log_guard);
    }
}

/// Resolve when the process receives a shutdown signal: `ctrl_c` (SIGINT) or,
/// on unix, `SIGTERM` (what systemd/launchd/`docker stop` send). Registering the
/// `SIGTERM` handler here is also what stops the default kill-on-`SIGTERM`, so the
/// ordered teardown gets to run inside the SIGTERM→SIGKILL window.
pub async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = signal::ctrl_c().await {
            warn!(%error, "failed to install ctrl_c handler");
            // Never resolve: fall back to SIGTERM only rather than treating a
            // failed registration as an immediate shutdown.
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(error) => {
                warn!(%error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn per_phase_deadlines_sum_to_the_total_budget() {
        // The ratified split (RFC §7 ruling 1) must stay inside the ~10s SIGTERM
        // window and its three parts must sum to the named total — encode the
        // invariant so a future edit to one constant cannot silently drift it.
        assert_eq!(
            HTTP_DRAIN_DEADLINE + SUPERVISOR_STOP_DEADLINE + STORE_CLOSE_DEADLINE,
            TOTAL_SHUTDOWN_BUDGET
        );
        assert!(TOTAL_SHUTDOWN_BUDGET <= Duration::from_secs(10));
    }

    #[tokio::test]
    async fn signal_registration_succeeds_and_stays_pending() {
        // Installing the ctrl_c + SIGTERM handlers must not panic, and — absent an
        // actual signal — the wait must stay pending (a failed registration must
        // not read as an immediate shutdown).
        let wait = wait_for_shutdown_signal();
        tokio::pin!(wait);
        tokio::select! {
            () = &mut wait => panic!("no signal was sent; the wait must stay pending"),
            () = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    struct RecordingSupervisor(Arc<AtomicUsize>);
    #[async_trait]
    impl SupervisorStop for RecordingSupervisor {
        async fn stop_all(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct RecordingStore(Arc<AtomicUsize>);
    #[async_trait]
    impl StoreClose for RecordingStore {
        async fn close(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn run_cancels_token_drains_and_invokes_both_seams() {
        let token = CancellationToken::new();
        // A serve task that returns only once the token is cancelled — i.e. the
        // graceful-drain contract in miniature.
        let serve_token = token.clone();
        let server_join = tokio::spawn(async move { serve_token.cancelled().await });

        let supervisor_calls = Arc::new(AtomicUsize::new(0));
        let store_calls = Arc::new(AtomicUsize::new(0));
        let sequence = ShutdownSequence::new(token.clone(), server_join)
            .with_supervisor_stop(Box::new(RecordingSupervisor(supervisor_calls.clone())))
            .with_store_close(Box::new(RecordingStore(store_calls.clone())));

        let start = tokio::time::Instant::now();
        sequence.run().await;

        assert!(token.is_cancelled(), "run must cancel the token to start the drain");
        assert_eq!(supervisor_calls.load(Ordering::SeqCst), 1, "supervisor stop runs once");
        assert_eq!(store_calls.load(Ordering::SeqCst), 1, "store close runs once");
        assert!(
            start.elapsed() < TOTAL_SHUTDOWN_BUDGET,
            "a clean teardown completes well inside the budget"
        );
    }

    struct HangingStore;
    #[async_trait]
    impl StoreClose for HangingStore {
        async fn close(&self) {
            std::future::pending::<()>().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_hanging_phase_is_bounded_and_teardown_still_completes() {
        // A store close that never resolves must be cut at its deadline so the
        // process is never hung; with the clock paused the timeout is virtual.
        let token = CancellationToken::new();
        let server_join = tokio::spawn(async {});
        let sequence = ShutdownSequence::new(token, server_join)
            .with_store_close(Box::new(HangingStore));
        // Completes (does not hang) — the phase timeout fires on the paused clock.
        sequence.run().await;
    }
}
