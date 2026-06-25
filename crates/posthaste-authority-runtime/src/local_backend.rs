//! `LocalBackend`: the in-process [`BackendApi`] implementation.
//!
//! The default ([replication backend-link L2 §2](../replication/backend-link/L2.md)):
//! direct calls to a co-located [`Backend`] far node, zero serialization, instant
//! confirmation — byte-for-byte the pre-link behavior (`colocated-unchanged`). The
//! remote counterpart (`RemoteBackend`) lives in `posthaste-runtime`, the near node.
//!
//! @spec docs/replication/backend-link/L2#2-backendapi-implementations-localbackend-remotebackend

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use posthaste_domain::{
    AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress, CommandAck,
    ConversationId, ConversationView, DomainEvent, DraftContent, EventFilter, Identity, MailboxId,
    MailboxSummary, MessageDetail, MessageId, MessageSummary, Operation, OperationId,
    RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext, SendMessageRequest,
    SetKeywordsCommand, SmartMailbox, SmartMailboxId, SmartMailboxSummary, SyncMode, TagSummary,
    EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_link_contract::{
    BackendApi, BaseAssertion, BaseUpdate, DownFrame, DownStream, LinkCoverage,
};
use posthaste_link_core::MessageFoldState;
use posthaste_runtime_contract::{
    AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation, MailQueryPage,
    MailQueryRequest, MessageResourceKind, MutationReceipt, MutationRequest,
    MutationSettlementState, PatchAccountMutation, PatchAppSettingsMutation,
    PatchSmartMailboxMutation, RuntimeAccountList, RuntimeError, RuntimeMutationId,
    RuntimeResourceBytes,
};

use crate::backend::Backend;

pub(crate) struct LocalBackend {
    backend: Arc<Backend>,
}

impl LocalBackend {
    pub(crate) fn new(backend: Arc<Backend>) -> Self {
        Self { backend }
    }
}

/// How a message domain event names its message's authoritative base change —
/// the pure half of the down-channel mapping, factored out so it is testable
/// without a running store. `current` is the message's complete fold state read
/// from the backend (`None` when the message is gone); a `deleted` event maps to
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
impl BackendApi for LocalBackend {
    /// Up-channel: apply the named mutation to the co-located backend and return
    /// a receipt carrying the backend's `RuntimeMutationId` (the confirmation
    /// join key) and the command's events as `output`. In-process this is a
    /// direct call — no serialization, the mutation is applied (and confirmed)
    /// before the receipt returns.
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let ack = self.backend.apply_named_message_mutation(&mutation).await?;
        let output = serde_json::to_value(&ack).map_err(|error| {
            RuntimeError::internal(
                format!("failed to serialize mutation output: {error}"),
                None,
            )
        })?;
        Ok(MutationReceipt {
            runtime_mutation_id: Some(RuntimeMutationId::new(uuid::Uuid::new_v4().to_string())),
            client_mutation_id: mutation.client_mutation_id,
            name: mutation.name,
            state: MutationSettlementState::Accepted,
            error: None,
            output,
        })
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
    /// Read channel: serve the co-located backend's query computation directly.
    /// This is what a remote runtime reads through to (via `link_router`).
    async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.backend.query_mail_page(request).await
    }

    async fn current_summary(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.backend.current_summary(&account_id, &message_id).await
    }

    async fn message_detail(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.backend.message_detail(&account_id, &message_id)
    }

    async fn conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.backend.conversation(&conversation_id)
    }

    async fn account_count(&self) -> Result<Option<usize>, RuntimeError> {
        Ok(self.backend.account_count())
    }

    async fn list_accounts(&self) -> Result<RuntimeAccountList, RuntimeError> {
        self.backend.list_accounts().await
    }

    async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AccountOverview>, RuntimeError> {
        self.backend.get_account(account_id).await
    }

    async fn app_settings(&self) -> Result<AppSettings, RuntimeError> {
        self.backend.app_settings()
    }

    async fn resolve_account_scope(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        self.backend.resolve_account_scope(scope)
    }

    async fn list_mailboxes(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError> {
        self.backend.list_mailboxes(scope)
    }

    async fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.backend.list_smart_mailboxes()
    }

    async fn get_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.backend.get_smart_mailbox(smart_mailbox_id)
    }

    async fn list_tags(&self, scope: AccountScopeRequest) -> Result<Vec<TagSummary>, RuntimeError> {
        self.backend.list_tags(scope)
    }

    async fn get_identity(&self, account_id: AccountId) -> Result<Identity, RuntimeError> {
        self.backend.get_identity(account_id).await
    }

    async fn get_reply_context(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError> {
        self.backend.get_reply_context(account_id, message_id).await
    }

    async fn list_sender_addresses(&self) -> Result<Vec<CachedSenderAddress>, RuntimeError> {
        self.backend.list_sender_addresses()
    }

    async fn list_pending_operations(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        self.backend.list_pending_operations(account_id)
    }

    async fn replay_events(&self, filter: EventFilter) -> Result<Vec<DomainEvent>, RuntimeError> {
        self.backend.replay_events(filter)
    }

    async fn get_draft_content(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError> {
        self.backend.get_draft_content(account_id, message_id).await
    }

    async fn get_message_resource(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError> {
        self.backend
            .get_message_resource(account_id, message_id, kind)
            .await
    }

    async fn set_keywords(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<CommandAck, RuntimeError> {
        self.backend
            .set_keywords(account_id, message_id, command)
            .await
    }

    async fn add_to_mailbox(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<CommandAck, RuntimeError> {
        self.backend
            .add_to_mailbox(account_id, message_id, command)
            .await
    }

    async fn remove_from_mailbox(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<CommandAck, RuntimeError> {
        self.backend
            .remove_from_mailbox(account_id, message_id, command)
            .await
    }

    async fn replace_mailboxes(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<CommandAck, RuntimeError> {
        self.backend
            .replace_mailboxes(account_id, message_id, command)
            .await
    }

    async fn destroy_message(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandAck, RuntimeError> {
        self.backend.destroy(account_id, message_id).await
    }

    async fn set_mailbox_role(
        &self,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        self.backend
            .set_mailbox_role(account_id, mailbox_id, role)
            .await
    }

    async fn send_message(
        &self,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        self.backend.send_message(account_id, request).await
    }

    async fn save_draft(
        &self,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        self.backend.save_draft(account_id, draft_id, request).await
    }

    async fn delete_draft(
        &self,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        self.backend.delete_draft(account_id, draft_id).await
    }

    async fn discard_operation(&self, operation_id: OperationId) -> Result<(), RuntimeError> {
        self.backend.discard_operation(operation_id)
    }

    async fn retry_operation(
        &self,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.backend.retry_operation(account_id, operation_id).await
    }

    async fn sync_account(
        &self,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        self.backend.sync_account(account_id, mode).await
    }

    async fn patch_app_settings(
        &self,
        mutation: PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        self.backend.patch_app_settings(mutation)
    }

    async fn preview_automation_rule(
        &self,
        mutation: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError> {
        self.backend.preview_automation_rule(mutation)
    }

    async fn create_smart_mailbox(
        &self,
        mutation: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.backend.create_smart_mailbox(mutation)
    }

    async fn patch_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
        mutation: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.backend.patch_smart_mailbox(smart_mailbox_id, mutation)
    }

    async fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.backend.delete_smart_mailbox(smart_mailbox_id)
    }

    async fn reset_default_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.backend.reset_default_smart_mailboxes()
    }

    async fn create_account(
        &self,
        mutation: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.backend.create_account(mutation).await
    }

    async fn patch_account(
        &self,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.backend.patch_account(account_id, mutation).await
    }

    async fn delete_account(&self, account_id: AccountId) -> Result<(), RuntimeError> {
        self.backend.delete_account(account_id).await
    }

    async fn verify_account(
        &self,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.backend.verify_account(account_id).await
    }

    async fn set_account_enabled(
        &self,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        self.backend.set_account_enabled(account_id, enabled).await
    }

    async fn reload_config(&self) -> Result<(), RuntimeError> {
        self.backend.reload_config().await
    }

    async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        let backend = self.backend.clone();
        let mut receiver = backend.subscribe_events();
        let stream = async_stream::stream! {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let current = event
                            .message_id
                            .as_ref()
                            .and_then(|message_id| {
                                backend.current_fold_state(&event.account_id, message_id).ok().flatten()
                            });
                        if let Some(assertion) = message_event_to_assertion(&event, current) {
                            yield DownFrame::Base {
                                assertions: vec![assertion],
                            };
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain::{AccountId, MessageId};
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
