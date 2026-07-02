use super::*;

/// Lazily establish the gateway connection and push stream if not already
/// connected.
pub(crate) async fn ensure_connection(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: RuntimeGeneration,
    connection: &mut AccountRuntimeConnectionState,
) -> Result<(), ServiceError> {
    if connection.is_connected() {
        return Ok(());
    }
    ph_debug!(
        events::SUPERVISOR_CONNECTION_ESTABLISHING,
        account_id = %account.id,
        "establishing connection"
    );
    let conn = build_connection(account, shared, Some(generation)).await?;
    shared.set_gateway(&account.id, conn.gateway.clone()).await;
    connection.set_connected(conn);
    ph_info!(
        events::SUPERVISOR_CONNECTION_ESTABLISHED,
        account_id = %account.id,
        "connection established"
    );
    Ok(())
}

/// Secret resolver for IMAP/SMTP accounts.
///
/// Password and app-password accounts use a static resolver: the secret is read
/// once from the secret store and returned unchanged for every connection.
/// OAuth accounts dynamically refresh the short-lived access token through the
/// provider's token endpoint before each connection, updating the persisted
/// token set when a new refresh token is issued.
struct AccountSecretResolver {
    shared: Arc<SupervisorShared>,
    account: AccountSettings,
}

impl AccountSecretResolver {
    fn new(shared: Arc<SupervisorShared>, account: AccountSettings) -> Self {
        Self { shared, account }
    }
}

impl std::fmt::Debug for AccountSecretResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountSecretResolver")
            .field("account_id", &self.account.id)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl SecretResolver for AccountSecretResolver {
    async fn resolve_secret(&self) -> Result<String, GatewayError> {
        let secret_ref = self.account.transport.secret_ref.as_ref().ok_or_else(|| {
            GatewayError::Rejected("missing account secret reference".to_string())
        })?;
        let secret = resolve_account_secret(&self.account, &self.shared, secret_ref)
            .await
            .map_err(|error| match error {
                ServiceError::Gateway(gateway_error) => gateway_error,
                other => GatewayError::Rejected(other.to_string()),
            })?;
        Ok(secret)
    }
}

/// Build a gateway connection for an account, resolving its secret and
/// opening a resilient push stream (WS preferred, SSE fallback).
///
/// @spec docs/L2-transport#transport-negotiation
pub(crate) async fn build_connection(
    account: &AccountSettings,
    shared: &Arc<SupervisorShared>,
    generation: Option<RuntimeGeneration>,
) -> Result<AccountConnection, ServiceError> {
    match account.driver {
        AccountDriver::Mock => Ok(AccountConnection {
            gateway: Arc::new(MockJmapGateway::default()),
            push_events: None,
            remote_observation: RemoteObservationPolicy::disabled(),
            secret_resolver: Arc::new(StaticSecretResolver::new("")),
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
            let secret_resolver: Arc<dyn SecretResolver> = Arc::new(AccountSecretResolver::new(
                Arc::clone(shared),
                account.clone(),
            ));
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

            let transports = gateway.push_transports();
            let mut transports = transports.into_iter();
            let primary = transports.next();
            let fallback = transports.next();

            ph_info!(
                events::PUSH_TRANSPORT_NEGOTIATED,
                account_id = %account.id,
                primary = primary.as_ref().map(|t| t.name()),
                fallback = fallback.as_ref().map(|t| t.name()),
                reason = if primary.as_ref().map(|t| t.name()) == Some("ws") {
                    "server advertises WebSocket push support"
                } else {
                    "WebSocket not available, SSE only"
                },
                "push transport negotiation complete"
            );

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
                secret_resolver,
            })
        }
        AccountDriver::ImapSmtp => {
            let secret_resolver: Arc<dyn SecretResolver> = Arc::new(AccountSecretResolver::new(
                Arc::clone(shared),
                account.clone(),
            ));
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
                imap_security = ?imap_config.security,
                smtp_host = %smtp_config.host,
                smtp_port = smtp_config.port,
                smtp_security = ?smtp_config.security,
                auth = ?imap_config.auth,
                "connecting account gateway"
            );
            let gateway = LiveImapSmtpGateway::connect(
                imap_config.clone(),
                smtp_config,
                Some(shared.store.clone()),
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
            let push_events = if gateway.discovery().capabilities.supports_idle() {
                if let Some(mailbox_name) = idle_mailbox_name {
                    ph_info!(
                        events::IMAP_IDLE_PUSH_ENABLED,
                        account_id = %account.id,
                        mailbox_name,
                        "IMAP IDLE push hint enabled"
                    );
                    Some(imap_idle_event_stream(
                        account.id.clone(),
                        imap_config,
                        mailbox_name,
                        Arc::clone(&secret_resolver),
                    ))
                } else {
                    ph_warn!(
                        events::IMAP_IDLE_MAILBOX_MISSING,
                        account_id = %account.id,
                        "IMAP IDLE advertised but no selectable mailbox is available"
                    );
                    if let Some(generation) = generation {
                        shared
                            .set_push_status(&account.id, generation, PushStatus::Unsupported)
                            .await;
                    }
                    None
                }
            } else {
                ph_info!(
                    events::IMAP_IDLE_PERIODIC_POLL_ONLY,
                    account_id = %account.id,
                    "IMAP IDLE unavailable; using periodic poll only"
                );
                if let Some(generation) = generation {
                    shared
                        .set_push_status(&account.id, generation, PushStatus::Unsupported)
                        .await;
                }
                None
            };
            Ok(AccountConnection {
                gateway: Arc::new(gateway),
                push_events,
                remote_observation,
                secret_resolver,
            })
        }
    }
}

pub(crate) async fn resolve_account_secret(
    account: &AccountSettings,
    shared: &Arc<SupervisorShared>,
    secret_ref: &posthaste_domain_model::SecretRef,
) -> Result<String, ServiceError> {
    let secret = shared.secret_store.resolve(secret_ref)?;
    if account.transport.auth != ProviderAuthKind::OAuth2 {
        return Ok(secret);
    }

    let token_set = OAuthTokenSet::decode(&secret)?;
    refresh_oauth_access_token(shared, secret_ref, &token_set).await
}

pub(crate) async fn refresh_oauth_access_token(
    shared: &Arc<SupervisorShared>,
    secret_ref: &posthaste_domain_model::SecretRef,
    token_set: &OAuthTokenSet,
) -> Result<String, ServiceError> {
    let token_service = OAuthTokenService::new()?;
    let access_token = token_service
        .access_token(token_set, time::OffsetDateTime::now_utc())
        .await?;
    if let Some(updated_token_set) = access_token.updated_token_set {
        shared
            .secret_store
            .update(secret_ref, &updated_token_set.encode()?)?;
    }

    Ok(access_token.token)
}

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
        ImapAdapterError::Client(message) | ImapAdapterError::Smtp(message) => {
            GatewayError::Network(message).into()
        }
    }
}
