use async_trait::async_trait;
use posthaste_domain::{
    now_iso8601, AccountId, BlobId, FetchedBody, GatewayError, Identity, MailGateway, MailboxId,
    MessageId, MutationOutcome, PushTransport, ReplyContext, SendMessageRequest,
    SetKeywordsCommand, SyncBatch, SyncCursor,
};

use crate::{
    discover_imap_account, imap_mailbox_sync_batch, DiscoveredImapAccount, ImapAdapterError,
    ImapConnectionConfig,
};

/// Live IMAP/SMTP gateway after successful IMAP discovery.
///
/// The first implementation slice intentionally connects and discovers
/// capabilities/mailboxes only. Sync and mutation methods fail with typed
/// gateway errors until the full-snapshot sync path is implemented.
pub struct LiveImapSmtpGateway {
    config: ImapConnectionConfig,
    username: String,
    discovery: DiscoveredImapAccount,
}

impl LiveImapSmtpGateway {
    pub async fn connect(config: ImapConnectionConfig) -> Result<Self, ImapAdapterError> {
        let username = config.username.clone();
        let discovery = discover_imap_account(&config).await?;
        Ok(Self {
            config,
            username,
            discovery,
        })
    }

    pub fn discovery(&self) -> &DiscoveredImapAccount {
        &self.discovery
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
        Ok(imap_mailbox_sync_batch(account_id, discovery, updated_at))
    }

    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        Err(unsupported("body fetch"))
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        _blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        Err(unsupported("blob download"))
    }

    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _expected_state: Option<&str>,
        _command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(unsupported("keyword mutation"))
    }

    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _expected_state: Option<&str>,
        _mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        Err(unsupported("mailbox replacement"))
    }

    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(unsupported("message deletion"))
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
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        Err(unsupported("reply context fetch"))
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        Err(unsupported("SMTP send"))
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
            username: "alice@example.test".to_string(),
            discovery: DiscoveredImapAccount {
                capabilities: posthaste_domain::ImapCapabilities::default(),
                mailboxes: Vec::new(),
            },
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
            username: "alice@example.test".to_string(),
            discovery: DiscoveredImapAccount {
                capabilities: posthaste_domain::ImapCapabilities::default(),
                mailboxes: Vec::new(),
            },
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
}

fn imap_error_to_gateway(error: ImapAdapterError) -> GatewayError {
    match error {
        ImapAdapterError::MissingTransport
        | ImapAdapterError::MissingUsername
        | ImapAdapterError::MissingSecret
        | ImapAdapterError::InvalidMailboxName(_)
        | ImapAdapterError::MissingSelectData(_)
        | ImapAdapterError::ParseMessageHeaders => GatewayError::Rejected(error.to_string()),
        ImapAdapterError::Client(message) => GatewayError::Network(message),
    }
}
