//! OAuth access-token refresh behind the gateway's secret resolver: the
//! stored token-set format, the expiry check with a skew margin, a
//! per-secret single-flight that serializes concurrent refreshes, and
//! compare-and-swap persistence of a rotated token set. Password and
//! app-password accounts bypass all of it and read the stored secret as-is.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use posthaste_domain_model::{GatewayError, ProviderAuthKind, ProviderHint, SecretRef};
use posthaste_domain_service::{SecretCasOutcome, SecretResolver, SecretStore};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

/// An access token this close to expiry is refreshed up front, so a token
/// that would die mid-session is never handed to a provider connection.
const OAUTH_REFRESH_SKEW_SECONDS: i64 = 300;

/// The stored account secret for an OAuth account: the whole token bundle,
/// JSON-encoded, so a refresh can mint new access tokens without
/// re-authorizing. Resolved only inside the backend and reduced to the
/// short-lived access token before any provider session sees it; never
/// returned by any API answer.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OauthTokenSet {
    #[serde(default = "oauth_secret_type")]
    pub(crate) r#type: String,
    pub(crate) provider: ProviderHint,
    pub(crate) client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) client_secret: Option<String>,
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_at: Option<String>,
    #[serde(default)]
    pub(crate) scopes: Vec<String>,
}

impl std::fmt::Debug for OauthTokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OauthTokenSet")
            .field("type", &self.r#type)
            .field("provider", &self.provider)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[redacted]"),
            )
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish()
    }
}

impl OauthTokenSet {
    pub(crate) fn decode(secret: &str) -> Result<Self, GatewayError> {
        let token_set: Self = serde_json::from_str(secret).map_err(|error| {
            GatewayError::Rejected(format!("invalid OAuth token secret: {error}"))
        })?;
        if token_set.r#type != "oauth2" {
            return Err(GatewayError::Rejected(format!(
                "invalid OAuth token secret type: {}",
                token_set.r#type
            )));
        }
        Ok(token_set)
    }

    pub(crate) fn encode(&self) -> Result<String, GatewayError> {
        serde_json::to_string(self)
            .map_err(|error| GatewayError::Rejected(format!("invalid OAuth token secret: {error}")))
    }

    fn expires_at(&self) -> Result<Option<OffsetDateTime>, GatewayError> {
        self.expires_at
            .as_deref()
            .map(|expires_at| {
                OffsetDateTime::parse(expires_at, &Rfc3339).map_err(|error| {
                    GatewayError::Rejected(format!("invalid OAuth token expiry: {error}"))
                })
            })
            .transpose()
    }

    /// Whether the access token is expired — or close enough to expiry that
    /// handing it out risks a mid-session death. A token set without an
    /// expiry never proactively refreshes.
    fn requires_refresh_at(&self, now: OffsetDateTime) -> Result<bool, GatewayError> {
        let Some(expires_at) = self.expires_at()? else {
            return Ok(false);
        };
        Ok(expires_at <= now + Duration::seconds(OAUTH_REFRESH_SKEW_SECONDS))
    }
}

pub(crate) fn oauth_secret_type() -> String {
    "oauth2".to_string()
}

/// The provider token-endpoint exchange: take the current token set, return
/// a rotated one. Abstracted so resolver tests can fake the endpoint.
#[async_trait::async_trait]
pub(crate) trait TokenRefresher: Send + Sync {
    async fn refresh(
        &self,
        token_set: &OauthTokenSet,
        now: OffsetDateTime,
    ) -> Result<OauthTokenSet, GatewayError>;
}

/// Live refresher over the bundled provider registration's token endpoint.
struct ProviderTokenRefresher;

#[async_trait::async_trait]
impl TokenRefresher for ProviderTokenRefresher {
    async fn refresh(
        &self,
        token_set: &OauthTokenSet,
        now: OffsetDateTime,
    ) -> Result<OauthTokenSet, GatewayError> {
        crate::api::oauth::refresh_token_set(token_set, now).await
    }
}

/// Wall-clock source, injectable so tests drive expiry with a fake clock.
type Clock = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;

/// Per-secret single-flight locks: concurrent resolves of the same secret
/// serialize here, and each flight re-reads the stored token set inside the
/// lock, so one refresh serves them all and a follower can never refresh —
/// and thereby consume — a grant its predecessor already rotated.
#[derive(Default)]
pub(crate) struct RefreshFlights {
    flights: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl RefreshFlights {
    /// The process-wide flight map: every gateway build in this process,
    /// whatever its call site, serializes refreshes through it.
    fn global() -> Arc<Self> {
        static GLOBAL: LazyLock<Arc<RefreshFlights>> =
            LazyLock::new(|| Arc::new(RefreshFlights::default()));
        Arc::clone(&GLOBAL)
    }

    async fn for_key(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut flights = self.flights.lock().await;
        Arc::clone(flights.entry(key.to_string()).or_default())
    }
}

/// Secret resolver for provider accounts: reads the referenced secret from
/// the secret store on every resolve, so a credential rotated in the
/// keychain is picked up at the next (re)connect. Password and app-password
/// accounts return the stored secret verbatim; OAuth accounts return the
/// access token from the stored token set, refreshing it through the
/// provider's token endpoint first when it is at or near expiry.
pub(crate) struct RefreshingSecretResolver {
    secret_store: Arc<dyn SecretStore>,
    secret_ref: SecretRef,
    auth: ProviderAuthKind,
    refresher: Arc<dyn TokenRefresher>,
    clock: Clock,
    flights: Arc<RefreshFlights>,
}

impl RefreshingSecretResolver {
    /// Production resolver: live token endpoint, real clock, the
    /// process-wide flight map.
    pub(crate) fn for_account(
        auth: ProviderAuthKind,
        secret_ref: SecretRef,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self::new(
            auth,
            secret_ref,
            secret_store,
            Arc::new(ProviderTokenRefresher),
            Arc::new(OffsetDateTime::now_utc),
            RefreshFlights::global(),
        )
    }

    fn new(
        auth: ProviderAuthKind,
        secret_ref: SecretRef,
        secret_store: Arc<dyn SecretStore>,
        refresher: Arc<dyn TokenRefresher>,
        clock: Clock,
        flights: Arc<RefreshFlights>,
    ) -> Self {
        Self {
            secret_store,
            secret_ref,
            auth,
            refresher,
            clock,
            flights,
        }
    }

    fn read_stored(&self) -> Result<String, GatewayError> {
        self.secret_store
            .resolve(&self.secret_ref)
            .map_err(|error| GatewayError::Unavailable(error.to_string()))
    }
}

impl std::fmt::Debug for RefreshingSecretResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshingSecretResolver")
            .field("key", &self.secret_ref.key)
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl SecretResolver for RefreshingSecretResolver {
    async fn resolve_secret(&self) -> Result<String, GatewayError> {
        // Fast path: a non-OAuth credential is the secret, returned as-is.
        if self.auth != ProviderAuthKind::OAuth2 {
            return self.read_stored();
        }

        // Single-flight per secret: the stored set is re-read inside the
        // lock, so a follower that queued behind a refresh sees the rotated
        // set already fresh and returns it without a second exchange.
        let flight = self.flights.for_key(&self.secret_ref.key).await;
        let _guard = flight.lock().await;

        let stored = self.read_stored()?;
        let token_set = OauthTokenSet::decode(&stored)?;
        let now = (self.clock)();
        if !token_set.requires_refresh_at(now)? {
            return Ok(token_set.access_token);
        }

        let rotated = self.refresher.refresh(&token_set, now).await?;
        let outcome = self
            .secret_store
            .update_if_unchanged(&self.secret_ref, &stored, &rotated.encode()?)
            .map_err(|error| GatewayError::Unavailable(error.to_string()))?;
        match outcome {
            SecretCasOutcome::Swapped => Ok(rotated.access_token),
            SecretCasOutcome::Mismatch { current } => {
                // Another writer (e.g. a second posthaste process sharing the
                // keychain) rotated the stored set during this exchange.
                // Persisting ours would clobber a refresh token the provider
                // may have already consumed; adopt the winner's instead.
                tracing::warn!(
                    key = %self.secret_ref.key,
                    "OAuth refresh CAS-miss: stored token set was rotated concurrently; adopting the winner's token"
                );
                let winner = OauthTokenSet::decode(&current)?;
                Ok(winner.access_token)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use posthaste_domain_model::SecretKind;
    use posthaste_testkit::TestSecretStore;

    use super::*;

    type RefreshFn = Box<
        dyn Fn(&OauthTokenSet, OffsetDateTime) -> Result<OauthTokenSet, GatewayError> + Send + Sync,
    >;

    /// Fake token endpoint: counts calls, optionally dwells like a network
    /// round-trip, then applies the configured rotation (or failure).
    struct FakeRefresher {
        calls: AtomicUsize,
        dwell: std::time::Duration,
        respond: RefreshFn,
    }

    impl FakeRefresher {
        fn new(respond: RefreshFn) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                dwell: std::time::Duration::ZERO,
                respond,
            })
        }

        fn slow(respond: RefreshFn, dwell: std::time::Duration) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                dwell,
                respond,
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl TokenRefresher for FakeRefresher {
        async fn refresh(
            &self,
            token_set: &OauthTokenSet,
            now: OffsetDateTime,
        ) -> Result<OauthTokenSet, GatewayError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.dwell.is_zero() {
                tokio::time::sleep(self.dwell).await;
            }
            (self.respond)(token_set, now)
        }
    }

    fn fixed_now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("valid timestamp")
    }

    fn rfc3339(at: OffsetDateTime) -> String {
        at.format(&Rfc3339).expect("formats")
    }

    fn token_set(access: &str, expires_at: Option<OffsetDateTime>) -> OauthTokenSet {
        OauthTokenSet {
            r#type: oauth_secret_type(),
            provider: ProviderHint::Gmail,
            client_id: "client-id".to_string(),
            client_secret: None,
            access_token: access.to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at: expires_at.map(rfc3339),
            scopes: vec!["https://mail.google.com/".to_string()],
        }
    }

    fn rotation(access: &'static str, refresh: &'static str) -> RefreshFn {
        Box::new(move |current, now| {
            Ok(OauthTokenSet {
                access_token: access.to_string(),
                refresh_token: Some(refresh.to_string()),
                expires_at: Some(rfc3339(now + Duration::seconds(3600))),
                ..current.clone()
            })
        })
    }

    fn secret_ref(key: &str) -> SecretRef {
        SecretRef {
            kind: SecretKind::Os,
            key: key.to_string(),
        }
    }

    struct Harness {
        store: Arc<TestSecretStore>,
        secret_ref: SecretRef,
        refresher: Arc<FakeRefresher>,
        resolver: Arc<RefreshingSecretResolver>,
    }

    fn harness(
        key: &str,
        auth: ProviderAuthKind,
        stored: &str,
        refresher: Arc<FakeRefresher>,
    ) -> Harness {
        let store = Arc::new(TestSecretStore::default());
        let secret_ref = secret_ref(key);
        store.save(&secret_ref, stored).expect("seed secret");
        let resolver = Arc::new(RefreshingSecretResolver::new(
            auth,
            secret_ref.clone(),
            store.clone(),
            refresher.clone(),
            Arc::new(fixed_now),
            Arc::new(RefreshFlights::default()),
        ));
        Harness {
            store,
            secret_ref,
            refresher,
            resolver,
        }
    }

    #[tokio::test]
    async fn password_account_returns_the_stored_secret_verbatim() {
        let refresher = FakeRefresher::new(rotation("unused", "unused"));
        let harness = harness("acct-pw", ProviderAuthKind::Password, "hunter2", refresher);

        let secret = harness.resolver.resolve_secret().await.expect("resolves");

        assert_eq!(secret, "hunter2");
        assert_eq!(harness.refresher.calls(), 0);
    }

    #[tokio::test]
    async fn fresh_oauth_token_returns_the_access_token_without_refreshing() {
        let stored = token_set("live-token", Some(fixed_now() + Duration::seconds(3600)))
            .encode()
            .expect("encodes");
        let refresher = FakeRefresher::new(rotation("unused", "unused"));
        let harness = harness("acct-fresh", ProviderAuthKind::OAuth2, &stored, refresher);

        let secret = harness.resolver.resolve_secret().await.expect("resolves");

        assert_eq!(secret, "live-token");
        assert_eq!(harness.refresher.calls(), 0);
    }

    #[tokio::test]
    async fn expired_oauth_token_refreshes_and_persists_the_rotated_set() {
        let stored = token_set("stale-token", Some(fixed_now() - Duration::seconds(60)))
            .encode()
            .expect("encodes");
        let refresher = FakeRefresher::new(rotation("new-token", "new-refresh"));
        let harness = harness("acct-expired", ProviderAuthKind::OAuth2, &stored, refresher);

        let secret = harness.resolver.resolve_secret().await.expect("resolves");

        assert_eq!(secret, "new-token");
        assert_eq!(harness.refresher.calls(), 1);
        let persisted = OauthTokenSet::decode(
            &harness
                .store
                .resolve(&harness.secret_ref)
                .expect("stored secret"),
        )
        .expect("decodes");
        assert_eq!(persisted.access_token, "new-token");
        assert_eq!(persisted.refresh_token.as_deref(), Some("new-refresh"));
        assert_eq!(
            persisted.expires_at,
            Some(rfc3339(fixed_now() + Duration::seconds(3600)))
        );
    }

    #[tokio::test]
    async fn token_inside_the_skew_margin_refreshes_proactively() {
        let stored = token_set("almost-dead", Some(fixed_now() + Duration::seconds(60)))
            .encode()
            .expect("encodes");
        let refresher = FakeRefresher::new(rotation("new-token", "new-refresh"));
        let harness = harness("acct-skew", ProviderAuthKind::OAuth2, &stored, refresher);

        let secret = harness.resolver.resolve_secret().await.expect("resolves");

        assert_eq!(secret, "new-token");
        assert_eq!(harness.refresher.calls(), 1);
    }

    #[tokio::test]
    async fn concurrent_resolves_share_a_single_refresh() {
        let stored = token_set("stale-token", Some(fixed_now() - Duration::seconds(60)))
            .encode()
            .expect("encodes");
        let refresher = FakeRefresher::slow(
            rotation("new-token", "new-refresh"),
            std::time::Duration::from_millis(50),
        );
        let harness = harness(
            "acct-concurrent",
            ProviderAuthKind::OAuth2,
            &stored,
            refresher,
        );

        let mut tasks = Vec::new();
        for _ in 0..8 {
            let resolver = harness.resolver.clone();
            tasks.push(tokio::spawn(async move { resolver.resolve_secret().await }));
        }
        for task in tasks {
            let secret = task
                .await
                .expect("task joins")
                .expect("every resolve succeeds");
            assert_eq!(secret, "new-token");
        }
        assert_eq!(
            harness.refresher.calls(),
            1,
            "followers must adopt the flight leader's rotation, not re-refresh"
        );
    }

    #[tokio::test]
    async fn refresh_failure_surfaces_the_typed_auth_error() {
        let stored = token_set("stale-token", Some(fixed_now() - Duration::seconds(60)))
            .encode()
            .expect("encodes");
        let refresher = FakeRefresher::new(Box::new(|_, _| Err(GatewayError::Auth)));
        let harness = harness("acct-fail", ProviderAuthKind::OAuth2, &stored, refresher);

        let error = harness
            .resolver
            .resolve_secret()
            .await
            .expect_err("refresh failure propagates");

        assert!(matches!(error, GatewayError::Auth));
        // The stale set stays put: nothing was rotated, nothing clobbered.
        assert_eq!(
            harness
                .store
                .resolve(&harness.secret_ref)
                .expect("stored secret"),
            stored
        );
    }

    #[tokio::test]
    async fn cas_miss_adopts_the_concurrently_rotated_token() {
        let stored = token_set("stale-token", Some(fixed_now() - Duration::seconds(60)))
            .encode()
            .expect("encodes");
        let store = Arc::new(TestSecretStore::default());
        let winner_ref = secret_ref("acct-cas");
        store.save(&winner_ref, &stored).expect("seed secret");

        // The "endpoint" simulates an external process winning the rotation
        // mid-exchange: it rewrites the stored set before answering.
        let race_store = store.clone();
        let race_ref = winner_ref.clone();
        let refresher = FakeRefresher::new(Box::new(move |current, now| {
            let winner = OauthTokenSet {
                access_token: "winner-token".to_string(),
                refresh_token: Some("winner-refresh".to_string()),
                expires_at: Some(rfc3339(now + Duration::seconds(3600))),
                ..current.clone()
            };
            race_store
                .update(&race_ref, &winner.encode()?)
                .expect("winner write");
            Ok(OauthTokenSet {
                access_token: "loser-token".to_string(),
                refresh_token: Some("loser-refresh".to_string()),
                expires_at: Some(rfc3339(now + Duration::seconds(3600))),
                ..current.clone()
            })
        }));
        let resolver = RefreshingSecretResolver::new(
            ProviderAuthKind::OAuth2,
            winner_ref.clone(),
            store.clone(),
            refresher,
            Arc::new(fixed_now),
            Arc::new(RefreshFlights::default()),
        );

        let secret = resolver.resolve_secret().await.expect("resolves");

        assert_eq!(secret, "winner-token");
        let persisted = OauthTokenSet::decode(&store.resolve(&winner_ref).expect("stored secret"))
            .expect("decodes");
        assert_eq!(
            persisted.refresh_token.as_deref(),
            Some("winner-refresh"),
            "the loser's set must not clobber the winner's rotation"
        );
    }
}
