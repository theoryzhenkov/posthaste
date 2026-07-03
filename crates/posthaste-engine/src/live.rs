use std::sync::Arc;

use jmap_client::client::{Client, Credentials};
use jmap_client::core::error::MethodErrorType;
use jmap_client::mailbox;
use posthaste_domain_model::{GatewayError, MailboxId};
use posthaste_observability::{events, ph_debug, ph_info};
use posthaste_provider_call::{
    CallErrorReason, ExecutorConfig, ProviderCallError, ProviderCallExecutor, METADATA_TOTAL,
};

mod gateway;

/// Discover and connect to a JMAP server, returning a configured client.
///
/// Performs session discovery via `.well-known/jmap`, authenticates with
/// the configured account secret, and follows redirects scoped to the
/// server's host.
///
/// @spec docs/L1-jmap#session
/// @spec docs/L1-jmap#authentication
pub async fn connect_jmap_client(
    url: &str,
    username: Option<&str>,
    secret: &str,
) -> Result<Arc<Client>, GatewayError> {
    ph_debug!(events::JMAP_SESSION_CONNECTING, "connecting to JMAP server");
    let host = url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(String::from))
        .unwrap_or_default();
    let client = Client::new()
        .credentials(jmap_credentials(username, secret))
        .follow_redirects([host])
        // Retire jmap-client's 10 s default (F2) for the calls that still route
        // through its own internal reqwest client (the typed `send()` HTTP path,
        // sync convenience calls, upload). The engine-controlled raw POST and
        // blob download now bypass this via the provider-call executor, which
        // applies the per-class deadline instead; this bump only governs the
        // residual jmap-client-internal calls.
        .timeout(METADATA_TOTAL)
        .connect(url)
        .await
        .map_err(map_gateway_error)?;
    let session = client.session();
    let ws_url = session
        .websocket_capabilities()
        .map(|caps| caps.url().to_string());
    let ws_push = session
        .websocket_capabilities()
        .map(|caps| caps.supports_push())
        .unwrap_or(false);
    ph_info!(
        events::JMAP_SESSION_ESTABLISHED,
        api_url = session.api_url(),
        event_source_url = session.event_source_url(),
        ws_url = ws_url.as_deref(),
        ws_push,
        "JMAP session established"
    );
    Ok(Arc::new(client))
}

/// Production `MailGateway` backed by a live JMAP server connection.
///
/// Holds an authenticated `jmap_client::Client` and, when the server
/// advertises WebSocket push support, a `SharedWsConnection` used for
/// interactive API calls and push notifications.
///
/// @spec docs/L1-jmap#session
/// @spec docs/L2-transport#transport-negotiation
pub struct LiveJmapGateway {
    client: Arc<Client>,
    ws: Option<Arc<crate::ws_connection::SharedWsConnection>>,
    /// The native outbound-call envelope (M31): the engine-controlled raw JMAP
    /// POST and blob download route through this for per-class deadlines, the
    /// `Retry-After` retry loop, and the per-account circuit breaker. `None` only
    /// if the shared client failed to build, in which case those two call sites
    /// fall back to their prior direct-reqwest / `client.download()` path.
    executor: Option<Arc<ProviderCallExecutor>>,
    /// The per-account breaker key (the server-side JMAP account id).
    account_key: String,
}

impl LiveJmapGateway {
    /// Wrap an already-connected client, opening a WebSocket if the server supports push.
    ///
    /// @spec docs/L2-transport#transport-negotiation
    pub fn from_client(client: Arc<Client>) -> Self {
        let ws = if client
            .session()
            .websocket_capabilities()
            .map(|capabilities| capabilities.supports_push())
            .unwrap_or(false)
        {
            ph_debug!(
                events::JMAP_WEBSOCKET_CAPABILITY_AVAILABLE,
                "WebSocket push capability available, creating shared connection"
            );
            Some(Arc::new(crate::ws_connection::SharedWsConnection::new(
                client.clone(),
            )))
        } else {
            ph_debug!(
                events::JMAP_WEBSOCKET_CAPABILITY_UNAVAILABLE,
                "WebSocket push capability not advertised, WS transport disabled"
            );
            None
        };
        let account_key = client.default_account_id().to_string();
        let executor = build_executor(&client);
        Self {
            client,
            ws,
            executor,
            account_key,
        }
    }

    /// Discover, authenticate, and construct a gateway in one step.
    ///
    /// @spec docs/L1-jmap#session
    pub async fn connect(
        url: &str,
        username: Option<&str>,
        secret: &str,
    ) -> Result<Self, GatewayError> {
        let client = connect_jmap_client(url, username, secret).await?;
        Ok(Self::from_client(client))
    }

    /// Borrow the underlying JMAP client for direct access.
    pub fn client(&self) -> &Arc<Client> {
        &self.client
    }

    /// JMAP account id selected from the server session.
    pub(crate) fn server_account_id(&self) -> &str {
        self.client.default_account_id()
    }

    /// Route a JMAP request through WebSocket if connected, HTTP otherwise.
    ///
    /// Currently only interactive methods (mutations, body fetch, identity,
    /// compose) use this. Sync helpers still use HTTP convenience methods
    /// directly. TODO: once jmap-client supports transparent WS routing in
    /// Client::send(), all paths will use WS automatically and this method
    /// can be removed.
    ///
    /// @spec docs/L2-transport#jmaptransport
    /// @spec docs/L2-transport#http-fallback
    pub(crate) async fn send_request(
        &self,
        request: jmap_client::core::request::Request<'_>,
    ) -> Result<
        jmap_client::core::response::Response<jmap_client::core::response::TaggedMethodResponse>,
        GatewayError,
    > {
        if let Some(ref ws) = self.ws {
            if ws.is_connected().await {
                return ws.send(request).await;
            }
        }
        request.send().await.map_err(map_gateway_error)
    }

    pub(crate) async fn fetch_mailbox_id_by_role(
        &self,
        role: mailbox::Role,
    ) -> Result<MailboxId, GatewayError> {
        let mut request = self.client.build();
        request.get_mailbox().properties([
            mailbox::Property::Id,
            mailbox::Property::Name,
            mailbox::Property::Role,
        ]);
        let mut response = self.send_request(request).await?;
        let mailboxes = required_method_response(response.pop_method_response(), "Mailbox/get")?
            .unwrap_get_mailbox()
            .map_err(map_gateway_error)?
            .take_list();

        mailboxes
            .into_iter()
            .find(|mailbox| mailbox.role() == role)
            .and_then(|mailbox| mailbox.id().map(|id| MailboxId::from(id.to_string())))
            .ok_or_else(|| {
                GatewayError::Rejected(format!("required {role:?} mailbox not available"))
            })
    }

    pub(crate) fn ws(&self) -> Option<&Arc<crate::ws_connection::SharedWsConnection>> {
        self.ws.as_ref()
    }

    /// The provider-call envelope, if it built. Call sites route through it when
    /// present and fall back to their direct path otherwise.
    pub(crate) fn executor(&self) -> Option<&Arc<ProviderCallExecutor>> {
        self.executor.as_ref()
    }

    /// The per-account circuit-breaker key (server-side JMAP account id).
    pub(crate) fn account_key(&self) -> &str {
        &self.account_key
    }
}

/// Build the shared outbound-call executor for a connected client, trusting the
/// session's API host for redirects (mirroring the connect-time follow-redirects
/// scoping). Returns `None` if the shared client cannot be built.
fn build_executor(client: &Client) -> Option<Arc<ProviderCallExecutor>> {
    let trusted_hosts = url::Url::parse(client.session().api_url())
        .ok()
        .and_then(|parsed| parsed.host_str().map(String::from))
        .into_iter()
        .collect();
    let config = ExecutorConfig {
        trusted_hosts,
        ..ExecutorConfig::default()
    };
    ProviderCallExecutor::new(config).ok().map(Arc::new)
}

/// Map a provider-call envelope error into the engine's `GatewayError`,
/// preserving the auth and circuit-open distinctions callers rely on.
pub(crate) fn map_provider_error(error: ProviderCallError) -> GatewayError {
    match error.reason {
        // A 401 stays an auth failure (drives re-auth, not a network retry).
        CallErrorReason::Http(401) => GatewayError::Auth,
        // An open breaker surfaces as gateway-unavailable so status can show
        // "provider circuit open" rather than a generic network error (D83).
        CallErrorReason::CircuitOpen => GatewayError::Unavailable(error.detail),
        _ => GatewayError::Network(error.detail),
    }
}

pub(crate) fn required_method_response<T>(
    response: Option<T>,
    method: &str,
) -> Result<T, GatewayError> {
    response.ok_or_else(|| GatewayError::Rejected(format!("{method} response missing")))
}
/// Map `jmap_client::Error` into the typed `GatewayError` enum.
///
/// Distinguishes auth errors (401), state mismatches, `cannotCalculateChanges`,
/// and generic network/method errors.
///
/// @spec docs/L1-jmap#error-model
pub(crate) fn map_gateway_error(error: jmap_client::Error) -> GatewayError {
    match error {
        jmap_client::Error::Problem(problem) => {
            if problem.status == Some(401) {
                GatewayError::Auth
            } else {
                GatewayError::Network(problem.to_string())
            }
        }
        jmap_client::Error::Method(method) => match method.p_type {
            MethodErrorType::StateMismatch => GatewayError::StateMismatch,
            MethodErrorType::CannotCalculateChanges => GatewayError::CannotCalculateChanges,
            _ => GatewayError::Rejected(method.to_string()),
        },
        jmap_client::Error::Set(error) => GatewayError::Rejected(error.to_string()),
        other => GatewayError::Network(other.to_string()),
    }
}

fn jmap_credentials(username: Option<&str>, secret: &str) -> Credentials {
    username
        .map(str::trim)
        .filter(|username| !username.is_empty())
        .map(|username| Credentials::basic(username, secret))
        .unwrap_or_else(|| Credentials::bearer(secret))
}

#[cfg(test)]
mod tests;
