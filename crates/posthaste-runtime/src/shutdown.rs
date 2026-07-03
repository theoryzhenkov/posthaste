//! Shutdown + error types for the runtime near-node build (D29 split from
//! `build.rs`). Owns the shutdown handle and the build/shutdown error enums.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use posthaste_domain_model::{ConfigError, ServiceError, StoreError};
use thiserror::Error;
use tokio::task::JoinHandle;

/// Shutdown ownership for authority runtime tasks and resources.
///
/// Owns the runtime-scoped background tasks so the composition root's
/// [`ShutdownSequence`](posthaste_http_api_adapter) can stop them at teardown
/// step (b) rather than letting them detach and die mid-work on a signal-kill.
/// Today that is the near node's down-channel bridge task (audit N7): its
/// `JoinHandle` used to be dropped at spawn (`assembly.rs`), so `shutdown` could
/// not stop it; it is retained here.
///
/// spec: docs/runtime/internals/L2#runtime-shutdown-handle
/// spec: docs/eph/RFC-L2-lifecycle-and-errors#d60
pub struct RuntimeShutdownHandle {
    pub(crate) stopped: Arc<AtomicBool>,
    /// The down-channel bridge task of a remote near node (N7), retained so
    /// `shutdown` can stop it. `None` for an in-process runtime (it shares the
    /// authority server's bus, so there is no bridge task to own).
    pub(crate) down_channel_task: Option<JoinHandle<()>>,
}

impl RuntimeShutdownHandle {
    /// Stop the runtime's owned tasks and mark the runtime `Stopped`.
    ///
    /// Awaits the down-channel bridge task's termination (abort + join), so the
    /// returned completion is an actual "stopped" signal, not just the flag flip
    /// — the composition root's teardown can rely on the bridge being gone before
    /// it moves on to closing the store.
    pub async fn shutdown(self) -> Result<(), RuntimeShutdownError> {
        let Self {
            stopped,
            down_channel_task,
        } = self;
        stopped.store(true, Ordering::SeqCst);
        if let Some(task) = down_channel_task {
            // The bridge only maps frame semantics (evict/absorb/republish); a
            // cooperative stop is unnecessary — abort it and await the join so a
            // cancelled task is a completed one. A `Cancelled` JoinError is the
            // expected, benign outcome.
            task.abort();
            let _ = task.await;
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum RuntimeBuildError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("service error: {0}")]
    Service(#[from] ServiceError),
    #[error("invalid runtime build config: {0}")]
    InvalidConfig(String),
    #[error("io error for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("failed to read bootstrap config {path}: {source}")]
    BootstrapRead { path: PathBuf, source: io::Error },
    #[error("failed to parse bootstrap config {path}: {message}")]
    BootstrapParse { path: PathBuf, message: String },
    #[error("failed to read runtime clock: {0}")]
    Clock(String),
}

#[derive(Debug, Error)]
pub enum RuntimeShutdownError {
    #[error("runtime shutdown failed: {0}")]
    Failed(String),
}
