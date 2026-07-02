//! Shutdown + error types for the runtime near-node build (D29 split from
//! `build.rs`). Owns the shutdown handle and the build/shutdown error enums.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use posthaste_domain_model::{ConfigError, ServiceError, StoreError};
use thiserror::Error;

/// Shutdown ownership for authority runtime tasks and resources.
///
/// The first extraction slice owns no long-lived account tasks yet; this handle
/// records shutdown state so adapters already depend on the runtime-owned
/// shutdown seam instead of tearing resources down themselves.
///
/// spec: docs/runtime/internals/L2#runtime-shutdown-handle
pub struct RuntimeShutdownHandle {
    pub(crate) stopped: Arc<AtomicBool>,
}

impl RuntimeShutdownHandle {
    // Async by contract: shutdown is part of the runtime's async lifecycle
    // (start/await, shutdown/await) and will await task joins as it grows.
    #[allow(clippy::unused_async)]
    pub async fn shutdown(self) -> Result<(), RuntimeShutdownError> {
        self.stopped.store(true, Ordering::SeqCst);
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
