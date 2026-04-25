use std::sync::Arc;

use async_trait::async_trait;
use posthaste_domain::{
    now_iso8601, plan_imap_mailbox_sync, AccountId, BlobId, FetchedBody, GatewayError, Identity,
    ImapMailboxSyncPlan, MailGateway, MailStore, MailboxId, MessageId, MutationOutcome,
    PushTransport, ReplyContext, SendMessageRequest, SetKeywordsCommand, StoreError, SyncBatch,
    SyncCursor,
};
use tracing::warn;

use crate::{
    append_smtp_sent_copy, apply_imap_keyword_delta_by_location,
    copy_imap_message_to_mailbox_by_location, discover_imap_account,
    expunge_imap_message_by_location, fetch_imap_reply_context_by_location,
    fetch_mailbox_header_snapshot, fetch_message_body_by_location, fetch_raw_message_by_location,
    imap_attachment_bytes_from_raw_mime, imap_delta_sync_batch, imap_full_sync_batch,
    imap_mailbox_replacement_delta, imap_mailbox_state_from_header_snapshot,
    mark_imap_message_deleted_by_location, parse_imap_attachment_blob_id, smtp_sent_copy_strategy,
    submit_smtp_message, DiscoveredImapAccount, ImapAdapterError, ImapConnectionConfig,
    SmtpConnectionConfig, SmtpSentCopyStrategy,
};

/// Live IMAP/SMTP gateway after successful IMAP discovery.
///
/// The first implementation performs conservative full metadata snapshots.
/// Mutations use conservative IMAP commands where implemented and reject
/// unsupported command surfaces with typed gateway errors.
pub struct LiveImapSmtpGateway {
    config: ImapConnectionConfig,
    smtp_config: SmtpConnectionConfig,
    username: String,
    discovery: DiscoveredImapAccount,
    store: Option<Arc<dyn MailStore>>,
}

impl LiveImapSmtpGateway {
    pub async fn connect(
        config: ImapConnectionConfig,
        smtp_config: SmtpConnectionConfig,
        store: Option<Arc<dyn MailStore>>,
    ) -> Result<Self, ImapAdapterError> {
        let username = config.username.clone();
        let discovery = discover_imap_account(&config).await?;
        Ok(Self {
            config,
            smtp_config,
            username,
            discovery,
            store,
        })
    }

    pub fn discovery(&self) -> &DiscoveredImapAccount {
        &self.discovery
    }

    fn location_and_mailbox_name(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(posthaste_domain::ImapMessageLocation, String), GatewayError> {
        let locations = self
            .store("message location lookup")?
            .list_imap_message_locations(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        let location = locations.first().cloned().ok_or_else(|| {
            GatewayError::Rejected(format!("missing IMAP location for message {message_id}"))
        })?;
        let mailbox_name = self.mailbox_name_for_id(account_id, &location.mailbox_id)?;

        Ok((location, mailbox_name))
    }

    fn store(&self, operation: &str) -> Result<&Arc<dyn MailStore>, GatewayError> {
        self.store.as_ref().ok_or_else(|| unsupported(operation))
    }

    fn mailbox_name_for_id(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<String, GatewayError> {
        self.store("mailbox name lookup")?
            .get_imap_mailbox_state(account_id, mailbox_id)
            .map_err(store_error_to_gateway)?
            .map(|state| state.mailbox_name)
            .or_else(|| {
                self.discovery
                    .mailboxes
                    .iter()
                    .find(|mailbox| &mailbox.id == mailbox_id)
                    .map(|mailbox| mailbox.name.clone())
            })
            .ok_or_else(|| {
                GatewayError::Rejected(format!("missing IMAP mailbox name for {mailbox_id}"))
            })
    }
}

fn unsupported(operation: &str) -> GatewayError {
    GatewayError::Rejected(format!(
        "IMAP/SMTP {operation} is not implemented yet; discovery is available"
    ))
}

#[async_trait]
impl MailGateway for LiveImapSmtpGateway {
    async fn sync(
        &self,
        account_id: &AccountId,
        _cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError> {
        let discovery = discover_imap_account(&self.config)
            .await
            .map_err(imap_error_to_gateway)?;
        let updated_at = now_iso8601().map_err(GatewayError::Rejected)?;
        let store = self.store.as_ref();
        let mut headers = Vec::new();
        let mut local_locations = Vec::new();
        let mut mailbox_states = Vec::new();
        let mut requires_full_message_snapshot = store.is_none();
        for mailbox in discovery
            .mailboxes
            .iter()
            .filter(|mailbox| mailbox.selectable)
        {
            let snapshot =
                fetch_mailbox_header_snapshot(&self.config, &mailbox.name, updated_at.clone())
                    .await
                    .map_err(imap_error_to_gateway)?;
            mailbox_states.push(imap_mailbox_state_from_header_snapshot(
                &snapshot,
                updated_at.clone(),
            ));
            if let Some(store) = store {
                let stored_state = store
                    .get_imap_mailbox_state(account_id, &mailbox.id)
                    .map_err(store_error_to_gateway)?;
                if matches!(
                    plan_imap_mailbox_sync(
                        &discovery.capabilities,
                        stored_state.as_ref(),
                        &snapshot.selected
                    ),
                    ImapMailboxSyncPlan::FullSnapshot { .. }
                ) {
                    requires_full_message_snapshot = true;
                }
                local_locations.extend(
                    store
                        .list_imap_mailbox_message_locations(account_id, &mailbox.id)
                        .map_err(store_error_to_gateway)?,
                );
            }
            headers.extend(snapshot.headers);
        }

        if requires_full_message_snapshot {
            Ok(imap_full_sync_batch(
                account_id,
                discovery,
                headers,
                mailbox_states,
                updated_at,
            ))
        } else {
            Ok(imap_delta_sync_batch(
                account_id,
                discovery,
                headers,
                mailbox_states,
                local_locations,
                updated_at,
            ))
        }
    }

    async fn fetch_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;

        fetch_message_body_by_location(&self.config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn download_blob(
        &self,
        account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        let (message_id, _attachment_index) =
            parse_imap_attachment_blob_id(blob_id).map_err(imap_error_to_gateway)?;
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, &message_id)?;
        let raw_mime = fetch_raw_message_by_location(&self.config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)?;

        imap_attachment_bytes_from_raw_mime(blob_id, raw_mime).map_err(imap_error_to_gateway)
    }

    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;

        apply_imap_keyword_delta_by_location(&self.config, &mailbox_name, &location, command)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let store = self.store("mailbox replacement state lookup")?;
        let current_mailbox_ids = store
            .get_message_mailboxes(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        let locations = store
            .list_imap_message_locations(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        let source_location = locations.first().cloned().ok_or_else(|| {
            GatewayError::Rejected(format!("missing IMAP location for message {message_id}"))
        })?;
        let source_mailbox_name =
            self.mailbox_name_for_id(account_id, &source_location.mailbox_id)?;
        let delta = imap_mailbox_replacement_delta(&current_mailbox_ids, mailbox_ids);

        for mailbox_id in &delta.add {
            let target_mailbox_name = self.mailbox_name_for_id(account_id, mailbox_id)?;
            copy_imap_message_to_mailbox_by_location(
                &self.config,
                &source_mailbox_name,
                &source_location,
                &target_mailbox_name,
            )
            .await
            .map_err(imap_error_to_gateway)?;
        }

        for mailbox_id in &delta.remove {
            let location = locations
                .iter()
                .find(|location| &location.mailbox_id == mailbox_id)
                .ok_or_else(|| {
                    imap_error_to_gateway(ImapAdapterError::MissingMessageLocation(
                        mailbox_id.to_string(),
                    ))
                })?;
            let mailbox_name = self.mailbox_name_for_id(account_id, mailbox_id)?;
            mark_imap_message_deleted_by_location(&self.config, &mailbox_name, location)
                .await
                .map_err(imap_error_to_gateway)?;
        }

        Ok(MutationOutcome { cursor: None })
    }

    async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        let locations = self
            .store("message deletion state lookup")?
            .list_imap_message_locations(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        if locations.is_empty() {
            return Err(GatewayError::Rejected(format!(
                "missing IMAP location for message {message_id}"
            )));
        }

        for location in &locations {
            let mailbox_name = self.mailbox_name_for_id(account_id, &location.mailbox_id)?;
            if self.discovery.capabilities.supports_uidplus() {
                expunge_imap_message_by_location(&self.config, &mailbox_name, location)
                    .await
                    .map_err(imap_error_to_gateway)?;
            } else {
                mark_imap_message_deleted_by_location(&self.config, &mailbox_name, location)
                    .await
                    .map_err(imap_error_to_gateway)?;
            }
        }

        Ok(MutationOutcome { cursor: None })
    }

    async fn set_mailbox_role(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _expected_state: Option<&str>,
        _role: Option<&str>,
        _clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(unsupported("mailbox role mutation"))
    }

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Ok(Identity {
            id: "imap-smtp-default".to_string(),
            name: self
                .username
                .split('@')
                .next()
                .unwrap_or(self.username.as_str())
                .to_string(),
            email: self.username.clone(),
        })
    }

    async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;

        fetch_imap_reply_context_by_location(&self.config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        let submitted = submit_smtp_message(&self.smtp_config, request)
            .await
            .map_err(imap_error_to_gateway)?;

        if smtp_sent_copy_strategy(&self.smtp_config.provider)
            == SmtpSentCopyStrategy::AppendToSentMailbox
        {
            if let Some(sent_mailbox) = self
                .discovery
                .mailboxes
                .iter()
                .find(|mailbox| mailbox.selectable && mailbox.role == Some("sent"))
            {
                if let Err(error) =
                    append_smtp_sent_copy(&self.config, &sent_mailbox.name, &submitted.raw_message)
                        .await
                {
                    warn!(
                        mailbox = sent_mailbox.name,
                        error = %error,
                        "SMTP send accepted but IMAP Sent copy append failed"
                    );
                }
            } else {
                warn!("SMTP send accepted but no selectable IMAP Sent mailbox was discovered");
            }
        }

        Ok(())
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_identity_uses_configured_username() {
        let gateway = LiveImapSmtpGateway {
            config: test_config(),
            smtp_config: test_smtp_config(),
            username: "alice@example.test".to_string(),
            discovery: DiscoveredImapAccount {
                capabilities: posthaste_domain::ImapCapabilities::default(),
                mailboxes: Vec::new(),
            },
            store: None,
        };

        let identity = gateway
            .fetch_identity(&AccountId::from("primary"))
            .await
            .expect("identity");

        assert_eq!(identity.email, "alice@example.test");
        assert_eq!(identity.name, "alice");
    }

    #[tokio::test]
    async fn fetch_body_reports_clear_unsupported_error() {
        let gateway = LiveImapSmtpGateway {
            config: test_config(),
            smtp_config: test_smtp_config(),
            username: "alice@example.test".to_string(),
            discovery: DiscoveredImapAccount {
                capabilities: posthaste_domain::ImapCapabilities::default(),
                mailboxes: Vec::new(),
            },
            store: None,
        };

        let error = gateway
            .fetch_message_body(&AccountId::from("primary"), &MessageId::from("message"))
            .await
            .expect_err("body fetch is not implemented");

        assert!(matches!(error, GatewayError::Rejected(message) if message.contains("discovery")));
    }

    fn test_config() -> ImapConnectionConfig {
        ImapConnectionConfig {
            host: "imap.example.test".to_string(),
            port: 993,
            security: posthaste_domain::TransportSecurity::Tls,
            username: "alice@example.test".to_string(),
            secret: "secret".to_string(),
            auth: posthaste_domain::ProviderAuthKind::Password,
        }
    }

    fn test_smtp_config() -> SmtpConnectionConfig {
        SmtpConnectionConfig {
            host: "smtp.example.test".to_string(),
            port: 587,
            security: posthaste_domain::TransportSecurity::StartTls,
            username: "alice@example.test".to_string(),
            secret: "secret".to_string(),
            auth: posthaste_domain::ProviderAuthKind::Password,
            provider: posthaste_domain::ProviderHint::Generic,
        }
    }
}

fn imap_error_to_gateway(error: ImapAdapterError) -> GatewayError {
    match error {
        ImapAdapterError::MissingTransport
        | ImapAdapterError::MissingSmtpTransport
        | ImapAdapterError::MissingUsername
        | ImapAdapterError::MissingSecret
        | ImapAdapterError::InvalidMailboxName(_)
        | ImapAdapterError::MissingSelectData(_)
        | ImapAdapterError::UidValidityMismatch { .. }
        | ImapAdapterError::MissingFetchData(_)
        | ImapAdapterError::InvalidUidSequence(_)
        | ImapAdapterError::InvalidKeywordFlag { .. }
        | ImapAdapterError::MissingMessageLocation(_)
        | ImapAdapterError::InvalidBlobId(_)
        | ImapAdapterError::ParseMessageHeaders
        | ImapAdapterError::ParseMessageBody
        | ImapAdapterError::MissingAttachment { .. }
        | ImapAdapterError::InvalidSmtpAddress { .. }
        | ImapAdapterError::BuildSmtpMessage(_) => GatewayError::Rejected(error.to_string()),
        ImapAdapterError::Client(message) | ImapAdapterError::Smtp(message) => {
            GatewayError::Network(message)
        }
    }
}

fn store_error_to_gateway(error: StoreError) -> GatewayError {
    GatewayError::Rejected(format!("IMAP local state lookup failed: {error}"))
}
