use std::fmt;

use async_trait::async_trait;

use crate::GatewayError;

/// Resolve the current account secret immediately before a provider operation.
///
/// Password-based accounts typically return the configured password unchanged.
/// OAuth accounts may refresh the short-lived access token through the provider
/// token endpoint. Callers receive a fresh secret for every connection or
/// request, so the gateway never relies on a token that may have expired.
#[async_trait]
pub trait SecretResolver: Send + Sync + fmt::Debug {
    /// Return the current secret, refreshing it first if necessary.
    async fn resolve_secret(&self) -> Result<String, GatewayError>;
}

/// Resolver that always returns a fixed secret.
///
/// Used for password-based accounts, app-password accounts, and tests where the
/// secret does not change during the gateway lifetime.
#[derive(Debug, Clone)]
pub struct StaticSecretResolver {
    secret: String,
}

impl StaticSecretResolver {
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
        }
    }
}

#[async_trait]
impl SecretResolver for StaticSecretResolver {
    async fn resolve_secret(&self) -> Result<String, GatewayError> {
        Ok(self.secret.clone())
    }
}
