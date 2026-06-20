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
        let shared = self.shared.clone();
        let account = account.clone();
        let account_id = account.id.clone();
        let span = info_span!("supervisor.runtime", account_id = %account_id);
        let handle = tokio::spawn(
            async move {
                run_account_runtime(shared, account, command_rx).await;
            }
            .instrument(span),
        );
        self.runtimes.write().await.insert(
            account_id.to_string(),
            ManagedRuntime { command_tx, handle },
        );
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
    pub async fn trigger_account_sync(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
    ) -> Result<(), ServiceError> {
        let runtimes = self.runtimes.read().await;
        let runtime = runtimes
            .get(account_id.as_str())
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
        runtime
            .command_tx
            .send(RuntimeCommand::TriggerOnly { trigger })
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;
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

    /// Get the current runtime status snapshot for an account.
    pub async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.shared.runtime_overview(account_id).await
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
        let conn = build_connection(account, &self.shared).await?;
        let identity = conn.gateway.fetch_identity(&account.id).await.ok();
        Ok(AccountVerification {
            ok: true,
            identity,
            push_supported: account.driver.capabilities().supports_push,
        })
    }
}
