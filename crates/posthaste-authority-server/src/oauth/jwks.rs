use super::*;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-metadata-URL JWKS coordination.
///
/// `fetch_lock` is the single-flight leader election: a burst of concurrent
/// cold- or expired-cache validations serializes here so exactly ONE
/// discovery + JWKS fetch reaches the IdP (cures the N13 stampede). `cached`
/// holds the last-good key set — reused by waiters and served stale on a fetch
/// failure. `fetch_gen` bumps on every successful fetch so a forced refresh can
/// tell whether a concurrent leader already refreshed while it waited.
#[derive(Default)]
pub(crate) struct JwksCacheEntry {
    fetch_lock: Mutex<()>,
    cached: StdMutex<Option<CachedJwks>>,
    fetch_gen: AtomicU64,
}

impl JwksCacheEntry {
    fn guard(&self) -> std::sync::MutexGuard<'_, Option<CachedJwks>> {
        self.cached.lock().expect("JWKS cache entry poisoned")
    }

    /// A key set still within its cache lifetime.
    fn fresh_jwks(&self, now: OffsetDateTime) -> Option<JwkSet> {
        self.guard()
            .as_ref()
            .filter(|cached| cached.expires_at > now)
            .map(|cached| cached.jwks.clone())
    }

    /// Whatever key set is cached, regardless of freshness.
    fn any_jwks(&self) -> Option<JwkSet> {
        self.guard().as_ref().map(|cached| cached.jwks.clone())
    }

    /// The last-good key set, served past its expiry only within the bounded
    /// stale window (the degrade-under-pressure fallback).
    fn stale_within_bound(&self, now: OffsetDateTime) -> Option<JwkSet> {
        let max_stale = Duration::seconds(OAUTH_JWKS_MAX_STALE_SECONDS);
        self.guard()
            .as_ref()
            .filter(|cached| now <= cached.expires_at + max_stale)
            .map(|cached| cached.jwks.clone())
    }

    fn store(&self, fetched: CachedJwks) {
        *self.guard() = Some(fetched);
        self.fetch_gen.fetch_add(1, Ordering::Release);
    }
}

static OAUTH_JWKS_CACHE: OnceLock<StdMutex<HashMap<&'static str, Arc<JwksCacheEntry>>>> =
    OnceLock::new();

fn jwks_cache_entry(metadata_url: &'static str) -> Arc<JwksCacheEntry> {
    OAUTH_JWKS_CACHE
        .get_or_init(|| StdMutex::new(HashMap::new()))
        .lock()
        .expect("JWKS cache map poisoned")
        .entry(metadata_url)
        .or_default()
        .clone()
}

impl OAuthTokenService {
    pub(super) async fn jwks_for_profile(
        &self,
        profile: &OAuthProviderProfile,
        now: OffsetDateTime,
        force_refresh: bool,
    ) -> Result<JwkSet, GatewayError> {
        let entry = jwks_cache_entry(profile.metadata_url);
        jwks_single_flight(&entry, now, force_refresh, || self.fetch_jwks(profile, now)).await
    }

    async fn fetch_jwks(
        &self,
        profile: &OAuthProviderProfile,
        now: OffsetDateTime,
    ) -> Result<CachedJwks, GatewayError> {
        let metadata = self
            .http_client
            .get(profile.metadata_url)
            .send()
            .await
            .map_err(oauth_request_error)?;
        if !metadata.status().is_success() {
            return Err(GatewayError::Network(format!(
                "OAuth metadata request failed with {}",
                metadata.status()
            )));
        }
        let metadata_body = metadata.text().await.map_err(oauth_request_error)?;
        let metadata: OpenIdProviderMetadata =
            serde_json::from_str(&metadata_body).map_err(oauth_request_error)?;

        let jwks_response = self
            .http_client
            .get(&metadata.jwks_uri)
            .send()
            .await
            .map_err(oauth_request_error)?;
        if !jwks_response.status().is_success() {
            return Err(GatewayError::Network(format!(
                "OAuth JWKS request failed with {}",
                jwks_response.status()
            )));
        }
        let expires_at = now + jwks_cache_duration(jwks_response.headers());
        let jwks_body = jwks_response.text().await.map_err(oauth_request_error)?;
        let jwks = serde_json::from_str(&jwks_body).map_err(oauth_request_error)?;

        Ok(CachedJwks { jwks, expires_at })
    }
}

/// Single-flight + bounded stale-fallback coordination around a JWKS/discovery
/// `fetch`. Extracted from [`OAuthTokenService::jwks_for_profile`] so the
/// concurrency and fallback invariants are unit-testable with an injected
/// fetcher (M25 gate). D65 / audit N13; tenets VI, VII, XIX.
pub(crate) async fn jwks_single_flight<F, Fut>(
    entry: &JwksCacheEntry,
    now: OffsetDateTime,
    force_refresh: bool,
    fetch: F,
) -> Result<JwkSet, GatewayError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<CachedJwks, GatewayError>>,
{
    // Fast path: a fresh cache serves without contending the fetch lock. A forced
    // refresh (a rotated signing key) must skip this and actually re-fetch.
    if !force_refresh {
        if let Some(jwks) = entry.fresh_jwks(now) {
            return Ok(jwks);
        }
    }

    let prior_gen = entry.fetch_gen.load(Ordering::Acquire);
    // Single-flight: only the lock holder fetches; waiters re-observe the cache
    // the leader populated instead of stampeding the IdP.
    let _leader = entry.fetch_lock.lock().await;

    // Re-check after the wait — a concurrent leader may already have refreshed.
    if force_refresh {
        if entry.fetch_gen.load(Ordering::Acquire) != prior_gen {
            if let Some(jwks) = entry.any_jwks() {
                return Ok(jwks);
            }
        }
    } else if let Some(jwks) = entry.fresh_jwks(now) {
        return Ok(jwks);
    }

    match fetch().await {
        Ok(fetched) => {
            let jwks = fetched.jwks.clone();
            entry.store(fetched);
            Ok(jwks)
        }
        // Degrade under pressure (XIX): a fetch failure with a last-good cache
        // serves stale keys within the bound; a cold cache hard-fails the
        // exchange, propagating the original fetch error.
        Err(fetch_error) => entry.stale_within_bound(now).ok_or(fetch_error),
    }
}

#[cfg(test)]
impl JwksCacheEntry {
    pub(super) fn with_cached(cached: CachedJwks) -> Self {
        Self {
            cached: StdMutex::new(Some(cached)),
            ..Self::default()
        }
    }
}
