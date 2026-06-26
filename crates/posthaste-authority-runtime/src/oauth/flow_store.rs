use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAuthorizationSession {
    pub authorization_url: String,
    pub state: String,
    #[serde(skip_serializing)]
    pub pkce_verifier: String,
    #[serde(skip_serializing)]
    pub nonce: String,
    pub redirect_uri: String,
}

#[derive(Clone, Debug)]
pub struct PendingOAuthFlow {
    pub account_id: Option<posthaste_domain::AccountId>,
    pub profile: OAuthProviderProfile,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub pkce_verifier: String,
    pub nonce: String,
}

pub(crate) const OAUTH_COMPLETION_STATE_TTL_SECONDS: i64 = 10 * 60;

#[derive(Debug)]
enum StoredOAuthFlow {
    Pending(Box<PendingOAuthFlow>, OffsetDateTime),
    Completing(OffsetDateTime),
    Completed(OffsetDateTime),
}

pub enum OAuthFlowCompletion {
    Pending(Box<PendingOAuthFlow>),
    Completing,
    Completed,
    Unknown,
}

#[derive(Default)]
pub struct OAuthFlowStore {
    flows: Mutex<HashMap<String, StoredOAuthFlow>>,
}

impl OAuthFlowStore {
    pub async fn insert(&self, state: String, flow: PendingOAuthFlow) {
        self.flows.lock().await.insert(
            state,
            StoredOAuthFlow::Pending(Box::new(flow), OffsetDateTime::now_utc()),
        );
    }

    #[cfg(test)]
    pub(crate) async fn insert_at(
        &self,
        state: String,
        flow: PendingOAuthFlow,
        started_at: OffsetDateTime,
    ) {
        self.flows
            .lock()
            .await
            .insert(state, StoredOAuthFlow::Pending(Box::new(flow), started_at));
    }

    pub async fn begin_completion(&self, state: &str) -> OAuthFlowCompletion {
        let now = OffsetDateTime::now_utc();
        let mut flows = self.flows.lock().await;
        prune_oauth_states(&mut flows, now);
        match flows.remove(state) {
            Some(StoredOAuthFlow::Pending(flow, _started_at)) => {
                flows.insert(state.to_string(), StoredOAuthFlow::Completing(now));
                OAuthFlowCompletion::Pending(flow)
            }
            Some(StoredOAuthFlow::Completing(started_at)) => {
                flows.insert(state.to_string(), StoredOAuthFlow::Completing(started_at));
                OAuthFlowCompletion::Completing
            }
            Some(StoredOAuthFlow::Completed(completed_at)) => {
                flows.insert(state.to_string(), StoredOAuthFlow::Completed(completed_at));
                OAuthFlowCompletion::Completed
            }
            None => OAuthFlowCompletion::Unknown,
        }
    }

    pub async fn mark_completed(&self, state: String) {
        self.flows
            .lock()
            .await
            .insert(state, StoredOAuthFlow::Completed(OffsetDateTime::now_utc()));
    }
}

fn prune_oauth_states(flows: &mut HashMap<String, StoredOAuthFlow>, now: OffsetDateTime) {
    flows.retain(|_, flow| match flow {
        StoredOAuthFlow::Pending(_, started_at) => {
            now - *started_at < Duration::seconds(OAUTH_COMPLETION_STATE_TTL_SECONDS)
        }
        StoredOAuthFlow::Completing(started_at) | StoredOAuthFlow::Completed(started_at) => {
            now - *started_at < Duration::seconds(OAUTH_COMPLETION_STATE_TTL_SECONDS)
        }
    });
}
