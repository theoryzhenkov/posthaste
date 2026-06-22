use super::*;

impl MailService {
    /// Query the event log with optional filters.
    ///
    /// @spec docs/L1-api#sse-event-stream
    pub fn list_events(
        &self,
        filter: &crate::EventFilter,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        self.events.list_events(filter).map_err(Into::into)
    }

    /// Fetch the primary sender identity from the gateway.
    ///
    /// @spec docs/L1-jmap#methods-used
    pub async fn fetch_identity(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Identity, ServiceError> {
        gateway.fetch_identity(account_id).await.map_err(Into::into)
    }

    /// Fetch reply/forward metadata for composing a response.
    pub async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: &dyn MailGateway,
    ) -> Result<crate::ReplyContext, ServiceError> {
        gateway
            .fetch_reply_context(account_id, message_id)
            .await
            .map_err(Into::into)
    }
}
