use std::sync::Arc;

use async_trait::async_trait;
use jmap_client::client::Client;
use mail_domain::{
    now_iso8601 as domain_now_iso8601, AccountId, GatewayError, PushNotification, PushStream,
    PushTransport,
};

use crate::live::map_gateway_error;

pub struct WsPushTransport {
    client: Arc<Client>,
}

impl WsPushTransport {
    pub fn new(client: Arc<Client>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl PushTransport for WsPushTransport {
    fn name(&self) -> &'static str {
        "ws"
    }

    async fn open(
        &self,
        account_id: &AccountId,
        checkpoint: Option<&str>,
    ) -> Result<Option<PushStream>, GatewayError> {
        if self.client.session().websocket_capabilities().is_none() {
            return Ok(None);
        }

        let ws = self
            .client
            .connect_ws_correlated()
            .await
            .map_err(map_gateway_error)?;

        ws.enable_push_ws(
            Some([
                jmap_client::DataType::Email,
                jmap_client::DataType::Mailbox,
                jmap_client::DataType::EmailDelivery,
                jmap_client::DataType::EmailSubmission,
            ]),
            checkpoint.map(String::from),
        )
        .await
        .map_err(map_gateway_error)?;

        let account_id = account_id.clone();

        Ok(Some(Box::pin(async_stream::stream! {
            loop {
                match ws.next_push().await {
                    Some(Ok(push)) => {
                        match convert_push_object(&account_id, push) {
                            Some(notification) => yield Ok(notification),
                            None => continue,
                        }
                    }
                    Some(Err(error)) => {
                        yield Err(map_gateway_error(error));
                        return;
                    }
                    None => return,
                }
            }
        })))
    }
}

fn convert_push_object(
    account_id: &AccountId,
    push: jmap_client::PushObject,
) -> Option<PushNotification> {
    match push {
        jmap_client::PushObject::StateChange { mut changed } => {
            let changed_types = changed
                .remove(account_id.as_str())
                .map(|entries| entries.into_keys().map(|dt| dt.to_string()).collect())
                .unwrap_or_default();
            let received_at = domain_now_iso8601().ok()?;
            Some(PushNotification {
                account_id: account_id.clone(),
                changed: changed_types,
                received_at,
                checkpoint: None,
            })
        }
        _ => None,
    }
}
