//! `LocalAuthorityServer`: the in-process implementation of the far-node trait
//! pair ([`AuthorityServerApi`] + [`AuthorityServerLink`], D33).
//!
//! The default ([replication authority-server-link L2 §2](../replication/authority-server-link/L2.md)):
//! direct calls to a co-located [`AuthorityServer`] far node, zero serialization, instant
//! confirmation — byte-for-byte the pre-link behavior (`colocated-unchanged`). The
//! remote counterpart (`RemoteAuthorityServer`) lives in `posthaste-runtime`, the near node.
//!
//! @spec docs/replication/authority-server-link/L2#2-backendapi-implementations-localbackend-remotebackend

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use posthaste_domain_model::{
    AccountId, AccountOverview, AppSettings, CachedSenderAddress, CommandAck, ConversationId,
    ConversationView, DomainEvent, DraftContent, EventFilter, Identity, MailboxId, MailboxSummary,
    MessageDetail, MessageId, MessageSummary, Operation, OperationId, ReplyContext,
    RevLogSnapshot, SendMessageRequest, SmartMailbox, SmartMailboxId, SmartMailboxSummary,
    SyncMode, TagSummary, EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_authority_server_link::{
    AuthorityServerApi, AuthorityServerFrame, AuthorityServerLink, AuthorityServerLinkId,
    BaseAssertion, BaseUpdate, DownStream, LinkCoverage, MailCommandRequest, SequencedFrame,
};
use posthaste_link_far_end::Resume;
use tracing::warn;
use posthaste_replica_core::MessageFoldState;
use posthaste_contract_core::{
    AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation, MailOperation,
    MailQueryPage, MailQueryRequest, MessageResourceKind, MutationReceipt, MutationRequest,
    PatchAccountMutation, PatchAppSettingsMutation, PatchSmartMailboxMutation, RuntimeAccountList,
    RuntimeError, RuntimeResourceBytes,
};

use crate::authority_server::AuthorityServer;

pub(crate) struct LocalAuthorityServer {
    authority_server: Arc<AuthorityServer>,
    /// The co-located runtime's id — minted once at construction. This is just
    /// runtime #1 of X (X=1 in-process), not a single-runtime special case: the
    /// same `forward_mutation_for` / `subscribe_for` path serves it as any
    /// remote runtime.
    runtime_id: AuthorityServerLinkId,
}

impl LocalAuthorityServer {
    pub(crate) fn new(authority_server: Arc<AuthorityServer>) -> Self {
        Self {
            authority_server,
            runtime_id: AuthorityServerLinkId::new(uuid::Uuid::new_v4().to_string()),
        }
    }
}

/// Map one authoritative event to a `AuthorityServerFrame::Base` (its message's complete
/// fold state), or `None` if the event yields no assertion. Reads the current
/// state from the authority server so the assertion carries the *complete* post-state
/// ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)).
pub(crate) fn base_frame_from_event(authority_server: &AuthorityServer, event: &DomainEvent) -> Option<AuthorityServerFrame> {
    let current = event
        .message_id
        .as_ref()
        .and_then(|message_id| authority_server.current_fold_state(&event.account_id, message_id).ok().flatten());
    message_event_to_assertion(event, current)
        .map(|assertion| AuthorityServerFrame::Base { assertions: vec![assertion] })
}

/// How a message domain event names its message's authoritative base change —
/// the pure half of the down-channel mapping, factored out so it is testable
/// without a running store. `current` is the message's complete fold state read
/// from the authority server (`None` when the message is gone); a `deleted` event maps to
/// a removal regardless. Non-message events and events without a message id
/// produce no assertion.
pub(crate) fn message_event_to_assertion(
    event: &DomainEvent,
    current: Option<MessageFoldState>,
) -> Option<BaseAssertion> {
    if event.topic != EVENT_TOPIC_MESSAGE_UPDATED {
        return None;
    }
    let message_id = event.message_id.as_ref()?.as_str().to_string();
    let account_id = event.account_id.as_str().to_string();
    let deleted = event
        .payload
        .get("deleted")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if deleted {
        return Some(BaseAssertion {
            account_id,
            message_id,
            update: BaseUpdate::Removed,
        });
    }
    // A present message asserts its complete current state. If the read found
    // nothing (a race with a concurrent removal), treat it as removed.
    Some(BaseAssertion {
        account_id,
        message_id,
        update: match current {
            Some(state) => BaseUpdate::Present(state),
            None => BaseUpdate::Removed,
        },
    })
}

#[async_trait]
impl AuthorityServerLink for LocalAuthorityServer {
    /// Up-channel: forward the named mutation to the co-located authority server under
    /// this `LocalAuthorityServer`'s minted `AuthorityServerLinkId` (runtime #1 of X). Dedup and
    /// `RuntimeMutationId` assignment live in `Authority server::forward_mutation_for`.
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.authority_server
            .forward_mutation_for(&self.runtime_id, mutation)
            .await
    }

    /// Up-channel, runtime-aware: a remote runtime (via `link_router`) forwards
    /// under its credential-derived `AuthorityServerLinkId`; the co-located path uses
    /// [`forward_mutation`](Self::forward_mutation) with this node's minted id.
    /// Both reach `Authority server::forward_mutation_for`.
    async fn forward_mutation_for(
        &self,
        runtime_id: &AuthorityServerLinkId,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.authority_server.forward_mutation_for(runtime_id, mutation).await
    }

    /// Down-channel: the ordered stream of authoritative base assertions. Each
    /// `message.updated` event becomes a complete [`BaseAssertion`] over its
    /// message (the far node reads the message's current summary to author the
    /// whole-message state); a `deleted` event becomes a removal. Non-message
    /// events are filtered out.
    ///
    /// In-process the up-channel confirms synchronously (the receipt returns
    /// after the effect is applied), so confirmation is carried by
    /// `forward_mutation`'s receipt rather than as a separate `Settlement`
    /// frame — those matter when the channels are decoupled (the remote
    /// transport, W3). The near node still rebases its base cache on these
    /// assertions; a co-located runtime that derives views from the cache is the
    /// W3-paired step (in-process the cache equals the store, so the view read
    /// path is unchanged today, keeping `colocated-unchanged`).
    async fn subscribe(
        &self,
        _coverage: LinkCoverage,
        after_seq: Option<u64>,
    ) -> Result<DownStream, RuntimeError> {
        Ok(self.build_down_stream(&self.runtime_id, after_seq))
    }

    /// Down-channel, runtime-aware: a remote runtime (via `link_router`)
    /// subscribes under its credential-derived `AuthorityServerLinkId`; the co-located path
    /// uses [`subscribe`](Self::subscribe) with this node's minted id. Both merge
    /// the broadcast `Base` with this runtime's routed `Settlement`s.
    async fn subscribe_for(
        &self,
        runtime_id: &AuthorityServerLinkId,
        _coverage: LinkCoverage,
        after_seq: Option<u64>,
    ) -> Result<DownStream, RuntimeError> {
        Ok(self.build_down_stream(runtime_id, after_seq))
    }

    async fn discard_operation(&self, operation_id: OperationId) -> Result<(), RuntimeError> {
        self.authority_server.discard_operation(operation_id)
    }

    async fn retry_operation(
        &self,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.authority_server.retry_operation(account_id, operation_id).await
    }
}

#[async_trait]
impl AuthorityServerApi for LocalAuthorityServer {
    /// Direct-apply a mail operation (D34): project it onto its typed command
    /// via the shared bridge ([`MailCommandRequest::from_operation`], which also
    /// rejects replica-only operations) and dispatch to the co-located far
    /// node's command surface.
    async fn apply(&self, op: MailOperation) -> Result<CommandAck, RuntimeError> {
        match MailCommandRequest::from_operation(op)? {
            MailCommandRequest::SetKeywords(request) => {
                self.authority_server
                    .set_keywords(request.account_id, request.message_id, request.command)
                    .await
            }
            MailCommandRequest::AddToMailbox(request) => {
                self.authority_server
                    .add_to_mailbox(request.account_id, request.message_id, request.command)
                    .await
            }
            MailCommandRequest::RemoveFromMailbox(request) => {
                self.authority_server
                    .remove_from_mailbox(request.account_id, request.message_id, request.command)
                    .await
            }
            MailCommandRequest::ReplaceMailboxes(request) => {
                self.authority_server
                    .replace_mailboxes(request.account_id, request.message_id, request.command)
                    .await
            }
            MailCommandRequest::Destroy(request) => {
                self.authority_server
                    .destroy(request.account_id, request.message_id)
                    .await
            }
        }
    }

    /// Read channel: serve the co-located authority server's query computation directly.
    /// This is what a remote runtime reads through to (via `link_router`).
    async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.authority_server.query_mail_page(request).await
    }

    async fn current_summary(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.authority_server.current_summary(&account_id, &message_id).await
    }

    async fn message_detail(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.authority_server.message_detail(&account_id, &message_id)
    }

    async fn conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.authority_server.conversation(&conversation_id)
    }

    async fn account_count(&self) -> Result<Option<usize>, RuntimeError> {
        Ok(self.authority_server.account_count())
    }

    async fn rev_log_snapshot(
        &self,
        account_id: AccountId,
    ) -> Result<RevLogSnapshot, RuntimeError> {
        self.authority_server.rev_log_snapshot(&account_id)
    }

    async fn list_accounts(&self) -> Result<RuntimeAccountList, RuntimeError> {
        self.authority_server.list_accounts().await
    }

    async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AccountOverview>, RuntimeError> {
        self.authority_server.get_account(account_id).await
    }

    async fn app_settings(&self) -> Result<AppSettings, RuntimeError> {
        self.authority_server.app_settings()
    }

    async fn resolve_account_scope(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        self.authority_server.resolve_account_scope(scope)
    }

    async fn list_mailboxes(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError> {
        self.authority_server.list_mailboxes(scope)
    }

    async fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.authority_server.list_smart_mailboxes()
    }

    async fn get_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.authority_server.get_smart_mailbox(smart_mailbox_id)
    }

    async fn list_tags(&self, scope: AccountScopeRequest) -> Result<Vec<TagSummary>, RuntimeError> {
        self.authority_server.list_tags(scope)
    }

    async fn get_identity(&self, account_id: AccountId) -> Result<Identity, RuntimeError> {
        self.authority_server.get_identity(account_id).await
    }

    async fn get_reply_context(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError> {
        self.authority_server.get_reply_context(account_id, message_id).await
    }

    async fn list_sender_addresses(&self) -> Result<Vec<CachedSenderAddress>, RuntimeError> {
        self.authority_server.list_sender_addresses()
    }

    async fn list_pending_operations(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        self.authority_server.list_pending_operations(account_id)
    }

    async fn replay_events(&self, filter: EventFilter) -> Result<Vec<DomainEvent>, RuntimeError> {
        self.authority_server.replay_events(filter)
    }

    async fn get_draft_content(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError> {
        self.authority_server.get_draft_content(account_id, message_id).await
    }

    async fn get_message_resource(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError> {
        self.authority_server
            .get_message_resource(account_id, message_id, kind)
            .await
    }

    async fn set_mailbox_role(
        &self,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        self.authority_server
            .set_mailbox_role(account_id, mailbox_id, role)
            .await
    }

    async fn send_message(
        &self,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        self.authority_server.send_message(account_id, request).await
    }

    async fn save_draft(
        &self,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        self.authority_server.save_draft(account_id, draft_id, request).await
    }

    async fn delete_draft(
        &self,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        self.authority_server.delete_draft(account_id, draft_id).await
    }

    async fn sync_account(
        &self,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        self.authority_server.sync_account(account_id, mode).await
    }

    async fn patch_app_settings(
        &self,
        mutation: PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        self.authority_server.patch_app_settings(mutation)
    }

    async fn preview_automation_rule(
        &self,
        mutation: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError> {
        self.authority_server.preview_automation_rule(mutation)
    }

    async fn create_smart_mailbox(
        &self,
        mutation: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.authority_server.create_smart_mailbox(mutation)
    }

    async fn patch_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
        mutation: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.authority_server.patch_smart_mailbox(smart_mailbox_id, mutation)
    }

    async fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.authority_server.delete_smart_mailbox(smart_mailbox_id)
    }

    async fn reset_default_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.authority_server.reset_default_smart_mailboxes()
    }

    async fn create_account(
        &self,
        mutation: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.authority_server.create_account(mutation).await
    }

    async fn patch_account(
        &self,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.authority_server.patch_account(account_id, mutation).await
    }

    async fn delete_account(&self, account_id: AccountId) -> Result<(), RuntimeError> {
        self.authority_server.delete_account(account_id).await
    }

    async fn verify_account(
        &self,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.authority_server.verify_account(account_id).await
    }

    async fn set_account_enabled(
        &self,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        self.authority_server.set_account_enabled(account_id, enabled).await
    }

    async fn reload_config(&self) -> Result<(), RuntimeError> {
        self.authority_server.reload_config().await
    }
}

impl LocalAuthorityServer {
    /// Build the merged down-stream for a runtime from the far-end sub-stores: a
    /// per-runtime `Base` live broadcast (recorded at emission into the backlog —
    /// D49 [0]) merged with this runtime's routed `Settlement` sink (also recorded
    /// at emission), each frame carrying the replay store's monotonic per-runtime
    /// seq (D46). `Base` is authored globally but recorded per-runtime; `Settlement`
    /// is per-runtime (`settlement-routed-to-origin-runtime`).
    ///
    /// Both channels are read behind a **monotonic cursor gate**: every seq is
    /// emitted at most once, a lagged/gapped `Base` delivery is recovered by
    /// replaying the (complete-by-construction) backlog, and a resume point that
    /// fell out of the backlog yields a `Reset` control element so the near node
    /// collapses-and-reseeds (D49 — was a log-only warning). A newer down-stream
    /// for the same runtime supersedes this one via the generation stamp (D49 [8]).
    fn build_down_stream(
        &self,
        runtime_id: &AuthorityServerLinkId,
        after_seq: Option<u64>,
    ) -> DownStream {
        let authority_server = self.authority_server.clone();
        let rid = runtime_id.clone();
        let crate::runtime_registry::DownStreamChannels {
            mut base,
            mut settlement,
            generation,
        } = authority_server.register_down_stream(&rid);
        let resume = authority_server.replay_resume(&rid, after_seq);
        Box::pin(async_stream::stream! {
            // The last seq emitted downstream — the cursor gate (dedups the two
            // channels + drives gap replay).
            let mut cursor = after_seq.unwrap_or(0);

            // Resume prelude (D46/D49).
            match resume {
                Resume::Fresh => {}
                Resume::Replay(frames) => {
                    for framed in frames {
                        cursor = framed.seq();
                        yield framed;
                    }
                }
                Resume::Collapse => {
                    // The resume point fell out of the backlog: reset the near
                    // node to current state and hand it our cursor (D49).
                    let highest = authority_server.highest_seq(&rid);
                    cursor = highest;
                    yield SequencedFrame::reset(highest);
                }
            }

            loop {
                // A newer down-stream superseded this one (D49 [8]) — terminate.
                if authority_server.current_generation(&rid) != generation {
                    break;
                }
                tokio::select! {
                    biased;
                    ev = base.recv() => match ev {
                        Ok(framed) => {
                            let seq = framed.seq();
                            // A live gap (an earlier frame was dropped by the lossy
                            // broadcast): replay the complete backlog to bridge it
                            // (D49 [0]).
                            if seq > cursor + 1 {
                                for framed in replay_gap(&authority_server, &rid, &mut cursor) {
                                    yield framed;
                                }
                            }
                            // Emit only if not already covered by a replay/settlement.
                            if seq > cursor {
                                cursor = seq;
                                yield framed;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(missed)) => {
                            warn!(
                                runtime_id = %rid.as_str(),
                                missed_events = missed,
                                "authority-server base broadcast lagged; replaying the backlog",
                            );
                            for framed in replay_gap(&authority_server, &rid, &mut cursor) {
                                yield framed;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    frame = settlement.recv() => match frame {
                        // Settlements are pre-sequenced at emission and non-lossy;
                        // the cursor gate skips any the backlog replay already covered.
                        Some(framed) => {
                            let seq = framed.seq();
                            if seq > cursor {
                                cursor = seq;
                                yield framed;
                            }
                        }
                        // Sink sender dropped (authority server shutdown) — end.
                        None => break,
                    }
                }
            }
        })
    }
}

/// Replay the backlog from `cursor` to bridge a live gap (D49 [0]): the missing
/// frames are still retained (records precede the lossy broadcast), so `Replay`
/// fills them; a `Collapse` (backlog overflowed) resets the near node. Advances
/// `cursor` past everything it returns.
fn replay_gap(
    authority_server: &AuthorityServer,
    rid: &AuthorityServerLinkId,
    cursor: &mut u64,
) -> Vec<SequencedFrame> {
    match authority_server.replay_resume(rid, Some(*cursor)) {
        Resume::Replay(frames) => {
            let mut out = Vec::with_capacity(frames.len());
            for framed in frames {
                *cursor = framed.seq();
                out.push(framed);
            }
            out
        }
        Resume::Collapse => {
            let highest = authority_server.highest_seq(rid);
            *cursor = highest;
            vec![SequencedFrame::reset(highest)]
        }
        Resume::Fresh => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_model::{AccountId, MessageId};
    use serde_json::json;

    fn message_event(payload: serde_json::Value) -> DomainEvent {
        DomainEvent {
            seq: 1,
            account_id: AccountId("acct".into()),
            topic: EVENT_TOPIC_MESSAGE_UPDATED.to_string(),
            occurred_at: "2026-06-24T00:00:00Z".into(),
            mailbox_id: None,
            message_id: Some(MessageId("m1".into())),
            payload,
        }
    }

    fn fold(keywords: &[&str], mailboxes: &[&str]) -> MessageFoldState {
        MessageFoldState {
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            mailbox_ids: mailboxes.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn present_event_asserts_the_complete_current_state() {
        let event = message_event(json!({ "messageId": "m1", "changes": { "keywords": true } }));
        let assertion =
            message_event_to_assertion(&event, Some(fold(&["$flagged"], &["inbox"]))).unwrap();
        assert_eq!(assertion.message_id, "m1");
        assert_eq!(
            assertion.update,
            BaseUpdate::Present(fold(&["$flagged"], &["inbox"]))
        );
    }

    #[test]
    fn deleted_event_asserts_removal_regardless_of_read() {
        let event = message_event(json!({ "messageId": "m1", "deleted": true }));
        let assertion = message_event_to_assertion(&event, Some(fold(&[], &["inbox"]))).unwrap();
        assert_eq!(assertion.update, BaseUpdate::Removed);
    }

    #[test]
    fn present_event_with_missing_read_falls_back_to_removal() {
        let event = message_event(json!({ "messageId": "m1" }));
        let assertion = message_event_to_assertion(&event, None).unwrap();
        assert_eq!(assertion.update, BaseUpdate::Removed);
    }

    #[test]
    fn non_message_events_produce_no_assertion() {
        let mut event = message_event(json!({}));
        event.topic = "sync.completed".into();
        assert!(message_event_to_assertion(&event, Some(fold(&[], &[]))).is_none());
    }
}
