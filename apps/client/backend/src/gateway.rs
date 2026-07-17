//! Per-account gateway construction: build a live provider connection
//! (JMAP over the engine, IMAP/SMTP, or the mock driver) from account
//! settings plus the secret store, paired with its optional push stream.

use std::sync::Arc;

use posthaste_domain_model::{
    AccountDriver, AccountSettings, GatewayError, RemoteIdleScope, RemoteObservationPolicy,
    SecretRef, ServiceError,
};
use posthaste_domain_service::{
    MailStore, PushEventStream, ResilientPushConfig, SecretResolver, SecretStore, SharedGateway,
};
use posthaste_engine::{connect_jmap_client, LiveJmapGateway, MockJmapGateway};
use posthaste_imap::{
    ImapAdapterError, ImapConnectionConfig, LiveImapSmtpGateway, SmtpConnectionConfig,
};
use posthaste_observability::{events, ph_info, ph_warn};

use crate::push::resilient_push_stream;

/// A live gateway connection paired with its optional push event stream.
pub(crate) struct AccountConnection {
    pub(crate) gateway: SharedGateway,
    pub(crate) push_events: Option<PushEventStream>,
    pub(crate) remote_observation: RemoteObservationPolicy,
    /// Whether the provider advertises no push transport at all (IMAP
    /// without IDLE), so the supervisor can mark push `Unsupported` up front.
    pub(crate) push_unsupported: bool,
}

/// Local runtime connection state; keeps gateway and push stream lifetimes
/// coupled so a teardown drops both together.
#[derive(Default)]
pub(crate) enum ConnectionState {
    #[default]
    Disconnected,
    Connected(AccountConnection),
}

impl ConnectionState {
    pub(crate) fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    pub(crate) fn gateway(&self) -> Option<SharedGateway> {
        match self {
            Self::Connected(connection) => Some(connection.gateway.clone()),
            Self::Disconnected => None,
        }
    }

    pub(crate) fn remote_observation(&self) -> Option<RemoteObservationPolicy> {
        match self {
            Self::Connected(connection) => Some(connection.remote_observation),
            Self::Disconnected => None,
        }
    }

    pub(crate) fn push_events_mut(&mut self) -> Option<&mut PushEventStream> {
        match self {
            Self::Connected(connection) => connection.push_events.as_mut(),
            Self::Disconnected => None,
        }
    }

    pub(crate) fn set_connected(&mut self, connection: AccountConnection) {
        *self = Self::Connected(connection);
    }

    pub(crate) fn disconnect(&mut self) {
        *self = Self::Disconnected;
    }
}

/// Secret resolver for provider accounts: reads the referenced secret from
/// the secret store on every resolve, so a credential rotated in the
/// keychain is picked up at the next (re)connect. OAuth access-token
/// refresh is not performed here; the stored secret is returned as-is.
struct StoreSecretResolver {
    secret_store: Arc<dyn SecretStore>,
    secret_ref: SecretRef,
}

impl std::fmt::Debug for StoreSecretResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreSecretResolver")
            .field("key", &self.secret_ref.key)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl SecretResolver for StoreSecretResolver {
    async fn resolve_secret(&self) -> Result<String, GatewayError> {
        self.secret_store
            .resolve(&self.secret_ref)
            .map_err(|error| GatewayError::Unavailable(error.to_string()))
    }
}

fn secret_resolver_for(
    account: &AccountSettings,
    secret_store: &Arc<dyn SecretStore>,
) -> Result<Arc<dyn SecretResolver>, ServiceError> {
    let secret_ref = account.transport.secret_ref.clone().ok_or_else(|| {
        ServiceError::from(GatewayError::Rejected(
            "missing account secret reference".to_string(),
        ))
    })?;
    Ok(Arc::new(StoreSecretResolver {
        secret_store: Arc::clone(secret_store),
        secret_ref,
    }))
}

/// Build a gateway connection for an account, resolving its secret and
/// opening a resilient push stream where the provider supports one.
pub(crate) async fn build_connection(
    account: &AccountSettings,
    secret_store: &Arc<dyn SecretStore>,
    store: &Arc<dyn MailStore>,
) -> Result<AccountConnection, ServiceError> {
    match account.driver {
        AccountDriver::Mock => Ok(AccountConnection {
            gateway: Arc::new(MockJmapGateway::default()),
            push_events: None,
            remote_observation: RemoteObservationPolicy::disabled(),
            push_unsupported: true,
        }),
        AccountDriver::Jmap => {
            let url = account
                .transport
                .base_url
                .as_deref()
                .ok_or_else(|| GatewayError::Rejected("missing JMAP base URL".to_string()))?;
            let username = account
                .transport
                .username
                .as_deref()
                .map(str::trim)
                .filter(|username| !username.is_empty());
            let secret_resolver = secret_resolver_for(account, secret_store)?;
            ph_info!(
                events::SUPERVISOR_GATEWAY_CONNECTING,
                account_id = %account.id,
                driver = "jmap",
                target_url = url,
                has_username = username.is_some(),
                "connecting account gateway"
            );
            let secret = secret_resolver.resolve_secret().await?;
            let client = connect_jmap_client(url, username, &secret).await?;
            let gateway: SharedGateway = Arc::new(LiveJmapGateway::from_client(client));

            let mut transports = gateway.push_transports().into_iter();
            let primary = transports.next();
            let fallback = transports.next();
            ph_info!(
                events::PUSH_TRANSPORT_NEGOTIATED,
                account_id = %account.id,
                primary = primary.as_ref().map(|t| t.name()),
                fallback = fallback.as_ref().map(|t| t.name()),
                "push transport negotiation complete"
            );

            let push_unsupported = primary.is_none();
            let push_events = primary.map(|primary| {
                resilient_push_stream(
                    account.id.clone(),
                    primary,
                    fallback,
                    ResilientPushConfig::default(),
                )
            });

            Ok(AccountConnection {
                gateway,
                push_events,
                remote_observation: account
                    .transport
                    .provider_profile()
                    .jmap()
                    .remote_observation(),
                push_unsupported,
            })
        }
        AccountDriver::ImapSmtp => {
            let secret_resolver = secret_resolver_for(account, secret_store)?;
            let secret = secret_resolver.resolve_secret().await?;
            let imap_config =
                ImapConnectionConfig::from_account_transport(&account.transport, secret.clone())
                    .map_err(imap_adapter_error)?;
            let smtp_config = SmtpConnectionConfig::from_account_settings(account, secret)
                .map_err(imap_adapter_error)?;
            ph_info!(
                events::SUPERVISOR_GATEWAY_CONNECTING,
                account_id = %account.id,
                driver = "imap_smtp",
                imap_host = %imap_config.host,
                imap_port = imap_config.port,
                smtp_host = %smtp_config.host,
                smtp_port = smtp_config.port,
                "connecting account gateway"
            );
            let gateway = LiveImapSmtpGateway::connect(
                imap_config,
                smtp_config,
                Some(Arc::clone(store)),
                Arc::clone(&secret_resolver),
            )
            .await
            .map_err(imap_adapter_error)?;
            let remote_observation = gateway
                .discovery()
                .provider_profile()
                .imap()
                .remote_observation();
            let idle_mailbox_name =
                if remote_observation.idle_scope() == RemoteIdleScope::SelectedMailbox {
                    gateway
                        .discovery()
                        .mailboxes
                        .iter()
                        .find(|mailbox| mailbox.selectable && mailbox.role == Some("inbox"))
                        .or_else(|| {
                            gateway
                                .discovery()
                                .mailboxes
                                .iter()
                                .find(|mailbox| mailbox.selectable)
                        })
                        .map(|mailbox| mailbox.name.clone())
                } else {
                    None
                };
            ph_info!(
                events::IMAP_DISCOVERY_COMPLETED,
                account_id = %account.id,
                mailbox_count = gateway.discovery().mailboxes.len(),
                "IMAP discovery complete"
            );
            let mut push_unsupported = false;
            let push_events = if gateway.discovery().capabilities.supports_idle() {
                if let Some(mailbox_name) = idle_mailbox_name {
                    ph_info!(
                        events::IMAP_IDLE_PUSH_ENABLED,
                        account_id = %account.id,
                        mailbox_name,
                        "IMAP IDLE push hint enabled"
                    );
                    // IDLE rides the gateway's single managed session: it
                    // recalls/re-issues around operations instead of opening
                    // a connection of its own.
                    Some(gateway.idle_event_stream(account.id.clone(), mailbox_name))
                } else {
                    ph_warn!(
                        events::IMAP_IDLE_MAILBOX_MISSING,
                        account_id = %account.id,
                        "IMAP IDLE advertised but no selectable mailbox is available"
                    );
                    push_unsupported = true;
                    None
                }
            } else {
                ph_info!(
                    events::IMAP_IDLE_PERIODIC_POLL_ONLY,
                    account_id = %account.id,
                    "IMAP IDLE unavailable; using periodic poll only"
                );
                push_unsupported = true;
                None
            };
            Ok(AccountConnection {
                gateway: Arc::new(gateway),
                push_events,
                remote_observation,
                push_unsupported,
            })
        }
    }
}

/// Map an IMAP adapter error into the domain's gateway error taxonomy. A
/// send whose fate is unknown (post-DATA transport drop) stays
/// dispatch-uncertain so the outbox parks it rather than blind-resending
/// into a duplicate delivery.
pub(crate) fn imap_adapter_error(error: ImapAdapterError) -> ServiceError {
    match error {
        ImapAdapterError::MissingTransport
        | ImapAdapterError::MissingSmtpTransport
        | ImapAdapterError::MissingUsername
        | ImapAdapterError::MissingSmtpSenderEmail
        | ImapAdapterError::MissingSecret
        | ImapAdapterError::InvalidMailboxName(_)
        | ImapAdapterError::MissingSelectData(_)
        | ImapAdapterError::UidValidityMismatch { .. }
        | ImapAdapterError::MissingFetchData(_)
        | ImapAdapterError::InvalidUidSequence(_)
        | ImapAdapterError::InvalidModSeq(_)
        | ImapAdapterError::InvalidKeywordFlag { .. }
        | ImapAdapterError::MissingMessageLocation(_)
        | ImapAdapterError::InvalidBlobId(_)
        | ImapAdapterError::ParseMessageHeaders
        | ImapAdapterError::ParseMessageBody
        | ImapAdapterError::MissingAttachment { .. }
        | ImapAdapterError::InvalidSmtpAddress { .. }
        | ImapAdapterError::BuildSmtpMessage(_) => GatewayError::Rejected(error.to_string()).into(),
        ImapAdapterError::Auth(_) => GatewayError::Auth.into(),
        ImapAdapterError::Timeout { operation } => {
            GatewayError::Network(format!("{operation} timed out")).into()
        }
        ImapAdapterError::Client(message) | ImapAdapterError::Smtp(message) => {
            GatewayError::Network(message).into()
        }
        ImapAdapterError::SmtpDispatchUncertain(message) => {
            GatewayError::DispatchUncertain(message).into()
        }
    }
}
