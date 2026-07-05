use super::*;

impl AccountSupervisor {
    /// Create a supervisor with shared services and the configured poll interval.
    pub fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
        poll_interval: Duration,
    ) -> Self {
        Self {
            shared: Arc::new(SupervisorShared {
                service,
                store,
                secret_store,
                event_sender,
                gateways: RwLock::new(HashMap::new()),
                runtime_overviews: RwLock::new(HashMap::new()),
                runtime_generations: RwLock::new(HashMap::new()),
                sync_cycle_generations: RwLock::new(HashMap::new()),
                known_accounts: RwLock::new(HashSet::new()),
                account_count: AtomicUsize::new(0),
                cache_resources: Mutex::new(CacheResourceGovernor::new(
                    Instant::now(),
                    CacheResourcePolicy::default(),
                )),
                sync_governor: SyncGovernor::production(),
                poll_interval,
                oauth_refresh_flights: Mutex::new(HashMap::new()),
            }),
            runtimes: RwLock::new(HashMap::new()),
            root_cancel: CancellationToken::new(),
        }
    }

    /// Start (or restart) the async runtime for an account. Stops any existing
    /// runtime first. Disabled accounts get a `Disabled` status without spawning
    /// a task. Interactive path: the initial sync fires immediately (no splay).
    pub async fn start_account(&self, account: &AccountSettings) {
        self.start_account_inner(account, false).await;
    }

    /// Boot-loop entry (D98(a) / Sc1): like [`start_account`], but the initial
    /// `Startup` sync is randomly splayed within the governor's window so N
    /// accounts started in a tight loop do not all sync at the same instant.
    pub async fn start_account_on_boot(&self, account: &AccountSettings) {
        self.start_account_inner(account, true).await;
    }

    async fn start_account_inner(&self, account: &AccountSettings, splay_startup: bool) {
        self.stop_account(&account.id).await;
        self.shared.register_account(&account.id).await;
        if !account.enabled {
            ph_info!(
                events::SUPERVISOR_ACCOUNT_DISABLED,
                account_id = %account.id,
                "account disabled, skipping runtime"
            );
            // Advance the generation so a late write from the just-stopped task
            // cannot revive a status over the Disabled overview set below.
            self.shared.next_runtime_generation(&account.id).await;
            self.shared
                .set_runtime_overview(
                    &account.id,
                    AccountRuntimeOverview {
                        status: AccountStatus::Disabled,
                        push: PushStatus::Disabled,
                        ..Default::default()
                    },
                )
                .await;
            return;
        }

        ph_info!(
            events::SUPERVISOR_ACCOUNT_RUNTIME_STARTED,
            account_id = %account.id,
            driver = ?account.driver,
            "starting account runtime"
        );

        // Each account gets a cancellation token that is a child of the supervisor
        // root, so `stop_all` cancels every account with one call while
        // `stop_account` cancels only this one (D61).
        let cancel = self.root_cancel.child_token();
        let sync_state = SyncTriggerState::new();
        // The command channel's receiver is consumed by each incarnation, so a
        // watchdog restart mints a fresh channel; the live sender lives in this
        // slot, swapped by the (re)spawn factory. `incarnation_abort` targets the
        // current incarnation for the post-deadline stop escalation only.
        let command_slot: Arc<StdMutex<Option<mpsc::Sender<RuntimeCommand>>>> =
            Arc::new(StdMutex::new(None));
        let incarnation_abort: Arc<StdMutex<Option<AbortHandle>>> = Arc::new(StdMutex::new(None));

        let spawn = build_incarnation_spawner(
            self.shared.clone(),
            account.clone(),
            sync_state.clone(),
            command_slot.clone(),
            incarnation_abort.clone(),
            splay_startup,
        );

        let first = spawn(cancel.clone());
        let monitor = tokio::spawn(run_watchdog(
            account.id.clone(),
            self.shared.clone(),
            cancel.clone(),
            spawn,
            WatchdogPolicy::production(),
            first,
        ));

        self.runtimes.write().await.insert(
            account.id.to_string(),
            ManagedRuntime {
                command_tx: command_slot,
                sync_state,
                cancel,
                incarnation_abort,
                monitor,
            },
        );
    }

    /// Stop every account runtime as teardown step (b) (D60/D61 / M20 seam),
    /// bounded by `deadline` (the M20 `SUPERVISOR_STOP_DEADLINE` phase budget).
    ///
    /// Cooperative-first: cancelling the root token trips every account's
    /// `select!` cancelled arm at once; then each watchdog is joined under the
    /// shared deadline. Only a straggler that overruns is escalated to `abort()`
    /// (of its incarnation + watchdog) — the escalation, not the primary path.
    pub async fn stop_all(&self, deadline: Duration) {
        self.root_cancel.cancel();
        let entries: Vec<(String, ManagedRuntime)> = self.runtimes.write().await.drain().collect();
        let deadline_at = tokio::time::Instant::now() + deadline;
        for (account_id, runtime) in entries {
            let ManagedRuntime {
                cancel,
                incarnation_abort,
                monitor,
                ..
            } = runtime;
            // Redundant with the root cancel, but explicit about the contract.
            cancel.cancel();
            let remaining = deadline_at.saturating_duration_since(tokio::time::Instant::now());
            join_or_escalate(&account_id, monitor, &incarnation_abort, remaining).await;
            self.shared
                .remove_gateway(&AccountId::from(account_id.as_str()))
                .await;
        }
    }

    /// Stop one account's runtime cooperatively and remove its gateway: cancel the
    /// account's token, join its watchdog under [`PER_ACCOUNT_STOP_DEADLINE`], and
    /// escalate to `abort()` only if it overruns (the same cancel→join→abort shape
    /// as [`stop_all`](Self::stop_all)).
    pub async fn stop_account(&self, account_id: &AccountId) {
        let removed = self.runtimes.write().await.remove(account_id.as_str());
        if let Some(runtime) = removed {
            ph_info!(
                events::SUPERVISOR_ACCOUNT_RUNTIME_STOPPED,
                account_id = %account_id,
                "stopping account runtime"
            );
            let ManagedRuntime {
                cancel,
                incarnation_abort,
                monitor,
                ..
            } = runtime;
            cancel.cancel();
            join_or_escalate(
                account_id.as_str(),
                monitor,
                &incarnation_abort,
                PER_ACCOUNT_STOP_DEADLINE,
            )
            .await;
        }
        self.shared.remove_gateway(account_id).await;
    }

    /// Stop the runtime and clear runtime overview state for a deleted account.
    pub async fn remove_account(&self, account_id: &AccountId) {
        ph_info!(
            events::SUPERVISOR_ACCOUNT_REMOVED,
            account_id = %account_id,
            "removing account"
        );
        self.stop_account(account_id).await;
        self.shared.next_runtime_generation(account_id).await;
        self.shared.unregister_account(account_id).await;
        self.shared
            .runtime_overviews
            .write()
            .await
            .remove(account_id.as_str());
    }

    /// Send a manual sync trigger to the account runtime and await its result.
    ///
    /// @spec docs/L1-api#sync-and-events
    pub async fn sync_account(&self, account_id: &AccountId) -> Result<usize, ServiceError> {
        self.sync_account_with_mode(account_id, SyncMode::Incremental)
            .await
    }

    /// Send a manual sync trigger with an explicit mode and await its result.
    ///
    /// @spec docs/L1-api#sync-and-events
    pub async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        mode: SyncMode,
    ) -> Result<usize, ServiceError> {
        let command_tx = self.live_command_tx(account_id).await?;
        let (reply_tx, reply_rx) = oneshot::channel();
        command_tx
            .send(RuntimeCommand::Trigger {
                trigger: SyncTrigger::Manual,
                mode,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::from(GatewayError::Unavailable(account_id.to_string())))?
    }

    /// Request a runtime sync without waiting for completion.
    ///
    /// If the account runtime is already inside a sync cycle, the trigger is
    /// coalesced into a single pending follow-up trigger instead of enqueueing
    /// another full sync. The runtime runs the follow-up cycle when the current
    /// one finishes, and a single sync drains all queued local-first operations.
    pub async fn trigger_account_sync(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
    ) -> Result<(), ServiceError> {
        // Clone the live command sender + sync-state handle, then release the
        // runtimes read lock before awaiting so a slow send cannot block the map.
        let (command_tx, sync_state) = {
            let runtimes = self.runtimes.read().await;
            let runtime = runtimes
                .get(account_id.as_str())
                .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
            let command_tx = runtime
                .command_tx
                .lock()
                .expect("command slot poisoned")
                .clone();
            (command_tx, runtime.sync_state.clone())
        };
        let command_tx =
            command_tx.ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;

        // Reserve a command-slot before checking sync state. This guarantees
        // that a trigger is never dropped if the runtime stops between our check
        // and the send, and prevents spurious channel-pressure from coalesced
        // triggers (the reserved slot is released when coalescing).
        let permit = command_tx
            .reserve()
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;

        // Claim-or-coalesce atomically: the first trigger to observe the idle
        // window CLAIMS the cycle (marks the state active under the lock, before
        // this reserved slot enqueues it); a trigger that arrives once a cycle is
        // admitted folds into the pending follow-up (drained when that sync
        // finishes) instead of enqueueing its own. Doing the claim/coalesce and
        // the pending-store under one lock (inside `claim_or_coalesce`) both
        // closes the lost-wakeup race (a trigger stranded between the drain loop
        // clearing the flag and taking `pending`) and the P5 idle-boundary race
        // (N concurrent idle-window triggers each enqueuing a redundant cycle).
        if sync_state.claim_or_coalesce(trigger.clone()).await {
            ph_debug!(
                events::SUPERVISOR_SYNC_TRIGGER_COALESCED,
                account_id = %account_id,
                trigger = trigger.as_str(),
                "sync trigger coalesced while runtime is already syncing"
            );
            drop(permit);
            return Ok(());
        }
        permit.send(RuntimeCommand::TriggerOnly { trigger });
        Ok(())
    }

    /// Request cache re-score/fetch work without waiting for completion.
    pub async fn trigger_cache_maintenance(
        &self,
        account_id: &AccountId,
        operation_id: Option<String>,
    ) -> Result<(), ServiceError> {
        let command_tx = self.live_command_tx(account_id).await?;
        command_tx
            .send(RuntimeCommand::CacheMaintenance {
                interactive_pressure: CACHE_INTERACTIVE_PRESSURE,
                operation_id,
            })
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;
        Ok(())
    }

    /// Return the number of sync cycles executed by the account runtime since
    /// it was started. Used by tests and observability to verify that bursts
    /// of local mutations do not trigger one provider sync per mutation.
    pub async fn sync_cycle_count(&self, account_id: &AccountId) -> usize {
        let runtimes = self.runtimes.read().await;
        runtimes
            .get(account_id.as_str())
            .map(|runtime| runtime.sync_state.sync_cycle_count())
            .unwrap_or(0)
    }

    /// Get the current runtime status snapshot for an account.
    pub async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.shared.runtime_overview(account_id).await
    }

    /// Return the current number of accounts known to the supervisor.
    pub fn account_count(&self) -> usize {
        self.shared.account_count.load(Ordering::SeqCst)
    }

    /// Return the live gateway for an account, if its runtime is connected.
    pub async fn gateway(&self, account_id: &AccountId) -> Result<SharedGateway, ServiceError> {
        self.shared.gateway(account_id).await
    }

    /// Attempt JMAP session discovery for an account without starting a
    /// persistent runtime.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub async fn verify_account(
        &self,
        account: &AccountSettings,
    ) -> Result<AccountVerification, ServiceError> {
        let conn = build_connection(account, &self.shared, None).await?;
        let identity = conn.gateway.fetch_identity(&account.id).await.ok();
        Ok(AccountVerification {
            ok: true,
            identity,
            push_supported: account.driver.capabilities().supports_push,
        })
    }

    /// Clone the live command sender for an account, releasing the runtimes read
    /// lock before returning. `Unavailable` if the account has no runtime, or is
    /// momentarily between watchdog restarts (the slot is empty while a fresh
    /// incarnation is being spawned).
    async fn live_command_tx(
        &self,
        account_id: &AccountId,
    ) -> Result<mpsc::Sender<RuntimeCommand>, ServiceError> {
        let slot = {
            let runtimes = self.runtimes.read().await;
            runtimes
                .get(account_id.as_str())
                .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?
                .command_tx
                .clone()
        };
        let sender = slot.lock().expect("command slot poisoned").clone();
        sender.ok_or_else(|| GatewayError::Unavailable(account_id.to_string()).into())
    }
}

/// The (re)spawn factory the watchdog calls to (re)start an account incarnation.
/// Given the account's cancellation token, it mints a fresh command channel,
/// publishes the live sender + abort handle into the shared slots, spawns the
/// runtime task, and returns its `JoinHandle`.
pub(crate) type SpawnIncarnation = Box<dyn Fn(CancellationToken) -> JoinHandle<()> + Send + Sync>;

/// Build the production incarnation spawner for an account (D61). Every restart
/// mints a fresh command channel + generation and resets the coalescing state.
fn build_incarnation_spawner(
    shared: Arc<SupervisorShared>,
    account: AccountSettings,
    sync_state: Arc<SyncTriggerState>,
    command_slot: Arc<StdMutex<Option<mpsc::Sender<RuntimeCommand>>>>,
    incarnation_abort: Arc<StdMutex<Option<AbortHandle>>>,
    splay_startup: bool,
) -> SpawnIncarnation {
    Box::new(move |cancel: CancellationToken| {
        let (command_tx, command_rx) = mpsc::channel(32);
        *command_slot.lock().expect("command slot poisoned") = Some(command_tx);
        let shared = shared.clone();
        let account = account.clone();
        let sync_state = sync_state.clone();
        let span = info_span!("supervisor.runtime", account_id = %account.id);
        let handle = tokio::spawn(
            async move {
                // Fresh incarnation: clear coalescing state a panicked cycle may
                // have left, and take a new generation so a late write from the
                // prior incarnation is dropped by the generation guard.
                sync_state.reset().await;
                let generation = shared.next_runtime_generation(&account.id).await;
                run_account_runtime(
                    shared,
                    account,
                    generation,
                    command_rx,
                    sync_state,
                    cancel,
                    splay_startup,
                )
                .await;
            }
            .instrument(span),
        );
        *incarnation_abort.lock().expect("abort slot poisoned") = Some(handle.abort_handle());
        handle
    })
}

/// The ratified watchdog policy (RFC-L2-lifecycle §7 ruling 2 / D61). Fields are
/// injectable so tests can pin the jitter and shrink the timings.
pub(crate) struct WatchdogPolicy {
    pub(crate) max_restarts: u32,
    pub(crate) healthy_reset_after: Duration,
    pub(crate) backoff: BackoffPolicy,
    /// Full-jitter source in `[0,1)`. The backoff *shape* is the one vocabulary
    /// the M9 near-end engine owns ([`BackoffPolicy`]); the watchdog supplies its
    /// own jitter because, unlike the engine, it has no host `Scheduler`.
    pub(crate) jitter: Arc<dyn Fn() -> f64 + Send + Sync>,
}

impl WatchdogPolicy {
    /// Production policy: cap 3 restarts, 60s healthy-reset window, and the M9
    /// engine's default jittered-capped backoff (base 500ms, ×2, cap 30s).
    pub(crate) fn production() -> Self {
        Self {
            max_restarts: WATCHDOG_MAX_RESTARTS,
            healthy_reset_after: WATCHDOG_HEALTHY_RESET_AFTER,
            backoff: BackoffPolicy::default(),
            jitter: Arc::new(jitter_unit),
        }
    }
}

/// A cheap, dependency-free uniform-ish value in `[0,1)` for full-jitter
/// decorrelation of restart backoff. Not cryptographic; jitter quality is
/// "Review" (D61), matching the near-end engine's own review posture.
pub(crate) fn jitter_unit() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0);
    f64::from(nanos % 1_000_000) / 1_000_000.0
}

/// Extract a human-readable message from a captured panic payload.
fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

/// The supervisor-owned watchdog for one account (D61 / audit Part C). It
/// observes the incarnation `JoinHandle` so a panic or unexpected exit is
/// surfaced (`error!`/status) instead of silently swallowed, restarts under
/// [`WatchdogPolicy`] up to the cap, then halts the account with a truthful
/// status. Returns when the account is cooperatively cancelled or halted.
pub(crate) async fn run_watchdog(
    account_id: AccountId,
    shared: Arc<SupervisorShared>,
    cancel: CancellationToken,
    spawn: SpawnIncarnation,
    policy: WatchdogPolicy,
    mut current: JoinHandle<()>,
) {
    let mut restarts: u32 = 0;
    loop {
        let started = tokio::time::Instant::now();
        let outcome = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                // Cooperative stop: let the incarnation finish its own graceful
                // exit (its `select!` cancelled arm), then we are done.
                let _ = current.await;
                return;
            }
            result = &mut current => result,
        };

        let reason = match outcome {
            Ok(()) => {
                // The task returned on its own. A cancel makes this a graceful
                // stop; otherwise the loop exited unexpectedly (a fault).
                if cancel.is_cancelled() {
                    return;
                }
                ph_warn!(
                    events::SUPERVISOR_ACCOUNT_EXITED_UNEXPECTEDLY,
                    account_id = %account_id,
                    "account runtime exited unexpectedly"
                );
                "runtime exited unexpectedly".to_string()
            }
            Err(join_error) if join_error.is_panic() => {
                let message = panic_payload_message(join_error.into_panic());
                ph_error!(
                    events::SUPERVISOR_ACCOUNT_PANICKED,
                    account_id = %account_id,
                    payload = %message,
                    "account runtime panicked"
                );
                message
            }
            Err(_join_error) => {
                // A cancelled/aborted JoinError only arises from the stop
                // escalation (which cancels first) — treat as a cooperative stop.
                return;
            }
        };

        // A sustained-healthy incarnation resets the restart budget: a fault after
        // a long run is a fresh incident, not part of a storm.
        if started.elapsed() >= policy.healthy_reset_after {
            restarts = 0;
        }
        restarts += 1;

        if restarts > policy.max_restarts {
            shared.mark_account_halted(&account_id, &reason).await;
            ph_error!(
                events::SUPERVISOR_ACCOUNT_HALTED,
                account_id = %account_id,
                max_restarts = policy.max_restarts,
                "account runtime halted after exhausting its restart budget"
            );
            return;
        }

        shared
            .mark_account_faulted(&account_id, restarts, &reason)
            .await;
        let delay = policy.backoff.delay_for(restarts - 1, (policy.jitter)());
        ph_warn!(
            events::SUPERVISOR_ACCOUNT_RESTARTING,
            account_id = %account_id,
            attempt = restarts,
            max_restarts = policy.max_restarts,
            delay_ms = delay.as_millis() as u64,
            "restarting account runtime after backoff"
        );
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(delay) => {}
        }
        current = spawn(cancel.clone());
    }
}

#[cfg(test)]
impl AccountSupervisor {
    /// Build a supervisor around an existing shared context (tests only).
    pub(crate) fn from_shared_for_test(shared: Arc<SupervisorShared>) -> Self {
        Self {
            shared,
            runtimes: RwLock::new(HashMap::new()),
            root_cancel: CancellationToken::new(),
        }
    }

    /// Install a supervised account whose incarnation body is supplied by the
    /// test (so it can panic, hang, or exit at will), wired to the real watchdog
    /// + the real stop path — used to exercise `stop_all`/`stop_account`.
    pub(crate) async fn spawn_supervised_for_test(
        &self,
        account_id: AccountId,
        policy: WatchdogPolicy,
        incarnation: Arc<dyn Fn(CancellationToken) -> JoinHandle<()> + Send + Sync>,
    ) {
        let cancel = self.root_cancel.child_token();
        let sync_state = SyncTriggerState::new();
        let command_slot = Arc::new(StdMutex::new(None));
        let incarnation_abort: Arc<StdMutex<Option<AbortHandle>>> = Arc::new(StdMutex::new(None));
        let spawn: SpawnIncarnation = {
            let incarnation = incarnation.clone();
            let abort_slot = incarnation_abort.clone();
            Box::new(move |cancel| {
                let handle = incarnation(cancel);
                *abort_slot.lock().expect("abort slot poisoned") = Some(handle.abort_handle());
                handle
            })
        };
        let first = spawn(cancel.clone());
        let monitor = tokio::spawn(run_watchdog(
            account_id.clone(),
            self.shared.clone(),
            cancel.clone(),
            spawn,
            policy,
            first,
        ));
        self.runtimes.write().await.insert(
            account_id.to_string(),
            ManagedRuntime {
                command_tx: command_slot,
                sync_state,
                cancel,
                incarnation_abort,
                monitor,
            },
        );
    }
}

/// Join the account's watchdog under `deadline`; only if it overruns do we
/// ESCALATE (not the primary path) to aborting the current incarnation and then
/// the watchdog. Shared by [`AccountSupervisor::stop_all`] and
/// [`AccountSupervisor::stop_account`].
async fn join_or_escalate(
    account_id: &str,
    monitor: JoinHandle<()>,
    incarnation_abort: &Arc<StdMutex<Option<AbortHandle>>>,
    deadline: Duration,
) {
    let monitor_abort = monitor.abort_handle();
    if tokio::time::timeout(deadline, monitor).await.is_err() {
        if let Some(abort) = incarnation_abort
            .lock()
            .expect("abort slot poisoned")
            .as_ref()
        {
            abort.abort();
        }
        monitor_abort.abort();
        ph_warn!(
            events::SUPERVISOR_ACCOUNT_STOP_ESCALATED,
            account_id = %account_id,
            deadline_ms = deadline.as_millis() as u64,
            "account did not stop cooperatively within the deadline; escalated to abort"
        );
    }
}
