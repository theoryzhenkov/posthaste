//! Deadline seam for IMAP operations.
//!
//! `imap-client` bounds only its `IDLE` loop (`idle_timeout`); every other
//! network round-trip — `CONNECT`, `LOGIN`, `SELECT`, `UID FETCH`, `UID STORE`,
//! `UID MOVE`, … — awaits the server with no deadline, so a hung or hostile
//! provider hangs the sync loop indefinitely. This module gives those waits one
//! declared override point ([`IMAP_OP_TIMEOUT_MS`]) and helpers
//! ([`with_deadline`] / [`with_deadline_resolve`]) that bound an operation and
//! map a missed deadline to a typed [`ImapAdapterError::Timeout`].
//!
//! A deadline stops a *hung* dependency, not a *slow-but-progressing* one
//! (engineering principle VI): the per-op budget is reset for each IMAP command,
//! so a large mailbox sync makes progress across many bounded round-trips rather
//! than getting one wall-clock cutoff for the whole batch.
//!
//! @spec docs/L0-providers#imap-smtp-sync-strategy

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use imap_client::client::tokio::ClientError;
use imap_client::tasks::tasks::TaskError;

use crate::ImapAdapterError;

/// The deadline for a single IMAP network operation (connect, authenticate,
/// select, fetch, store, move, …). Generous: providers under load can take tens
/// of seconds, but a server that has not responded in a minute is hung, not slow.
///
/// Held as an `AtomicU64` of milliseconds so a test can shrink it via
/// [`set_op_timeout_ms_for_testing`] to assert a missed deadline fires
/// (principle II: one declared seam a test can reach).
pub(crate) static IMAP_OP_TIMEOUT_MS: AtomicU64 = AtomicU64::new(60_000);

pub(crate) fn op_timeout() -> Duration {
    Duration::from_millis(IMAP_OP_TIMEOUT_MS.load(Ordering::Relaxed))
}

/// Test-only: serialize tests that shrink the process-global deadline seams
/// ([`IMAP_OP_TIMEOUT_MS`], `session::IMAP_IDLE_REISSUE_MS`) so parallel
/// tests never observe each other's shrunken deadlines.
#[cfg(test)]
pub(crate) fn seam_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Test-only: shrink the per-op deadline so a stalling server triggers a
/// [`ImapAdapterError::Timeout`] without waiting a minute. Returns a guard
/// that restores the previous value on drop.
#[cfg(test)]
pub(crate) fn set_op_timeout_ms_for_testing(ms: u64) -> impl FnOnce() {
    let previous = IMAP_OP_TIMEOUT_MS.swap(ms, Ordering::Relaxed);
    move || {
        IMAP_OP_TIMEOUT_MS.store(previous, Ordering::Relaxed);
    }
}

/// Bound a single IMAP network operation by the [`IMAP_OP_TIMEOUT_MS`] deadline.
/// The `operation` label names the call site in the resulting
/// [`ImapAdapterError::Timeout`] so a hung dependency is observable without a
/// stack dive.
///
/// Use at each `imap-client` round-trip (the uncontrolled boundary); the helper
/// is the single place the deadline is set, so a test or a per-provider override
/// has one seam to reach (principle II).
pub(crate) async fn with_deadline<F, T, E>(
    operation: &'static str,
    future: F,
) -> Result<T, ImapAdapterError>
where
    F: Future<Output = Result<T, E>>,
    E: Into<ImapAdapterError>,
{
    match tokio::time::timeout(op_timeout(), future).await {
        Ok(result) => result.map_err(Into::into),
        Err(_elapsed) => Err(ImapAdapterError::Timeout { operation }),
    }
}

/// Bound a `client.resolve(task)` round-trip by the [`IMAP_OP_TIMEOUT_MS`] deadline.
/// returns a nested result — the outer `ClientError` (transport) and the inner
/// `TaskError` (server rejected the command) — so this helper maps both layers
/// to [`ImapAdapterError`] using the adapter's existing conventions while
/// keeping the deadline the single seam.
pub(crate) async fn with_deadline_resolve<F, T>(
    operation: &'static str,
    future: F,
) -> Result<T, ImapAdapterError>
where
    F: Future<Output = Result<Result<T, TaskError>, ClientError>>,
{
    match tokio::time::timeout(op_timeout(), future).await {
        Ok(outer) => outer
            .map_err(ImapAdapterError::from)?
            .map_err(|error| ImapAdapterError::Client(error.to_string())),
        Err(_elapsed) => Err(ImapAdapterError::Timeout { operation }),
    }
}
