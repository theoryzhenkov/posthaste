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
                known_accounts: RwLock::new(HashSet::new()),
                account_count: AtomicUsize::new(0),
                cache_resources: Mutex::new(CacheResourceGovernor::new(
                    Instant::now(),
                    CacheResourcePolicy::default(),
                )),
                poll_interval,
            }),
            runtimes: RwLock::new(HashMap::new()),
        }
    }

    /// Start (or restart) the async runtime for an account. Stops any
    /// existing runtime first. Disabled accounts get a `Disabled` status
    /// without spawning a task.
    pub async fn start_account(&self, account: &AccountSettings) {
        self.stop_account(&account.id).await;
        self.shared.register_account(&account.id).await;
        let generation = self.shared.next_runtime_generation(&account.id).await;
        if !account.enabled {
            ph_info!(
                events::SUPERVISOR_ACCOUNT_DISABLED,
                account_id = %account.id,
                "account disabled, skipping runtime"
            );
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
        let (command_tx, command_rx) = mpsc::channel(32);
        let sync_state = SyncTriggerState::new();
        let shared = self.shared.clone();
        let account = account.clone();
        let account_id = account.id.clone();
        let runtime_sync_state = sync_state.clone();
        let span = info_span!("supervisor.runtime", account_id = %account_id);
        let handle = tokio::spawn(
            async move {
                run_account_runtime(shared, account, generation, command_rx, runtime_sync_state)
                    .await;
            }
            .instrument(span),
        );
        self.runtimes.write().await.insert(
            account_id.to_string(),
            ManagedRuntime {
                command_tx,
                handle,
                sync_state,
            },
        );
    }

    /// Stop every account runtime as teardown step (b) (D60/D61 / M20 seam).
    ///
    /// M21: this is a placeholder that hard-`abort()`s each account task (the only
    /// stop mechanism that exists today, via [`stop_account`](Self::stop_account)).
    /// M21 replaces the internals with cooperative cancellation (a per-account
    /// token arm in the `select!` loop), a join under the shared deadline, and a
    /// panic-surfacing watchdog — behind this same method so the composition
    /// root's `ShutdownSequence` does not change. Bounded by the sequence's
    /// supervisor-stop deadline.
    pub async fn stop_all(&self) {
        let account_ids: Vec<String> = self.runtimes.read().await.keys().cloned().collect();
        for account_id in account_ids {
            // M21: cooperative cancel + join replaces this bare abort.
            self.stop_account(&AccountId::from(account_id.as_str())).await;
        }
    }

    /// Stop the runtime task and remove the gateway for an account.
    pub async fn stop_account(&self, account_id: &AccountId) {
        let removed = self.runtimes.write().await.remove(account_id.as_str());
        if let Some(runtime) = removed {
            ph_info!(
                events::SUPERVISOR_ACCOUNT_RUNTIME_STOPPED,
                account_id = %account_id,
                "stopping account runtime"
            );
            runtime.handle.abort();
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
        let runtimes = self.runtimes.read().await;
        let runtime = runtimes
            .get(account_id.as_str())
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        runtime
            .command_tx
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
        let runtimes = self.runtimes.read().await;
        let runtime = runtimes
            .get(account_id.as_str())
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;

        // Reserve a command-slot before checking sync state. This guarantees
        // that a trigger is never dropped if the runtime stops between our check
        // and the send, and prevents spurious channel-pressure from coalesced
        // triggers (the reserved slot is released when coalescing).
        let permit = runtime
            .command_tx
            .reserve()
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;

        // Coalesce-or-claim atomically: if a sync is in flight the trigger is
        // folded into the pending follow-up (drained when that sync finishes);
        // otherwise the reserved slot enqueues a fresh cycle. Doing the check and
        // the pending-store under one lock (inside `coalesce_if_syncing`) closes
        // the lost-wakeup race that previously stranded a trigger between the
        // drain loop clearing `syncing` and taking `pending`.
        if runtime
            .sync_state
            .coalesce_if_syncing(trigger.clone())
            .await
        {
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
        let runtimes = self.runtimes.read().await;
        let runtime = runtimes
            .get(account_id.as_str())
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
        runtime
            .command_tx
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
}
