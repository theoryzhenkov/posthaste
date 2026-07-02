//! The authority server's command apply path (D29 split of
//! `authority_server.rs`): the typed message commands, account/config
//! mutations, the up-channel `forward_mutation_for` (per-runtime idempotency +
//! settlement emit), and [`AuthorityServer::apply_operation`] — the one
//! exhaustive dispatch from the typed [`MailOperation`] vocabulary (D8/D34).
//! Verbatim moves from `authority_server.rs` except the dispatch, rewritten
//! typed at M5.
use super::*;

impl AuthorityServer {
    /// Phase 2: on a confirmed forward action whose `context` carries a
    /// `revStep`, append the reversible-op step to `rev_log` + emit
    /// [`EVENT_TOPIC_REV_LOG_APPENDED`] so the `RevLog` synced view re-serves
    /// the log + cursor. Best-effort — a store failure is logged (the mutation
    /// already applied; the client can retry the append by re-sending the step).
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    fn append_rev_log_step_if_present(
        &self,
        account_id: &str,
        message_id: &str,
        context: &Option<serde_json::Value>,
    ) {
        let Some(rev_step) = context.as_ref().and_then(|c| c.get("revStep")) else {
            return;
        };
        let Ok(rev_step) = serde_json::from_value::<RevStepInput>(rev_step.clone()) else {
            ph_warn!(
                events::REV_LOG_APPEND_FAILED,
                account_id = %account_id,
                "rev_log step payload in mutation context was invalid; skipping append"
            );
            return;
        };
        let account = AccountId(account_id.to_string());
        let created_at = now_iso8601().unwrap_or_default();
        match self.store.append_rev_log_step(
            &account,
            &rev_step.step_id,
            message_id,
            account_id,
            &rev_step.diff,
            &created_at,
        ) {
            Ok(_) => {
                let _ = self.event_sender.send(DomainEvent {
                    seq: 0,
                    account_id: account.clone(),
                    topic: EVENT_TOPIC_REV_LOG_APPENDED.to_string(),
                    occurred_at: created_at,
                    mailbox_id: None,
                    message_id: Some(MessageId(message_id.to_string())),
                    payload: serde_json::json!({ "stepId": rev_step.step_id }),
                });
            }
            Err(error) => ph_warn!(
                events::REV_LOG_APPEND_FAILED,
                account_id = %account_id,
                step_id = %rev_step.step_id,
                error = %error,
                "rev_log append failed; the mutation applied but is not yet undoable"
            ),
        }
    }

    /// Phase 2: apply a `revCursor` control mutation — validate the referenced
    /// steps exist in `rev_log`, then apply the idempotent cursor assignment +
    /// emit `rev_log.appended` so the `RevLog` synced view re-serves the
    /// cursor. Re-delivery is a no-op (the assignment is idempotent).
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    fn apply_rev_cursor(&self, args: &RevCursorArgs) -> Result<CommandAck, RuntimeError> {
        let account = AccountId(args.account_id.clone());
        // Validate: cursor_step_id (if Some) + redo_tail steps must exist.
        let snapshot = self
            .store
            .rev_log_snapshot(&account)
            .map_err(|e| RuntimeError::internal(e.to_string(), None))?;
        if let Some(cursor) = &args.cursor_step_id {
            if !snapshot.steps.iter().any(|s| &s.step_id == cursor) {
                return Err(RuntimeError::invalid_mutation(format!(
                    "revCursor cursor_step_id {cursor} is not in the rev_log"
                )));
            }
        }
        for step in &args.redo_tail {
            if !snapshot.steps.iter().any(|s| &s.step_id == step) {
                return Err(RuntimeError::invalid_mutation(format!(
                    "revCursor redo_tail step {step} is not in the rev_log"
                )));
            }
        }
        // Apply the idempotent cursor assignment.
        self.store
            .set_rev_cursor(&account, args.cursor_step_id.as_deref(), &args.redo_tail)
            .map_err(|e| RuntimeError::internal(e.to_string(), None))?;
        // Emit the recompute trigger (same topic as append).
        let _ = self.event_sender.send(DomainEvent {
            seq: 0,
            account_id: account,
            topic: EVENT_TOPIC_REV_LOG_APPENDED.to_string(),
            occurred_at: now_iso8601().unwrap_or_default(),
            mailbox_id: None,
            message_id: None,
            payload: serde_json::json!({
                "cursorStepId": args.cursor_step_id.clone(),
                "redoTail": args.redo_tail.clone(),
            }),
        });
        Ok(CommandAck { events: Vec::new() })
    }

    /// Nudge the account to sync so just-enqueued outbox operations flush
    /// promptly. Best-effort: if the account is offline the op stays queued and
    /// flushes on the next connectivity window.
    pub(crate) async fn trigger_outbox_flush(&self, account_id: &AccountId) {
        if let Err(error) = self
            .live_accounts
            .trigger_account_sync(account_id, SyncTrigger::Manual)
            .await
        {
            ph_warn!(
                events::OUTBOX_FOLLOWUP_SYNC_TRIGGER_FAILED,
                source_id = %account_id,
                error = %error,
                "outbox operation enqueued but follow-up sync trigger failed"
            );
        }
    }

    pub(crate) async fn set_keywords(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .set_keywords(&account_id, &message_id, &command)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn add_to_mailbox(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .add_to_mailbox(&account_id, &message_id, &command)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn remove_from_mailbox(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .remove_from_mailbox(&account_id, &message_id, &command)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn replace_mailboxes(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .replace_mailboxes(&account_id, &message_id, &command)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn destroy(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .destroy_message(&account_id, &message_id)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn set_mailbox_role(
        &self,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        let gateway = self.live_accounts.gateway(&account_id).await?;
        let events = self
            .service
            .set_mailbox_role(&account_id, &mailbox_id, role.as_deref(), gateway.as_ref())
            .await?;
        self.publish_events(&events);
        Ok(self.service.list_mailboxes(&account_id)?)
    }

    /// Write: queue a local-first send and nudge a flush. No live gateway is
    /// required to accept it; it flushes on the next connectivity window.
    pub(crate) async fn send_message(
        &self,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        let sender = request.from.clone();
        self.service.enqueue_send(&account_id, request)?;
        if let Some(sender) = &sender {
            if let Err(error) = self.store.remember_sender_address(&account_id, sender) {
                ph_warn!(
                    events::SEND_SENDER_CACHE_UPDATE_FAILED,
                    source_id = %account_id,
                    sender = %sender.email,
                    error = %error,
                    "send accepted but sender address cache update failed"
                );
            }
        }
        self.trigger_outbox_flush(&account_id).await;
        Ok(())
    }

    /// Write: save (create or update) a draft and nudge a flush.
    pub(crate) async fn save_draft(
        &self,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        let operation = self.service.save_draft(&account_id, draft_id, request)?;
        self.trigger_outbox_flush(&account_id).await;
        Ok(operation)
    }

    /// Write: delete a draft and nudge a flush.
    pub(crate) async fn delete_draft(
        &self,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        let operation = self.service.delete_draft(&account_id, draft_id)?;
        self.trigger_outbox_flush(&account_id).await;
        Ok(operation)
    }

    /// Write: discard a pending outbox operation.
    pub(crate) fn discard_operation(&self, operation_id: OperationId) -> Result<(), RuntimeError> {
        self.service.discard_operation(&operation_id)?;
        Ok(())
    }

    /// Write: re-arm a failed outbox operation to pending and nudge a flush.
    pub(crate) async fn retry_operation(
        &self,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.service.retry_operation(&operation_id)?;
        self.trigger_outbox_flush(&account_id).await;
        Ok(())
    }

    /// Write: drive an explicit account sync, returning the number of changes.
    pub(crate) async fn sync_account(
        &self,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        Ok(self
            .live_accounts
            .sync_account_with_mode(&account_id, mode)
            .await?)
    }

    // ===== Account + config mutations (account_mutations authority) =====

    pub(crate) fn patch_app_settings(
        &self,
        mutation: PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        self.account_mutations()?.patch_app_settings(mutation)
    }

    pub(crate) fn preview_automation_rule(
        &self,
        mutation: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError> {
        self.account_mutations()?.preview_automation_rule(mutation)
    }

    pub(crate) fn create_smart_mailbox(
        &self,
        mutation: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.account_mutations()?.create_smart_mailbox(mutation)
    }

    pub(crate) fn patch_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
        mutation: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.account_mutations()?
            .patch_smart_mailbox(smart_mailbox_id, mutation)
    }

    pub(crate) fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.account_mutations()?
            .delete_smart_mailbox(smart_mailbox_id)
    }

    pub(crate) fn reset_default_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.account_mutations()?.reset_default_smart_mailboxes()
    }

    pub(crate) async fn create_account(
        &self,
        mutation: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.account_mutations()?.create_account(mutation).await
    }

    pub(crate) async fn patch_account(
        &self,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.account_mutations()?
            .patch_account(account_id, mutation)
            .await
    }

    pub(crate) async fn delete_account(&self, account_id: AccountId) -> Result<(), RuntimeError> {
        self.account_mutations()?.delete_account(account_id).await
    }

    pub(crate) async fn verify_account(
        &self,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.account_mutations()?.verify_account(account_id).await
    }

    pub(crate) async fn set_account_enabled(
        &self,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        self.account_mutations()?
            .set_account_enabled(account_id, enabled)
            .await
    }

    pub(crate) async fn reload_config(&self) -> Result<(), RuntimeError> {
        self.account_mutations()?.reload_config().await
    }

    /// Resolve the account's mailbox for `role` and replace the message's
    /// mailbox membership with it. Role resolution is authority-server-owned so the
    /// runtime forwards role intent without looking up role mailboxes.
    ///
    /// @spec docs/state/mail/L1#message-change-assertions
    pub(crate) async fn move_message_to_role(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        role: String,
    ) -> Result<CommandAck, RuntimeError> {
        let mailbox = self
            .service
            .list_mailboxes(&account_id)?
            .into_iter()
            .find(|mailbox| mailbox.role.as_deref() == Some(role.as_str()))
            .ok_or_else(|| {
                RuntimeError::invalid_mutation(format!("account has no mailbox with role '{role}'"))
            })?;
        self.replace_mailboxes(
            account_id,
            message_id,
            ReplaceMailboxesCommand {
                mailbox_ids: vec![mailbox.id],
            },
        )
        .await
    }

    /// `message.snooze`: move to the Snoozed mailbox (the one with the `snooze`
    /// role) + record the return time. Reuses `move_message_to_role` for the
    /// provider move; the move's `replace_mailboxes_tx` invariant clears any
    /// prior snooze row, then we insert the new one. Rejects if no mailbox has
    /// the `snooze` role (the user must designate one via the role switch).
    /// @spec docs/eph/DESIGN-L2-snooze
    pub(crate) async fn snooze_message(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        until: i64,
    ) -> Result<CommandAck, RuntimeError> {
        let ack = self
            .move_message_to_role(account_id.clone(), message_id.clone(), "snooze".to_string())
            .await?;
        self.store
            .insert_snooze(&account_id, &message_id, until)
            .map_err(store_error_to_runtime_error)?;
        Ok(ack)
    }

    /// `message.unsnooze`: move a snoozed message back to the Inbox. The store
    /// invariant (`replace_mailboxes_tx` clears the snooze row when a message
    /// leaves the Snoozed mailbox) handles the return-time cleanup.
    /// @spec docs/eph/DESIGN-L2-snooze
    pub(crate) async fn unsnooze_message(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandAck, RuntimeError> {
        self.move_message_to_role(account_id, message_id, "inbox".to_string())
            .await
    }

    /// Up-channel for a (possibly remote) runtime: dedup by
    /// `(AuthorityServerLinkId, ClientMutationId)`, then apply the named mutation and return
    /// a receipt carrying the authority server's `RuntimeMutationId` for the confirmation
    /// join. A retried mutation resolves to its stored record, never a second
    /// application (`per-runtime-idempotency`). The co-located runtime passes a
    /// real minted id (it is runtime #1 of X, X=1 in-process — no single-runtime
    /// special case); a remote runtime's id is derived from its credential.
    ///
    /// @spec docs/replication/authority-server-link/L1#3-the-backendapi-contract
    pub(crate) async fn forward_mutation_for(
        &self,
        runtime_id: &AuthorityServerLinkId,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        match self.runtimes.accept(
            runtime_id,
            &mutation.client_mutation_id,
            mutation.operation.name(),
        ) {
            ForwardAcceptance::Existing(receipt) => Ok(receipt),
            // D47: a retried permanent rejection re-observes the same verdict and
            // never re-executes (the old path always re-executed — the AS-seam
            // half of D47's fix).
            ForwardAcceptance::Rejected(error) => Err(RuntimeError(error)),
            ForwardAcceptance::New { runtime_mutation_id } => {
                let ack = match self.apply_operation(&mutation).await {
                    Ok(ack) => ack,
                    Err(error) => {
                        // The mutation did not apply (atomic). Split by D47
                        // terminal class from the error's `retryable` flag: a
                        // transient failure CLEARS the entry so a retry
                        // re-executes; a permanent rejection is KEPT so a retry
                        // re-observes it. No Settlement either way — the near node
                        // learns of the failure from the up-channel error and
                        // cannot match a Settlement it never received a receipt for.
                        if error.envelope().retryable {
                            self.runtimes
                                .settle_failed(runtime_id, &mutation.client_mutation_id);
                        } else {
                            self.runtimes.settle_rejected(
                                runtime_id,
                                &mutation.client_mutation_id,
                                error.envelope().clone(),
                            );
                        }
                        return Err(error);
                    }
                };
                let output = serde_json::to_value(&ack).map_err(|error| {
                    RuntimeError::internal(
                        format!("failed to serialize mutation output: {error}"),
                        None,
                    )
                })?;
                // Route the per-mutation confirmation onto the originating
                // runtime's down-stream only (`settlement-routed-to-origin-runtime`):
                // never broadcast — a Settlement names one runtime's mutation. The
                // frame is recorded into the backlog at emission (D49 [0]); its
                // replay seq is the ack target D48 uses to reclaim the kept dedup
                // record once the runtime's resume cursor passes it.
                let settlement_seq = self.runtimes.emit_settlement(
                    runtime_id,
                    AuthorityServerFrame::Settlement {
                        mutation_id: MutationId(runtime_mutation_id.as_str().to_string()),
                        outcome: WireSettlementOutcome::Confirmed,
                    },
                );
                self.runtimes.settle_confirmed(
                    runtime_id,
                    &mutation.client_mutation_id,
                    output.clone(),
                    settlement_seq,
                );
                Ok(MutationReceipt {
                    runtime_mutation_id: Some(runtime_mutation_id),
                    client_mutation_id: mutation.client_mutation_id,
                    name: mutation.operation.name().to_string(),
                    state: MutationSettlementState::Accepted,
                    error: None,
                    output,
                })
            }
        }
    }

    /// Apply one typed operation — the authority server's up-channel handler and
    /// the one exhaustive dispatch from the [`MailOperation`] vocabulary to the
    /// typed commands ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)).
    /// The operation arrives already typed (parsed once at the wire edge, D8);
    /// there is no string lookup and no per-arm arg re-parse. The runtime keeps
    /// the link/undo/scope concerns around this call; this node only applies
    /// the effect and returns the resulting events.
    ///
    /// @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
    pub(crate) async fn apply_operation(
        &self,
        request: &MutationRequest,
    ) -> Result<CommandAck, RuntimeError> {
        let operation = &request.operation;
        // Phase 2: `revCursor` is a control operation (not a message mutation) —
        // it targets no message and appends no rev-log step.
        if let MailOperation::RevCursor(args) = operation {
            return self.apply_rev_cursor(args);
        }
        let account = AccountId(operation.account_id().to_string());
        let message = operation
            .message_id()
            .map(|id| MessageId(id.to_string()))
            .ok_or_else(|| {
                RuntimeError::internal("message operation without a message target", None)
            })?;
        let ack = match operation.clone() {
            MailOperation::SetKeywords(args) => {
                self.set_keywords(account.clone(), message.clone(), args.command)
                    .await
            }
            MailOperation::SetReadState(args) => {
                self.set_keywords(
                    account.clone(),
                    message.clone(),
                    keyword_toggle("$seen", args.read),
                )
                .await
            }
            MailOperation::SetFlaggedState(args) => {
                self.set_keywords(
                    account.clone(),
                    message.clone(),
                    keyword_toggle("$flagged", args.flagged),
                )
                .await
            }
            MailOperation::SetUserTags(args) => {
                self.set_keywords(
                    account.clone(),
                    message.clone(),
                    SetKeywordsCommand {
                        add: args.add,
                        remove: args.remove,
                    },
                )
                .await
            }
            MailOperation::MoveToMailbox(args) => {
                self.replace_mailboxes(
                    account.clone(),
                    message.clone(),
                    ReplaceMailboxesCommand {
                        mailbox_ids: vec![MailboxId(args.mailbox_id)],
                    },
                )
                .await
            }
            MailOperation::ReplaceMailboxes(args) => {
                self.replace_mailboxes(
                    account.clone(),
                    message.clone(),
                    ReplaceMailboxesCommand {
                        mailbox_ids: args.mailbox_ids.into_iter().map(MailboxId).collect(),
                    },
                )
                .await
            }
            MailOperation::AddToMailbox(args) => {
                self.add_to_mailbox(
                    account.clone(),
                    message.clone(),
                    AddToMailboxCommand {
                        mailbox_id: MailboxId(args.mailbox_id),
                    },
                )
                .await
            }
            MailOperation::RemoveFromMailbox(args) => {
                self.remove_from_mailbox(
                    account.clone(),
                    message.clone(),
                    RemoveFromMailboxCommand {
                        mailbox_id: MailboxId(args.mailbox_id),
                    },
                )
                .await
            }
            MailOperation::MoveToRole(args) => {
                self.move_message_to_role(account.clone(), message.clone(), args.role)
                    .await
            }
            MailOperation::Snooze(args) => {
                self.snooze_message(account.clone(), message.clone(), args.until)
                    .await
            }
            MailOperation::Unsnooze(_) => {
                self.unsnooze_message(account.clone(), message.clone()).await
            }
            MailOperation::Destroy(_) => {
                self.destroy(account.clone(), message.clone()).await
            }
            // `message.applyDiff` is the undo/redo vehicle — see `apply_diff`.
            MailOperation::ApplyDiff(args) => {
                self.apply_diff(account.clone(), message.clone(), args.diff).await
            }
            MailOperation::RevCursor(_) => unreachable!("handled above"),
        }?;
        // Phase 2: append the reversible-op step on a confirmed forward action
        // whose context carries a `revStep`, + emit the recompute trigger so the
        // `RevLog` synced view re-serves the log + cursor.
        self.append_rev_log_step_if_present(account.as_str(), message.as_str(), &request.context);
        Ok(ack)
    }

    /// `message.applyDiff`: apply the invertible diff as the equivalent keyword
    /// add/remove plus a mailbox add/remove. Keywords are a delta
    /// (`SetKeywordsCommand`); mailboxes are computed against the current
    /// membership and applied as one replace. The far-node mirror of the
    /// near-node `ApplyDiff` assertion fold.
    async fn apply_diff(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        diff: MessageChangeDiff,
    ) -> Result<CommandAck, RuntimeError> {
        let mut events = Vec::new();
        if !diff.keywords.added.is_empty() || !diff.keywords.removed.is_empty() {
            let ack = self
                .set_keywords(
                    account_id.clone(),
                    message_id.clone(),
                    SetKeywordsCommand {
                        add: diff.keywords.added,
                        remove: diff.keywords.removed,
                    },
                )
                .await?;
            events.extend(ack.events);
        }
        if !diff.mailboxes.added.is_empty() || !diff.mailboxes.removed.is_empty() {
            let mut mailbox_ids: Vec<MailboxId> = self
                .current_summary(&account_id, &message_id)
                .await?
                .map(|summary| summary.mailbox_ids)
                .unwrap_or_default();
            for added in &diff.mailboxes.added {
                let id = MailboxId(added.clone());
                if !mailbox_ids.contains(&id) {
                    mailbox_ids.push(id);
                }
            }
            mailbox_ids.retain(|id| !diff.mailboxes.removed.iter().any(|r| r == id.as_str()));
            let ack = self
                .replace_mailboxes(account_id, message_id, ReplaceMailboxesCommand { mailbox_ids })
                .await?;
            events.extend(ack.events);
        }
        Ok(CommandAck { events })
    }
}
