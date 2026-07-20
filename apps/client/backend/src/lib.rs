//! The client's backend service core: the domain service over the SQLite
//! store, per-account provider runtimes, the domain-event bus with the store
//! generation, and the assembly that wires them together. The API layer
//! (queries, commands, SSE, blobs) mounts on [`AppState`] via [`serve`].

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{ConfigError, ServiceError, StoreError};
use posthaste_domain_service::{ConfigRepository, MailService, MailStore, SecretStore};
use posthaste_observability::{events, ph_info, ph_warn};
use posthaste_store::{DatabaseStore, RepairReport};

mod api;
mod backfill;
mod event_bus;
mod gateway;
mod oauth_refresh;
mod paths;
mod push;
mod secret;
mod supervisor;

/// Marker file name (inside the state root) that requests a quarantine-and-
/// rebuild of the database on the next open; embedding shells write it.
pub use posthaste_store::REPAIR_MARKER_FILE;

pub use event_bus::{EventBus, DEFAULT_EVENT_CAPACITY};
pub use paths::{AppPaths, ConnectionInfo, StoreLock};
pub use secret::{keyring_entry_location, SystemSecretStore};
pub use supervisor::{AccountSupervisor, DEFAULT_POLL_INTERVAL};

/// Deadline for stopping all account runtimes during shutdown.
pub const SUPERVISOR_STOP_DEADLINE: Duration = Duration::from_secs(5);

/// Everything the API layer needs: the service, the store handles, the event
/// bus + generation, and the account supervisor. Cheap to clone (all shared
/// handles).
#[derive(Clone)]
pub struct AppState {
    /// The domain service — the only component that touches the store.
    pub service: Arc<MailService>,
    /// The concrete SQLite store; carries `close()` for teardown.
    pub database_store: Arc<DatabaseStore>,
    /// The store behind its port trait, for consumers that only read/write
    /// through the domain ports.
    pub store: Arc<dyn MailStore>,
    /// The TOML config repository (accounts, smart mailboxes, settings).
    pub config: Arc<dyn ConfigRepository>,
    /// OS-keychain-backed secret store.
    pub secret_store: Arc<dyn SecretStore>,
    /// The domain-event bus, store generation, and run id.
    pub events: EventBus,
    /// Per-account runtimes: sync scheduling, push, health.
    pub supervisor: Arc<AccountSupervisor>,
    /// Resolved filesystem roots.
    pub paths: AppPaths,
    /// Exclusive advisory lock on the state root, held while the state is
    /// live so no second backend can open the same store. Released by
    /// [`AppState::shutdown`] (or when the last state handle drops).
    pub store_lock: Arc<StoreLock>,
    /// Repair report when the database was quarantined and rebuilt on open,
    /// so the API can surface it and trigger a re-sync.
    pub repair: Option<Arc<RepairReport>>,
}

/// Assembly options. `Default` resolves paths from the environment and uses
/// production intervals and the OS keychain.
pub struct BuildOptions {
    pub paths: AppPaths,
    /// Provider poll interval (the safety net under push).
    pub poll_interval: Duration,
    /// Domain-event broadcast capacity.
    pub event_capacity: usize,
    /// Secret store override (tests inject an in-memory store).
    pub secret_store: Option<Arc<dyn SecretStore>>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            paths: AppPaths::resolve(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            event_capacity: DEFAULT_EVENT_CAPACITY,
            secret_store: None,
        }
    }
}

impl BuildOptions {
    /// Options rooted at explicit directories (tests, embedding hosts).
    pub fn at(paths: AppPaths) -> Self {
        Self {
            paths,
            ..Default::default()
        }
    }
}

/// Assembly failure: config could not load, the store could not open, or the
/// initial projection sync failed.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("service error: {0}")]
    Service(#[from] ServiceError),
    #[error("io error at {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("the mail store at {path} is in use by another running Posthaste instance")]
    StoreLocked { path: std::path::PathBuf },
}

impl AppState {
    /// Assemble the service core: open + migrate the store, build the
    /// service over it, create the event bus and the account supervisor, and
    /// start a runtime for every configured account. The store's deferred
    /// maintenance (body-cache repair, address-book and FTS backfill) runs
    /// on the blocking pool after this returns.
    ///
    /// Must run within a tokio runtime.
    pub async fn assemble(options: BuildOptions) -> Result<Self, BuildError> {
        let BuildOptions {
            paths,
            poll_interval,
            event_capacity,
            secret_store,
        } = options;

        std::fs::create_dir_all(&paths.state_root).map_err(|source| BuildError::Io {
            path: paths.state_root.clone(),
            source,
        })?;

        // One live backend per store: take the exclusive state-root lock
        // before anything opens the database. SQLite WAL mode permits a
        // concurrent multi-process open, so this lock is the only thing
        // stopping two sync engines from racing over one store.
        let lock_path = paths.lock_path();
        let store_lock = StoreLock::acquire(&lock_path)
            .map_err(|source| BuildError::Io {
                path: lock_path,
                source,
            })?
            .ok_or_else(|| BuildError::StoreLocked {
                path: paths.state_root.clone(),
            })?;
        let store_lock = Arc::new(store_lock);

        let config_repo = TomlConfigRepository::open(&paths.config_root)?;
        if config_repo.is_empty() {
            config_repo.initialize_defaults()?;
        }
        let config: Arc<dyn ConfigRepository> = Arc::new(config_repo);

        // Open (or create) the store; the schema migrates forward inside
        // `open`, and a corrupt database is quarantined and rebuilt (it is a
        // rebuildable projection — accounts live in config, secrets in the
        // keychain).
        let (database_store, repair) =
            DatabaseStore::open_with_repair(paths.db_path(), &paths.state_root)?;
        let database_store = Arc::new(database_store);
        spawn_deferred_store_maintenance(&database_store);

        let store: Arc<dyn MailStore> = database_store.clone();
        let service = Arc::new(MailService::new(database_store.clone(), config.clone()));
        service.sync_source_projections()?;
        let sources = service.list_sources()?;

        // A quarantined-and-rebuilt store salvaged the op log (and the
        // `draft_alias` map) but starts with an EMPTY overlay. Repopulate it
        // from (log, base) now so parked content — unsent drafts, provisional
        // Sent rows awaiting adoption — does not vanish for the
        // repair → first-completed-sync window. The post-sync sweep would
        // otherwise be the first thing to re-derive it, and only at the end of
        // a completed cycle; rows with no base successor (a provisional Sent
        // id) are not touched by a sync base-write in the meantime.
        if repair.is_some() {
            for source in &sources {
                service.replay_account_overrides(&source.id).await?;
            }
        }

        let events = EventBus::new(event_capacity);
        let secret_store: Arc<dyn SecretStore> =
            secret_store.unwrap_or_else(|| Arc::new(SystemSecretStore));

        let supervisor = Arc::new(AccountSupervisor::new(
            service.clone(),
            store.clone(),
            secret_store.clone(),
            events.clone(),
            poll_interval,
        ));
        for source in sources {
            supervisor.start_account(&source).await;
        }

        ph_info!(
            events::DATABASE_OPENED,
            state_root = %paths.state_root.display(),
            account_count = supervisor.account_count(),
            run_id = events.run_id(),
            "backend service core assembled"
        );

        Ok(Self {
            service,
            database_store,
            store,
            config,
            secret_store,
            events,
            supervisor,
            paths,
            store_lock,
            repair: repair.map(Arc::new),
        })
    }

    /// Ordered teardown: stop account runtimes (cooperative, bounded), close
    /// the store (checkpoint + release file handles), then release the
    /// state-root lock so a successor can open the store. Idempotent.
    pub async fn shutdown(&self) {
        self.supervisor.stop_all(SUPERVISOR_STOP_DEADLINE).await;
        self.database_store.close();
        self.store_lock.release();
    }
}

/// Deferred post-startup store maintenance, off the open path and best
/// effort: the scans are idempotent, so the next startup catches up whatever
/// a failed pass missed.
fn spawn_deferred_store_maintenance(database_store: &Arc<DatabaseStore>) {
    let repair_store = database_store.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(error) = repair_store.repair_body_cache_objects() {
            ph_warn!(
                events::STORE_STARTUP_BODY_CACHE_REPAIR_FAILED,
                error = %error,
                "deferred startup body-cache repair failed"
            );
        }
        if let Err(error) = repair_store.backfill_address_book() {
            ph_warn!(
                events::STORE_STARTUP_ADDRESS_BOOK_BACKFILL_FAILED,
                error = %error,
                "deferred startup address-book backfill failed"
            );
        }
        match repair_store.backfill_message_fts() {
            Ok(true) => ph_info!(
                events::STORE_STARTUP_MESSAGE_FTS_BACKFILL_COMPLETED,
                "deferred startup full-text-index rebuild completed"
            ),
            Ok(false) => {}
            Err(error) => ph_warn!(
                events::STORE_STARTUP_MESSAGE_FTS_BACKFILL_FAILED,
                error = %error,
                "deferred startup full-text-index backfill failed"
            ),
        }
    });
}

/// A bound API server: the loopback address actually bound (so an ephemeral
/// port request resolves to the real port for the connection-info file) and
/// the serving task.
pub struct ServerHandle {
    pub addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl ServerHandle {
    /// Stop serving.
    pub fn abort(&self) {
        self.task.abort();
    }

    /// Wait for the serving task to finish without consuming the handle, so
    /// the caller can still `abort` afterwards.
    pub async fn wait(&mut self) {
        let _ = (&mut self.task).await;
    }

    /// Wait for the serving task to finish.
    pub async fn join(self) {
        let _ = self.task.await;
    }
}

/// Bind the API on loopback and serve [`router`] over it. `port` 0 requests
/// an ephemeral port; the bound address is on the returned handle. `token`
/// is the session secret every request must present — the caller publishes
/// it through the connection-info file.
pub async fn serve(state: AppState, port: u16, token: String) -> std::io::Result<ServerHandle> {
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
    let addr = listener.local_addr()?;
    let app = router(state, token);
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!(%error, "API server exited with an error");
        }
    });
    Ok(ServerHandle { addr, task })
}

/// The API router over [`AppState`]: queries, commands, the event stream,
/// and blobs, guarded by the session `token`. Routes are mounted at the root
/// and under `/api`.
pub fn router(state: AppState, token: String) -> axum::Router {
    api::build_router(state, token)
}
