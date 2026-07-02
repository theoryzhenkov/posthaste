use super::*;

impl MailService {
    /// Query the event log with optional filters.
    ///
    /// @spec docs/L1-api#sse-event-stream
    pub fn list_events(
        &self,
        filter: &posthaste_domain_model::EventFilter,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        self.events.list_events(filter).map_err(Into::into)
    }

    /// Fetch the primary sender identity, falling back to the account's
    /// configured sender (address + display name) when the provider exposes no
    /// identity of its own (e.g. a Stalwart account whose `Identity/get` is
    /// empty). This keeps compose's default `from` working for any provider;
    /// provider-backed sending may still need a real provider identity id.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-compose#composesession-interface
    pub async fn fetch_identity(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Identity, ServiceError> {
        match gateway.fetch_identity(account_id).await {
            Ok(mut identity) => {
                // The locally configured display name (`full_name`) overrides the
                // provider's, which is often the bare account username (e.g.
                // "theor"). Only the name is overridden: the server `id` and
                // `email` are kept so `EmailSubmission/set`'s `identityId`
                // stays valid and delivery uses the real address.
                //
                // @spec docs/L1-compose#sender-selection
                if let Some(full_name) = self.configured_display_name(account_id)? {
                    identity.name = full_name;
                }
                Ok(identity)
            }
            Err(error) => match self.configured_sender_identity(account_id)? {
                Some(identity) => Ok(identity),
                None => Err(error.into()),
            },
        }
    }

    /// Build a sender identity from account config for providers that expose no
    /// identity. Returns `None` when no usable address is configured.
    fn configured_sender_identity(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<Identity>, ServiceError> {
        let Some(account) = self.config.get_source(account_id)? else {
            return Ok(None);
        };
        let Some(email) = account
            .email_patterns
            .iter()
            .find(|address| address.contains('@'))
            .cloned()
            .or_else(|| {
                account
                    .transport
                    .username
                    .clone()
                    .filter(|username| username.contains('@'))
            })
        else {
            return Ok(None);
        };
        let name = account
            .full_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| {
                email
                    .split('@')
                    .next()
                    .unwrap_or(email.as_str())
                    .to_string()
            });
        Ok(Some(Identity {
            id: format!("config:{}", account_id.as_str()),
            name,
            email,
        }))
    }

    /// The account's configured display name (`full_name`), when set and
    /// non-empty. Used to override the provider identity's name.
    fn configured_display_name(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<String>, ServiceError> {
        let Some(account) = self.config.get_source(account_id)? else {
            return Ok(None);
        };
        Ok(account
            .full_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string))
    }

    /// Fetch reply/forward metadata for composing a response.
    pub async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: &dyn MailGateway,
    ) -> Result<posthaste_domain_model::ReplyContext, ServiceError> {
        gateway
            .fetch_reply_context(account_id, message_id)
            .await
            .map_err(Into::into)
    }
}
