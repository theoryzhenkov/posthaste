use std::sync::Arc;

use async_trait::async_trait;
use mail_domain::{
    now_iso8601 as domain_now_iso8601, AccountId, GatewayError, PushNotification, PushStream,
    PushTransport,
};

use crate::live::map_gateway_error;
use crate::ws_connection::SharedWsConnection;

pub struct WsPushTransport {
    ws: Arc<SharedWsConnection>,
}

impl WsPushTransport {
    pub fn new(ws: Arc<SharedWsConnection>) -> Self {
        Self { ws }
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
        self.ws.ensure_connected().await?;
        self.ws.enable_push(checkpoint).await?;

        let ws = self.ws.clone();
        let account_id = account_id.clone();

        Ok(Some(Box::pin(async_stream::stream! {
            loop {
                match ws.next_push().await {
                    Some(Ok(push)) => {
                        match convert_push_object(&account_id, push) {
                            Ok(Some(notification)) => yield Ok(notification),
                            Ok(None) => continue,
                            Err(error) => yield Err(error),
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
) -> Result<Option<PushNotification>, GatewayError> {
    match push {
        jmap_client::PushObject::StateChange { mut changed } => {
            let changed_types = changed
                .remove(account_id.as_str())
                .map(|entries| entries.into_keys().map(|dt| dt.to_string()).collect())
                .unwrap_or_default();
            let received_at = domain_now_iso8601().map_err(GatewayError::Rejected)?;
            Ok(Some(PushNotification {
                account_id: account_id.clone(),
                changed: changed_types,
                received_at,
                // WS push notifications don't carry SSE-style event IDs.
                // The ResilientPushStream will pass the last SSE checkpoint
                // (if any) on reconnect, but WS connections cannot resume
                // from a checkpoint. A full delta sync after WS reconnect
                // handles this correctly.
                checkpoint: None,
            }))
        }
        _ => Ok(None),
    }
}
