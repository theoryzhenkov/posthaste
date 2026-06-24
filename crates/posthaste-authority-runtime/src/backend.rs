//! The backend far node: the single owner of message-command backend access.
//!
//! This is the **far node** of the runtime↔backend coherent link
//! ([replication L4 §2-§3](../replication/L4.md)). It owns the `MailService` +
//! store and is the one place message-state commands cross from the runtime into
//! the backend: each applies the command to the service, publishes the resulting
//! authoritative domain events, and nudges the provider outbox to flush.
//!
//! Today it is reached **in-process** (co-located), through
//! [`InProcessTransport`](crate::transport::InProcessTransport): the runtime
//! calls it directly, zero serialization, identical to the pre-link behavior
//! (assertion `colocated-unchanged`). Extracting it as a named type is the W1
//! seam — the runtime no longer reaches the backend by scattered direct
//! `service`/`store` calls on the mutation path; it goes through this far node.
//!
//! Reads stay on the runtime's direct store access for now; W2 moves the
//! runtime's served views onto a near-node base cache fed by this node's
//! down-channel, at which point reads stop crossing the link too.
//!
//! @spec docs/replication/L4#3-the-link-contract-backendlink

use std::sync::Arc;

use posthaste_domain::{
    AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CommandAck, ConversationId,
    ConversationView, DomainEvent, MailService, MailboxId, MailboxSummary, MessageDetail, MessageId,
    MessageSummary, RemoveFromMailboxCommand, ReplaceMailboxesCommand, SetKeywordsCommand,
    SyncTrigger,
};
use posthaste_link_core::MessageFoldState;
use posthaste_observability::{events, ph_warn};
use posthaste_runtime_contract::{
    MailQueryPage, MailQueryRequest, MutationRequest, RuntimeAccountList, RuntimeError,
};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::account_reads::AccountReadService;
use crate::live_accounts::LiveAccountRuntimeProvider;
use crate::mail_queries::MailQueryService;

/// Build a single-keyword add/remove command from a desired presence. Shared by
/// the backend's read-state/flagged-state application and the runtime's history
/// capture for the same mutations.
pub(crate) fn keyword_toggle(keyword: &str, present: bool) -> SetKeywordsCommand {
    if present {
        SetKeywordsCommand {
            add: vec![keyword.to_string()],
            remove: Vec::new(),
        }
    } else {
        SetKeywordsCommand {
            add: Vec::new(),
            remove: vec![keyword.to_string()],
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSetKeywordsMutationArgs {
    pub source_id: String,
    pub message_id: String,
    pub command: SetKeywordsCommand,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSetReadStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub read: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSetFlaggedStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub flagged: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSetUserTagsArgs {
    pub source_id: String,
    pub message_id: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageMoveToMailboxArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageMoveToRoleArgs {
    pub source_id: String,
    pub message_id: String,
    pub role: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageReplaceMailboxesArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_ids: Vec<String>,
}

/// A message mutation that targets one message by id (archive/trash/destroy).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageTargetArgs {
    pub source_id: String,
    pub message_id: String,
}

/// The backend far node ([replication L4 §3](../replication/L4.md)): owns the
/// service + store + the live-account supervisor + the event publisher, and
/// applies message-state commands to them.
pub(crate) struct Backend {
    service: Arc<MailService>,
    mail_queries: Arc<MailQueryService>,
    account_reads: Arc<AccountReadService>,
    live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
    event_sender: broadcast::Sender<DomainEvent>,
}

impl Backend {
    pub(crate) fn new(
        service: Arc<MailService>,
        mail_queries: Arc<MailQueryService>,
        account_reads: Arc<AccountReadService>,
        live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            service,
            mail_queries,
            account_reads,
            live_accounts,
            event_sender,
        }
    }

    /// Read channel: the account list.
    pub(crate) async fn list_accounts(&self) -> Result<RuntimeAccountList, RuntimeError> {
        Ok(self.account_reads.list_accounts().await?)
    }

    /// Read channel: one account's overview (`None` when absent).
    pub(crate) async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AccountOverview>, RuntimeError> {
        Ok(self.account_reads.get_account(account_id).await?)
    }

    /// Read channel: the application settings.
    pub(crate) fn app_settings(&self) -> Result<AppSettings, RuntimeError> {
        Ok(self.account_reads.app_settings()?)
    }

    /// Read channel: compute a page of a mail-list query — the query engine is
    /// the authority's ([replication L4 W4](../replication/L4.md)). A near node
    /// reads through here.
    pub(crate) async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.mail_queries.query_mail_page(request).await
    }

    /// Read channel: the message's current canonical summary (the point read
    /// behind undo-history).
    pub(crate) async fn current_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        let result = self
            .service
            .get_message_detail(account_id, message_id, None)
            .await?;
        Ok(result.detail.map(|detail| detail.summary))
    }

    /// Read channel: a message's detail (header + attachments, body-free) for the
    /// `messageDetail` view.
    pub(crate) async fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.mail_queries.message_detail(account_id, message_id).await
    }

    /// Read channel: an overlay-folded conversation for the `conversation` view.
    pub(crate) fn conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.mail_queries.conversation(conversation_id)
    }

    /// Publish authoritative domain events on the down-channel broadcast. In the
    /// co-located deployment this is the same event bus the runtime's views and
    /// the SSE event stream already consume.
    pub(crate) fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }

    /// A receiver on the authoritative domain-event broadcast — the raw signal
    /// the link's down-channel is built from
    /// ([`InProcessTransport::subscribe`](crate::transport::InProcessTransport)).
    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<DomainEvent> {
        self.event_sender.subscribe()
    }

    /// The message's current canonical fold state (keywords + mailbox
    /// membership) read from the authoritative store, or `None` if it is gone.
    ///
    /// The far node authors **complete** base assertions: individual command
    /// events do not all carry the full post-state (a mailbox move event omits
    /// keywords), but `MessageReplica`'s base is a whole-message replace, so the
    /// down-channel reads the current summary to assert the complete state
    /// ([replication L4 §3](../replication/L4.md)).
    pub(crate) fn current_fold_state(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageFoldState>, RuntimeError> {
        let detail = self.service.get_message_header(account_id, message_id)?;
        Ok(detail.map(|detail| MessageFoldState {
            keywords: detail.summary.keywords,
            mailbox_ids: detail
                .summary
                .mailbox_ids
                .iter()
                .map(|mailbox_id| mailbox_id.as_str().to_string())
                .collect(),
        }))
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
        let result = self.service.destroy_message(&account_id, &message_id).await?;
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

    /// Resolve the account's mailbox for `role` and replace the message's
    /// mailbox membership with it. Role resolution is backend-owned so the
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

    /// Apply one named message mutation — the backend's up-channel handler. This
    /// is the dispatch from a transport-neutral named mutation
    /// (`message.setKeywords` / `message.archive` / …) to the typed command,
    /// moved here from the runtime: the backend "accepts named mutations"
    /// ([replication L4 §3](../replication/L4.md)). The runtime keeps the
    /// session/undo/scope concerns around this call; this node only applies the
    /// effect and returns the resulting events.
    ///
    /// @spec docs/runtime/L2#mutation-pipeline-and-catalog
    pub(crate) async fn apply_named_message_mutation(
        &self,
        request: &MutationRequest,
    ) -> Result<CommandAck, RuntimeError> {
        match request.name.as_str() {
            "message.setKeywords" => {
                let args: MessageSetKeywordsMutationArgs = parse_args(request)?;
                self.set_keywords(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    args.command,
                )
                .await
            }
            "message.setReadState" => {
                let args: MessageSetReadStateArgs = parse_args(request)?;
                self.set_keywords(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    keyword_toggle("$seen", args.read),
                )
                .await
            }
            "message.setFlaggedState" => {
                let args: MessageSetFlaggedStateArgs = parse_args(request)?;
                self.set_keywords(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    keyword_toggle("$flagged", args.flagged),
                )
                .await
            }
            "message.setUserTags" => {
                let args: MessageSetUserTagsArgs = parse_args(request)?;
                self.set_keywords(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    SetKeywordsCommand {
                        add: args.add,
                        remove: args.remove,
                    },
                )
                .await
            }
            "message.moveToMailbox" => {
                let args: MessageMoveToMailboxArgs = parse_args(request)?;
                self.replace_mailboxes(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    ReplaceMailboxesCommand {
                        mailbox_ids: vec![MailboxId(args.mailbox_id)],
                    },
                )
                .await
            }
            "message.replaceMailboxes" => {
                let args: MessageReplaceMailboxesArgs = parse_args(request)?;
                self.replace_mailboxes(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    ReplaceMailboxesCommand {
                        mailbox_ids: args.mailbox_ids.into_iter().map(MailboxId).collect(),
                    },
                )
                .await
            }
            "message.moveToRole" => {
                let args: MessageMoveToRoleArgs = parse_args(request)?;
                self.move_message_to_role(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    args.role,
                )
                .await
            }
            "message.archive" | "message.trash" | "message.restoreToInbox" => {
                let args: MessageTargetArgs = parse_args(request)?;
                let role = match request.name.as_str() {
                    "message.archive" => "archive",
                    "message.trash" => "trash",
                    _ => "inbox",
                };
                self.move_message_to_role(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    role.to_string(),
                )
                .await
            }
            "message.destroy" => {
                let args: MessageTargetArgs = parse_args(request)?;
                self.destroy(AccountId(args.source_id), MessageId(args.message_id))
                    .await
            }
            _ => Err(RuntimeError::invalid_mutation(format!(
                "unknown runtime mutation '{}'",
                request.name
            ))),
        }
    }
}

pub(crate) fn parse_args<T>(request: &MutationRequest) -> Result<T, RuntimeError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(request.args.clone()).map_err(|error| {
        RuntimeError::with_details(
            posthaste_runtime_contract::RuntimeErrorCode::InvalidMutation,
            format!("invalid args for mutation '{}'", request.name),
            serde_json::json!({ "error": error.to_string() }),
        )
    })
}
