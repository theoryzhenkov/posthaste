use super::*;

use tokio_util::sync::CancellationToken;

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
    pub account_id: Option<posthaste_domain_model::AccountId>,
    pub profile: OAuthProviderProfile,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub pkce_verifier: String,
    pub nonce: String,
}

pub(crate) const OAUTH_COMPLETION_STATE_TTL_SECONDS: i64 = 10 * 60;

/// Hard cap on the number of tracked OAuth flow states (pending + completing +
/// completed). `insert` is reachable from the unauthenticated `/oauth/start`
/// endpoint (`handlers.rs`), so without a bound a flood of callback-less
/// `/oauth/start` requests grows a map of secrets-bearing `PendingOAuthFlow`
/// values (client secret, PKCE verifier, nonce) without limit — the unauth
/// memory DoS closed here (RFC-L2-lifecycle N12 / D67(a) / M27 sub-unit (a)).
/// Enforced on every [`OAuthFlowStore::insert`] by evicting the single oldest
/// entry once the TTL prune (below) leaves the map still at capacity.
/// **Review** (picked sane, not measured — flag for owner review, matching
/// the D66/D61 constants' posture).
pub(crate) const OAUTH_PENDING_FLOW_CAP: usize = 512;

/// Cadence for [`OAuthFlowStore::spawn_sweep_task`]'s background TTL sweep.
/// Defense-in-depth alongside the per-insert prune in `insert`/`begin_completion`:
/// a store that stops receiving requests entirely (an idle deployment) still
/// ages out its expired entries on this timer instead of holding them until
/// the next request arrives. **Review**.
pub(crate) const OAUTH_FLOW_SWEEP_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(5 * 60);

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
    /// Insert a fresh pending flow (called from unauthenticated `/oauth/start`
    /// — `handlers.rs`). Sweeps expired entries and enforces the hard cap on
    /// every call: previously only [`Self::begin_completion`] pruned, so an
    /// attacker who never calls back (only ever hits `/oauth/start`) grew this
    /// map unbounded. Both the sweep and the cap now run here too (N12).
    pub async fn insert(&self, state: String, flow: PendingOAuthFlow) {
        let now = OffsetDateTime::now_utc();
        let mut flows = self.flows.lock().await;
        prune_oauth_states(&mut flows, now);
        evict_oldest_if_at_capacity(&mut flows, OAUTH_PENDING_FLOW_CAP);
        flows.insert(state, StoredOAuthFlow::Pending(Box::new(flow), now));
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

    /// Test-only seam for asserting the map stays bounded under a flood (N12).
    #[cfg(test)]
    pub(crate) async fn len(&self) -> usize {
        self.flows.lock().await.len()
    }

    /// Background TTL sweep (N12 / D67(a) defense-in-depth): periodically
    /// prunes expired entries even when nothing is calling `insert` or
    /// `begin_completion` to trigger an inline sweep. Runs until `cancel` is
    /// cancelled, so the caller can tie its lifetime to the server's shutdown
    /// sequence like every other supervisor-owned periodic task.
    pub fn spawn_sweep_task(self: Arc<Self>, cancel: CancellationToken) -> tokio::task::JoinHandle<()> {
        let store = self;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(OAUTH_FLOW_SWEEP_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    _ = interval.tick() => {
                        let now = OffsetDateTime::now_utc();
                        let mut flows = store.flows.lock().await;
                        prune_oauth_states(&mut flows, now);
                    }
                }
            }
        })
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

/// Evicts the single oldest entry (by its stored timestamp) once `flows` is
/// at `cap`, making room for the insert that follows. Runs after
/// [`prune_oauth_states`], so this only fires when the map is *still* at
/// capacity after expired entries are gone — i.e. under sustained flood
/// pressure, not steady-state churn.
fn evict_oldest_if_at_capacity(flows: &mut HashMap<String, StoredOAuthFlow>, cap: usize) {
    if flows.len() < cap {
        return;
    }
    let oldest_key = flows
        .iter()
        .min_by_key(|(_, flow)| match flow {
            StoredOAuthFlow::Pending(_, started_at) => *started_at,
            StoredOAuthFlow::Completing(started_at) | StoredOAuthFlow::Completed(started_at) => {
                *started_at
            }
        })
        .map(|(key, _)| key.clone());
    if let Some(key) = oldest_key {
        flows.remove(&key);
    }
}
