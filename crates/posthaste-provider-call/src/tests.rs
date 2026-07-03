//! M31 gate tests. All hermetic and virtual-time driven (`start_paused`): the
//! retry/breaker control flow runs against scripted attempts, and the blob
//! stall behavior runs against a synthetic in-memory byte stream — no socket, no
//! wall clock, so a "150 s" download asserts instantly and deterministically.

use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_stream::stream;
use tokio::time::Instant;

use posthaste_call_policy::{BackoffSchedule, CallClass, Terminality, BLOB_STALL, METADATA_TOTAL};

use crate::breaker::{BreakerConfig, BreakerPhaseView};
use crate::error::{CallErrorReason, ProviderCallError};
use crate::executor::{ExecutorConfig, ProviderCallExecutor};
use crate::stall::{drain_with_stall, stall_guard, StallError};

fn executor(schedule: BackoffSchedule, breaker: BreakerConfig) -> ProviderCallExecutor {
    ProviderCallExecutor::new(ExecutorConfig {
        schedule,
        breaker,
        trusted_hosts: Vec::new(),
        connect_timeout: None,
    })
    .expect("build executor")
}

fn rate_limited(retry_after: Duration) -> ProviderCallError {
    ProviderCallError {
        terminality: Terminality::Transient,
        reason: CallErrorReason::RateLimited(429),
        retry_after: Some(retry_after),
        detail: "429".to_string(),
    }
}

fn permanent() -> ProviderCallError {
    ProviderCallError {
        terminality: Terminality::Permanent,
        reason: CallErrorReason::Http(500),
        retry_after: None,
        detail: "permanent".to_string(),
    }
}

// ---- F2: the blob stall-deadline stream adapter -----------------------------

/// A slow-but-progressing blob (a chunk every `BLOB_STALL/2`, total ≫ the old
/// 10 s monoculture) **completes**: the stall deadline never fires because every
/// chunk arrives before its window closes.
#[tokio::test(start_paused = true)]
async fn slow_but_progressing_blob_completes() {
    let chunks = 10u8; // 10 × 15 s = 150 s total, ≫ the old 10 s total timeout.
    let gap = BLOB_STALL / 2;
    let body = stream! {
        for i in 0..chunks {
            tokio::time::sleep(gap).await;
            yield Ok::<_, Infallible>(vec![i]);
        }
    };
    let drained = drain_with_stall(body, BLOB_STALL).await;
    assert_eq!(drained.expect("blob should complete").len(), chunks as usize);
}

/// A stalled blob (a gap ≫ the stall window) **errors** — the read is dead, not
/// slow — and the error is the transient `Stall` class at the executor edge.
#[tokio::test(start_paused = true)]
async fn stalled_blob_errors() {
    let body = stream! {
        yield Ok::<_, Infallible>(vec![1u8, 2, 3]);
        tokio::time::sleep(BLOB_STALL * 2).await; // no chunk within the window
        yield Ok::<_, Infallible>(vec![4u8]);
    };
    let drained = drain_with_stall(body, BLOB_STALL).await;
    assert!(matches!(drained, Err(None)), "a stalled read must error");
}

/// The adapter itself surfaces a discrete `Stalled` item when the wrapped stream
/// goes quiet past the deadline.
#[tokio::test(start_paused = true)]
async fn stall_guard_yields_stalled_then_ends() {
    use futures_util::StreamExt;
    let inner = stream! {
        yield Ok::<u8, Infallible>(1);
        tokio::time::sleep(BLOB_STALL * 2).await;
        yield Ok::<u8, Infallible>(2);
    };
    let guarded = stall_guard(inner, BLOB_STALL);
    futures_util::pin_mut!(guarded);
    assert!(matches!(guarded.next().await, Some(Ok(1))));
    assert!(matches!(guarded.next().await, Some(Err(StallError::Stalled))));
    assert!(guarded.next().await.is_none(), "adapter ends after a stall");
}

// ---- F1: Retry-After is honored, not re-hammered ----------------------------

/// A 429 with `Retry-After` is honored: the retry loop sleeps ≥ the server's
/// backpressure before the next attempt (never re-hammers immediately), and the
/// call eventually succeeds. Asserted in virtual time — the delay is measured,
/// not slept for real.
#[tokio::test(start_paused = true)]
async fn retry_after_is_honored_not_rehammered() {
    let exec = executor(BackoffSchedule::default(), BreakerConfig::default());
    let retry_after = Duration::from_secs(5);
    let calls = AtomicUsize::new(0);

    let start = Instant::now();
    let result: Result<(), _> = exec
        .run("acct", || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 2 {
                    Err(rate_limited(retry_after))
                } else {
                    Ok(())
                }
            }
        })
        .await;

    assert!(result.is_ok(), "the call should succeed after backing off");
    assert_eq!(calls.load(Ordering::SeqCst), 3, "two retries then success");
    // Two honored Retry-After waits ⇒ ≥ 2 × 5 s elapsed; crucially ≥ the server's
    // asked-for delay, so it was never re-hammered early.
    assert!(
        start.elapsed() >= retry_after * 2,
        "must wait at least the Retry-After between attempts"
    );
}

/// The give-up bound (`max_attempts`) stops the retry loop and surfaces the last
/// transient error rather than looping forever.
#[tokio::test(start_paused = true)]
async fn retry_loop_gives_up_at_max_attempts() {
    let schedule = BackoffSchedule {
        max_attempts: 3,
        ..BackoffSchedule::default()
    };
    let exec = executor(schedule, BreakerConfig::default());
    let calls = AtomicUsize::new(0);
    let result: Result<(), _> = exec
        .run("acct", || {
            calls.fetch_add(1, Ordering::SeqCst);
            async move { Err(rate_limited(Duration::from_secs(1))) }
        })
        .await;
    assert!(result.is_err());
    // Attempts run at indices 0,1,2,3; the give-up check fires at
    // attempt-index == max_attempts(3), after the 4th attempt has failed.
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

// ---- D83: the per-account circuit breaker -----------------------------------

/// The breaker opens after N consecutive failures, then **fast-fails** without
/// invoking the attempt; after the cooldown a single half-open probe is admitted
/// and closes the breaker on success.
/// A single run whose attempt fails for the first three real invocations, then
/// succeeds — the shared `attempts` counter also proves a fast-failed
/// (short-circuited) run never invokes the attempt.
async fn breaker_run(
    exec: &ProviderCallExecutor,
    attempts: &AtomicUsize,
) -> Result<(), ProviderCallError> {
    exec.run("acct", || {
        let made = attempts.fetch_add(1, Ordering::SeqCst);
        async move {
            if made < 3 {
                Err(permanent())
            } else {
                Ok(())
            }
        }
    })
    .await
}

#[tokio::test(start_paused = true)]
async fn breaker_opens_fast_fails_then_half_open_probe_closes() {
    let breaker = BreakerConfig {
        enabled: true,
        failure_threshold: 3,
        cooldown: Duration::from_secs(45),
    };
    // No retries: each run is exactly one attempt ⇒ one breaker outcome.
    let schedule = BackoffSchedule {
        max_attempts: 0,
        ..BackoffSchedule::default()
    };
    let exec = executor(schedule, breaker);
    let attempts = AtomicUsize::new(0);

    // Three consecutive failures open the breaker.
    for _ in 0..3 {
        assert!(breaker_run(&exec, &attempts).await.is_err());
    }
    assert_eq!(exec.breaker_phase("acct"), BreakerPhaseView::Open);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);

    // While open, the next call fast-fails with the distinct CircuitOpen reason
    // and does NOT invoke the attempt (attempts count unchanged).
    let fast = breaker_run(&exec, &attempts)
        .await
        .expect_err("open breaker fast-fails");
    assert_eq!(fast.reason, CallErrorReason::CircuitOpen);
    assert_eq!(attempts.load(Ordering::SeqCst), 3, "attempt not invoked");

    // After the cooldown, a single half-open probe is admitted; it succeeds and
    // closes the breaker.
    tokio::time::advance(Duration::from_secs(45)).await;
    assert!(
        breaker_run(&exec, &attempts).await.is_ok(),
        "half-open probe succeeds"
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 4, "probe invoked the attempt");
    assert_eq!(exec.breaker_phase("acct"), BreakerPhaseView::Closed(0));
}

/// A half-open probe that *fails* re-opens the breaker for another cooldown
/// (it does not close on a failed probe).
#[tokio::test(start_paused = true)]
async fn breaker_half_open_probe_failure_reopens() {
    let breaker = BreakerConfig {
        enabled: true,
        failure_threshold: 2,
        cooldown: Duration::from_secs(30),
    };
    let schedule = BackoffSchedule {
        max_attempts: 0,
        ..BackoffSchedule::default()
    };
    let exec = executor(schedule, breaker);

    for _ in 0..2 {
        let _ = exec.run("acct", || async { Err::<(), _>(permanent()) }).await;
    }
    assert_eq!(exec.breaker_phase("acct"), BreakerPhaseView::Open);

    tokio::time::advance(Duration::from_secs(30)).await;
    // The admitted probe fails ⇒ breaker re-opens rather than closing.
    let _ = exec.run("acct", || async { Err::<(), _>(permanent()) }).await;
    assert_eq!(exec.breaker_phase("acct"), BreakerPhaseView::Open);
}

/// The breaker is per-account: one account tripping does not fast-fail another
/// (R86 — never global).
#[tokio::test(start_paused = true)]
async fn breaker_is_per_account_not_global() {
    let breaker = BreakerConfig {
        enabled: true,
        failure_threshold: 2,
        cooldown: Duration::from_secs(30),
    };
    let schedule = BackoffSchedule {
        max_attempts: 0,
        ..BackoffSchedule::default()
    };
    let exec = executor(schedule, breaker);

    for _ in 0..2 {
        let _ = exec.run("bad", || async { Err::<(), _>(permanent()) }).await;
    }
    assert_eq!(exec.breaker_phase("bad"), BreakerPhaseView::Open);
    // A healthy account is unaffected and its call runs.
    assert!(exec.run("good", || async { Ok::<_, ProviderCallError>(()) }).await.is_ok());
    assert_eq!(exec.breaker_phase("good"), BreakerPhaseView::Closed(0));
}

// ---- D81: the per-class deadline table is the single tuning surface ----------

/// The executor consumes the ratified per-class deadline table: metadata gets a
/// total (and no stall), blob gets a stall (and no total, F2's fix).
#[test]
fn per_class_deadline_table_is_consumed() {
    let metadata = CallClass::Metadata.deadline_policy();
    assert_eq!(metadata.total, Some(METADATA_TOTAL));
    assert_eq!(metadata.stall, None);

    let blob = CallClass::Blob.deadline_policy();
    assert_eq!(blob.total, None, "F2: a total on a streamed body is the bug");
    assert_eq!(blob.stall, Some(BLOB_STALL));
}
