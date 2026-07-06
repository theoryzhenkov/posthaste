//! Far-node assembly: builds the authority server (store + provider engine + IMAP) and
//! composes a runtime over it via [`posthaste_runtime::assemble_runtime`]. The
//! near node itself (handle, views, links, read cache, pending set, the remote
//! transport, and `build_remote_runtime`) lives in `posthaste-runtime`.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use posthaste_authority_server_link::AuthorityServerLinkHandle;
use posthaste_config::TomlConfigRepository;
use posthaste_contract_core::{RuntimeLifecycle, RuntimeStatus, RuntimeStoreStatus};
use posthaste_domain_model::DomainEvent;
use posthaste_domain_service::{ConfigRepository, MailService, MailStore, SecretStore};
use posthaste_observability::{events, ph_warn};
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;

use posthaste_runtime::{
    assemble_runtime, AuthorityServerTransportConfig, AuthorityServerTransportDecorator, ReadCache,
    RemoteAuthorityServer, RuntimeAssembly, RuntimeBuildConfig, RuntimeBuildError, RuntimeHandle,
    RuntimeShutdownHandle, SystemSecretStore,
};

use crate::account_reads::{AccountReadService, DefaultAccountRuntimeOverviewProvider};
use crate::account_repository::AccountRepository;
use crate::authority_server::AuthorityServer;
use crate::bootstrap::initialize_config;
use crate::local_authority_server::LocalAuthorityServer;
use crate::mail_queries::MailQueryService;
use crate::mutations::AccountMutationService;
use crate::{
    AccountRuntimeOverviewProvider, AccountSupervisor, LiveAccountRuntimeProvider,
    UnavailableLiveAccountRuntimeProvider,
};

/// Result of building the authority runtime in-process (authority server + near node).
pub struct AuthorityServerBuild {
    pub handle: RuntimeHandle,
    pub shutdown: RuntimeShutdownHandle,
    pub runtime_status: RuntimeStatus,
    pub account_supervisor: Arc<AccountSupervisor>,
    /// The authority server's account-mutation service, surfaced so the host can run the
    /// OAuth holdout (account creation from a provider exchange) directly — it
    /// is an authority server operation, not part of the renderer-facing handle.
    pub account_mutations: Arc<AccountMutationService>,
    /// Runtime-owned secret store exposed for host-owned auth-token setup.
    pub secret_store: Arc<dyn SecretStore>,
    /// The authority server's service/store/secret-store/event-bus graph, surfaced for
    /// hosts (testkit/bench) that need direct access to seed state and observe
    /// events. The wrapper migration is complete (RFC D20); this is now the
    /// permanent service-graph handle on the build, not a temporary bridge.
    pub api_bridge: AuthorityServerApiMigrationBridge,
    /// The in-process runtime↔authority-server link. A split-authority-server host serves its
    /// transport over the link wire (`link_router`) so a remote runtime can
    /// connect; the in-process runtime already holds the same link internally.
    ///
    /// @spec docs/replication/authority-server-link/L1#3-the-backendapi-contract
    pub authority_server_link: AuthorityServerLinkHandle,
    /// The concrete SQLite store, for teardown step (c) — the composition root
    /// closes it as the final phase of the [`ShutdownSequence`](posthaste_http_api_adapter)
    /// (D62/M20).
    pub database_store: Arc<DatabaseStore>,
    /// The far node itself, retained so the composition root can spawn the
    /// in-process rule engine over its event bus + apply surface
    /// ([`spawn_rule_engine`](AuthorityServerBuild::spawn_rule_engine)). Private:
    /// `AuthorityServer` is a crate-internal type.
    authority_server: Arc<AuthorityServer>,
    /// The config root, for loading `rules.toml` at rule-engine spawn.
    config_root: std::path::PathBuf,
}

impl AuthorityServerBuild {
    /// Spawn the in-process automation rule engine over this node's event bus
    /// (RFC-L2-scripting S5). Loads `rules.toml` from the config root; if there
    /// are no enabled rules, spawns nothing and returns `None`.
    ///
    /// `minter` supplies per-invocation capability tokens for Level-1 hook
    /// actions (webhook/exec). Pass `None` for a deployment without the macaroon
    /// root key (Level-0 tag/move/notify still run; hook actions dead-letter).
    /// The returned handle keeps the engine alive; drop it to stop.
    ///
    /// ALWAYS spawns, even with zero enabled rules: the engine's bus subscription
    /// must be live from the start so a GUI-created rule (via the returned
    /// handle's [`ManagedRulesHandle`](crate::rules::ManagedRulesHandle)) hot-swaps
    /// into a running evaluator and fires without a restart (reload path,
    /// prerequisite 2). A `rules.toml`/`rules.d` load failure logs and starts
    /// empty rather than skipping the engine, so a later write still reloads.
    pub fn spawn_rule_engine(
        &self,
        minter: Option<crate::rules::SharedMinter>,
    ) -> crate::rules::RuleEngineHandle {
        let enabled = match crate::rules::load_rules(&self.config_root) {
            Ok(rules) => rules.into_iter().filter(|rule| rule.enabled).collect(),
            Err(error) => {
                ph_warn!(
                    events::RULE_ENGINE_STARTED,
                    error = %error,
                    "failed to load rules; rule engine starting empty (writes will reload)"
                );
                Vec::new()
            }
        };
        crate::rules::spawn_engine(
            self.authority_server.clone(),
            self.api_bridge.service.clone(),
            self.api_bridge.store.clone(),
            self.api_bridge.event_sender.clone(),
            self.config_root.clone(),
            enabled,
            minter,
        )
    }
}

/// The authority server's service-graph handle: the `MailService`, `MailStore`, secret
/// store, and domain-event bus built by `build_authority_server_parts`. Carried on the build so
/// hosts (testkit/bench) can seed state and observe events directly. The wrapper
/// migration is complete (RFC D20) — a permanent handle, not a temporary bridge.
#[derive(Clone)]
pub struct AuthorityServerApiMigrationBridge {
    pub service: Arc<MailService>,
    pub store: Arc<dyn MailStore>,
    pub secret_store: Arc<dyn SecretStore>,
    pub event_sender: broadcast::Sender<DomainEvent>,
}

impl AuthorityServerApiMigrationBridge {
    pub fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            service,
            store,
            secret_store,
            event_sender,
        }
    }
}

/// Build the authority runtime without binding HTTP or depending on Tauri.
///
/// The bundled/daemon composition ([replication authority-server-link L2 §7](../replication/authority-server-link/L2.md)):
/// build the authority server far node, then the runtime near node over it (in-process
/// link by default). Behavior-preserving — the same graph as before, factored so
/// the two roles can also be composed independently by the lean binaries.
///
/// spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first
/// spec: docs/runtime/internals/L2#runtime-builder-transport-free
pub async fn build_authority_server(
    config: RuntimeBuildConfig,
) -> Result<AuthorityServerBuild, RuntimeBuildError> {
    let authority_server = build_authority_server_parts(&config).await?;
    Ok(build_runtime(authority_server, config))
}

/// A standalone authority server far node ([replication authority-server-link L2 §7](../replication/authority-server-link/L2.md),
/// assertion `authority-server-builds-standalone`): the store + service + account
/// supervisor + the L4 [`AuthorityServer`], with NO runtime near node. The
/// `posthaste-authority-server` role binary serves [`transport`](AuthorityServerNode::transport)
/// over `link_router` so a remote runtime drives it across the link.
pub struct AuthorityServerNode {
    transport: AuthorityServerLinkHandle,
    /// Held so the supervisor's account tasks keep running for the node's life
    /// (also reachable through the transport's far node); surfaced for teardown
    /// step (b) via [`AuthorityServerNode::account_supervisor`].
    account_supervisor: Arc<AccountSupervisor>,
    /// The concrete store, surfaced for teardown step (c).
    database_store: Arc<DatabaseStore>,
    runtime_status: RuntimeStatus,
}

impl AuthorityServerNode {
    /// The in-process link transport (both trait halves of the D33 seam) over
    /// this authority server — hand to `link_router`.
    pub fn transport(&self) -> AuthorityServerLinkHandle {
        self.transport.clone()
    }

    /// The account supervisor, for teardown step (b) (D60/D61). The standalone
    /// authority server binary wires it into its `ShutdownSequence`.
    pub fn account_supervisor(&self) -> Arc<AccountSupervisor> {
        self.account_supervisor.clone()
    }

    /// The concrete store, for teardown step (c) (D62). Wired into the standalone
    /// authority server binary's `ShutdownSequence`.
    pub fn database_store(&self) -> Arc<DatabaseStore> {
        self.database_store.clone()
    }

    /// The authority server's startup status (store readiness + account count).
    pub fn runtime_status(&self) -> &RuntimeStatus {
        &self.runtime_status
    }
}

/// Build a standalone authority server far node (no runtime). The far node is live after
/// this returns (the supervisor has started its accounts); serve
/// [`AuthorityServerNode::transport`] over the link to expose it to a remote runtime.
pub async fn build_authority_server_node(
    config: RuntimeBuildConfig,
) -> Result<AuthorityServerNode, RuntimeBuildError> {
    let authority_server = build_authority_server_parts(&config).await?;
    let transport = AuthorityServerLinkHandle::new(Arc::new(LocalAuthorityServer::new(
        authority_server.authority_server.clone(),
    )));
    Ok(AuthorityServerNode {
        transport,
        account_supervisor: authority_server.account_supervisor.clone(),
        database_store: authority_server.database_store.clone(),
        runtime_status: authority_server.runtime_status,
    })
}

/// A lean runtime near node ([replication authority-server-link L2 §7](../replication/authority-server-link/L2.md)): the
/// link / view / pending-set machinery over a REMOTE authority server link, with NO local
/// authority server (no store, service, or supervisor). The `posthaste-runtime` role (the
/// daemon configured with a remote authority server) builds this — reads + writes cross
/// the link, and the down-channel bridge keeps the cache and views live.
pub(crate) struct AuthorityServerParts {
    secret_store: Arc<dyn SecretStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    account_supervisor: Arc<AccountSupervisor>,
    account_mutations: Arc<AccountMutationService>,
    authority_server: Arc<AuthorityServer>,
    api_bridge: AuthorityServerApiMigrationBridge,
    runtime_status: RuntimeStatus,
    /// The concrete SQLite store, retained so the composition root can close it
    /// as teardown step (c) (D62/M20). The `api_bridge` holds the same store as
    /// `Arc<dyn MailStore>`; the concrete handle is what carries `close()`.
    database_store: Arc<DatabaseStore>,
}

/// Build the authority server far node alone (no runtime). Used directly by a
/// authority-server-only deployment and as the first half of the bundled build.
pub(crate) async fn build_authority_server_parts(
    config: &RuntimeBuildConfig,
) -> Result<AuthorityServerParts, RuntimeBuildError> {
    if config.event_channel_capacity == 0 {
        return Err(RuntimeBuildError::InvalidConfig(
            "event_channel_capacity must be greater than zero".to_string(),
        ));
    }

    fs::create_dir_all(&config.state_root).map_err(|source| RuntimeBuildError::Io {
        path: config.state_root.clone(),
        source,
    })?;
    fs::create_dir_all(&config.cache_root).map_err(|source| RuntimeBuildError::Io {
        path: config.cache_root.clone(),
        source,
    })?;

    let config_repo = TomlConfigRepository::open(&config.config_root)?;
    initialize_config(&config_repo, config.bootstrap_path.as_deref())?;

    let database_store = Arc::new(DatabaseStore::open(
        config.state_root.join("mail.sqlite"),
        &config.state_root,
    )?);
    // Deferred post-startup body-cache repair (N15 / RFC-L2-lifecycle D67(b) /
    // M27 sub-unit (b)): this used to run unconditionally inside
    // `init_schema`, blocking `DatabaseStore::open` behind an unbounded
    // startup scan. It now runs off that path, on the blocking pool, after
    // the store above is already open and serving real reads/writes. Best
    // effort: a failure here is logged and does not fail startup — the scan
    // is idempotent, so the next repair (a future retry, or the next
    // process startup) catches up whatever this pass missed.
    {
        let repair_store = database_store.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(error) = repair_store.repair_body_cache_objects() {
                ph_warn!(
                    events::STORE_STARTUP_BODY_CACHE_REPAIR_FAILED,
                    error = %error,
                    "deferred startup body-cache repair failed"
                );
            }
        });
    }
    let store: Arc<dyn MailStore> = database_store.clone();
    let config_repo: Arc<dyn ConfigRepository> = Arc::new(config_repo);
    let service = Arc::new(MailService::new(
        database_store.clone(),
        config_repo.clone(),
    ));

    service.sync_source_projections()?;
    let account_count = service.list_sources()?.len();

    let (event_sender, _) = broadcast::channel(config.event_channel_capacity);
    let secret_store = config
        .secret_store
        .clone()
        .unwrap_or_else(|| Arc::new(SystemSecretStore));

    let runtime_status = RuntimeStatus {
        lifecycle: RuntimeLifecycle::Ready,
        store: RuntimeStoreStatus {
            config_loaded: true,
            state_store_open: true,
            cache_root_ready: true,
        },
        account_count,
    };

    let api_bridge = AuthorityServerApiMigrationBridge::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
    );
    let account_supervisor = Arc::new(AccountSupervisor::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
        config.poll_interval,
    ));
    // Boot path (D98(a) / Sc1): splay each account's initial Startup sync within
    // the governor's window so N accounts started in this tight loop don't all
    // open a provider sync at the same instant (the boot storm).
    for source in service.list_sources()? {
        account_supervisor.start_account_on_boot(&source).await;
    }
    let account_reads = Arc::new(AccountReadService::new(
        api_bridge.service.clone(),
        account_supervisor.clone(),
    ));
    let account_repository = Arc::new(AccountRepository::new(
        api_bridge.service.clone(),
        api_bridge.secret_store.clone(),
    ));
    let account_mutations = Arc::new(AccountMutationService::new(
        api_bridge.service.clone(),
        api_bridge.store.clone(),
        account_repository,
        api_bridge.event_sender.clone(),
        account_supervisor.clone(),
        account_reads.clone(),
    ));
    let mail_queries = Arc::new(MailQueryService::new(
        api_bridge.service.clone(),
        account_supervisor.clone(),
    ));
    let authority_server = Arc::new(AuthorityServer::new(
        service.clone(),
        store.clone(),
        mail_queries,
        account_reads,
        Some(account_mutations.clone()),
        account_supervisor.clone(),
        event_sender.clone(),
    ));
    // The send-bridge (step 3): route async outbox settlements → routed
    // `Settlement` frames. Always on — every deployment (bundled + standalone far
    // node) accepts sends whose verdict is deferred to the async flush.
    crate::authority_server::spawn_settlement_bridge(&authority_server, &event_sender);

    Ok(AuthorityServerParts {
        secret_store,
        event_sender,
        account_supervisor,
        account_mutations,
        authority_server,
        api_bridge,
        runtime_status,
        database_store,
    })
}

/// Compose a runtime near node over the in-process authority server
/// ([replication authority-server-link L2 §7](../replication/authority-server-link/L2.md)). The link
/// is config-selected (in-process over the authority server, or remote); the read cache
/// follows the same selection. Surfaces the authority server's account-mutation service
/// for the OAuth holdout. Must run within a Tokio runtime.
pub(crate) fn build_runtime(
    authority_server: AuthorityServerParts,
    config: RuntimeBuildConfig,
) -> AuthorityServerBuild {
    let AuthorityServerParts {
        secret_store,
        event_sender,
        account_supervisor,
        account_mutations,
        authority_server,
        api_bridge,
        runtime_status,
        database_store,
    } = authority_server;
    // Retain the config root for the rule engine (loads `rules.toml`) before the
    // config is destructured.
    let config_root = config.config_root.clone();
    let rule_engine_far_node = authority_server.clone();
    // Take the transport selection out of the config (the override decorator is
    // `FnOnce`, so it is moved, not cloned); the rest of the config was consumed
    // by `build_authority_server_parts`.
    let RuntimeBuildConfig {
        authority_server_transport,
        authority_server_transport_override,
        ..
    } = config;

    let (authority_server_link, down_channel) = select_authority_server_link(
        &authority_server_transport,
        authority_server_transport_override,
        authority_server.clone(),
    );
    let reads = Arc::new(build_read_cache(
        &authority_server_transport,
        &authority_server,
        &authority_server_link,
    ));
    // A split runtime drives its cache + views from the authority server down-channel; an
    // in-process runtime shares the authority server's bus, so no bridge is needed.
    let composed = assemble_runtime(RuntimeAssembly {
        authority_server_link: authority_server_link.clone(),
        reads,
        event_sender,
        startup_status: runtime_status.clone(),
        down_channel,
        // DS7: back the runtime's apply-scoped idempotency ledger with the
        // authority server's SQLite `apply_ledger` table, so keyed
        // direct-apply/send/draft decisions survive the in-memory TTL reap
        // and a process restart (never re-executed on redelivery).
        durable_apply: Some(Arc::new(
            crate::apply_ledger_store::StoreDurableApplyLedger::new(database_store.clone()),
        )),
    });

    AuthorityServerBuild {
        handle: composed.handle,
        shutdown: composed.shutdown,
        runtime_status,
        account_supervisor,
        account_mutations,
        secret_store,
        api_bridge,
        authority_server_link,
        database_store,
        authority_server: rule_engine_far_node,
        config_root,
    }
}

fn build_read_cache(
    transport: &AuthorityServerTransportConfig,
    authority_server: &Arc<AuthorityServer>,
    authority_server_link: &AuthorityServerLinkHandle,
) -> ReadCache {
    match transport {
        // Remote: read through the same RemoteAuthorityServer the link uses (shared HTTP
        // client), retaining what flows back. In-process: read straight through
        // a LocalAuthorityServer over the co-located far node, retaining nothing.
        AuthorityServerTransportConfig::Remote { .. } => {
            ReadCache::retaining(authority_server_link.api().clone())
        }
        AuthorityServerTransportConfig::InProcess => ReadCache::passthrough(Arc::new(
            LocalAuthorityServer::new(authority_server.clone()),
        )),
    }
}

/// Build the runtime↔authority-server [`AuthorityServerLinkHandle`] over its config-selected transport
/// ([replication authority-server-link L2 §6](../replication/authority-server-link/L2.md), assertion `transport-selected-by-config`).
/// The co-located default wraps the in-process far node; `Remote` wraps a
/// [`RemoteAuthorityServer`] pointed at an authority server serving the link wire. An override
/// decorator, when present, composes over that real transport (delegating what
/// it does not intercept) — it does not replace the surface.
fn select_authority_server_link(
    transport: &AuthorityServerTransportConfig,
    override_decorator: Option<AuthorityServerTransportDecorator>,
    authority_server: Arc<AuthorityServer>,
) -> (
    AuthorityServerLinkHandle,
    Option<tokio::sync::mpsc::UnboundedReceiver<posthaste_authority_server_link::SequencedFrame>>,
) {
    let (base, down_channel) = match transport {
        AuthorityServerTransportConfig::InProcess => (
            AuthorityServerLinkHandle::new(Arc::new(LocalAuthorityServer::new(authority_server))),
            None,
        ),
        AuthorityServerTransportConfig::Remote { base_url, token } => {
            // The remote down-channel is the near-end ENGINE's reconnect loop
            // (M9b2) — taken from the raw transport, not a trait subscribe.
            let remote = Arc::new(RemoteAuthorityServer::with_token(
                base_url.clone(),
                token.clone(),
            ));
            let down_channel = remote.take_down_channel();
            (AuthorityServerLinkHandle::new(remote), down_channel)
        }
    };
    let handle = match override_decorator {
        Some(decorate) => decorate(base),
        None => base,
    };
    (handle, down_channel)
}

/// A runtime handle built from pre-existing api parts, plus the authority server's
/// account-mutation service for the OAuth holdout. Used by test harnesses that
/// stand up a runtime around a pre-configured service graph (RFC D20).
pub struct MigrationRuntime {
    pub handle: RuntimeHandle,
    pub account_mutations: Arc<AccountMutationService>,
}

/// Build a runtime handle around a pre-existing service/store graph — the
/// test-harness entry point for standing up a runtime without a fresh
/// `build_authority_server_parts` (RFC D20).
pub fn from_api_bridge_for_migration(
    api_bridge: AuthorityServerApiMigrationBridge,
    account_count: usize,
) -> RuntimeHandle {
    migration_runtime(
        api_bridge,
        account_count,
        Arc::new(DefaultAccountRuntimeOverviewProvider),
        Arc::new(UnavailableLiveAccountRuntimeProvider),
        None,
    )
    .0
}

pub fn from_api_bridge_with_account_supervisor_for_migration(
    api_bridge: AuthorityServerApiMigrationBridge,
    account_count: usize,
    account_supervisor: Arc<AccountSupervisor>,
) -> MigrationRuntime {
    let (handle, account_mutations) = migration_runtime(
        api_bridge,
        account_count,
        account_supervisor.clone(),
        account_supervisor.clone(),
        Some(account_supervisor),
    );
    MigrationRuntime {
        handle,
        account_mutations: account_mutations
            .expect("the account-supervisor variant always builds the mutation service"),
    }
}

fn migration_runtime(
    api_bridge: AuthorityServerApiMigrationBridge,
    account_count: usize,
    status_provider: Arc<dyn AccountRuntimeOverviewProvider>,
    live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
    account_supervisor: Option<Arc<AccountSupervisor>>,
) -> (RuntimeHandle, Option<Arc<AccountMutationService>>) {
    let account_reads = Arc::new(AccountReadService::new(
        api_bridge.service.clone(),
        status_provider,
    ));
    let query_supervisor = account_supervisor.clone().unwrap_or_else(|| {
        Arc::new(AccountSupervisor::new(
            api_bridge.service.clone(),
            api_bridge.store.clone(),
            api_bridge.secret_store.clone(),
            api_bridge.event_sender.clone(),
            Duration::from_secs(60),
        ))
    });
    let account_mutations = account_supervisor.map(|supervisor| {
        let account_repository = Arc::new(AccountRepository::new(
            api_bridge.service.clone(),
            api_bridge.secret_store.clone(),
        ));
        Arc::new(AccountMutationService::new(
            api_bridge.service.clone(),
            api_bridge.store.clone(),
            account_repository,
            api_bridge.event_sender.clone(),
            supervisor,
            account_reads.clone(),
        ))
    });
    let mail_queries = Arc::new(MailQueryService::new(
        api_bridge.service.clone(),
        query_supervisor,
    ));
    let authority_server = Arc::new(AuthorityServer::new(
        api_bridge.service.clone(),
        api_bridge.store.clone(),
        mail_queries,
        account_reads,
        account_mutations.clone(),
        live_accounts.clone(),
        api_bridge.event_sender.clone(),
    ));
    let reads = Arc::new(ReadCache::passthrough(Arc::new(LocalAuthorityServer::new(
        authority_server.clone(),
    ))));
    let runtime_status = RuntimeStatus {
        lifecycle: RuntimeLifecycle::Ready,
        store: RuntimeStoreStatus {
            config_loaded: true,
            state_store_open: true,
            cache_root_ready: false,
        },
        account_count,
    };
    let authority_server_link =
        AuthorityServerLinkHandle::new(Arc::new(LocalAuthorityServer::new(authority_server)));
    let composed = assemble_runtime(RuntimeAssembly {
        authority_server_link,
        reads,
        event_sender: api_bridge.event_sender,
        startup_status: runtime_status,
        down_channel: None,
        // The migration/test-harness runtime is built over an abstract
        // `MailStore` (no `DatabaseStore` in hand), so its apply ledger stays
        // in-memory-only — the pre-DS7 baseline the harness tests pin.
        durable_apply: None,
    });
    (composed.handle, account_mutations)
}
