use super::*;

impl OAuthTokenService {
    pub(super) async fn jwks_for_profile(
        &self,
        profile: &OAuthProviderProfile,
        now: OffsetDateTime,
        force_refresh: bool,
    ) -> Result<JwkSet, GatewayError> {
        let cache = OAUTH_JWKS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if !force_refresh {
            let cached = cache.lock().await.get(profile.metadata_url).cloned();
            if let Some(cached) = cached {
                if cached.expires_at > now {
                    return Ok(cached.jwks);
                }
            }
        }

        let fetched = self.fetch_jwks(profile, now).await?;
        cache
            .lock()
            .await
            .insert(profile.metadata_url, fetched.clone());
        Ok(fetched.jwks)
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
