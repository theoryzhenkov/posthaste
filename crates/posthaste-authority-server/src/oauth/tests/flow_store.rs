use super::*;
use crate::oauth::flow_store::{OAUTH_COMPLETION_STATE_TTL_SECONDS, OAUTH_PENDING_FLOW_CAP};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

fn test_flow(client_id: &str) -> PendingOAuthFlow {
    let profile = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");
    PendingOAuthFlow {
        account_id: Some(posthaste_domain_model::AccountId::from("gmail")),
        profile,
        client_id: client_id.to_string(),
        client_secret: Some("client-secret".to_string()),
        redirect_uri: "http://127.0.0.1:12345/v1/oauth/callback".to_string(),
        pkce_verifier: "verifier".to_string(),
        nonce: "nonce".to_string(),
    }
}

#[tokio::test]
async fn flow_store_transitions_pending_state_once() {
    let store = OAuthFlowStore::default();
    let profile = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");
    let flow = PendingOAuthFlow {
        account_id: Some(posthaste_domain_model::AccountId::from("gmail")),
        profile,
        client_id: "client-id".to_string(),
        client_secret: Some("client-secret".to_string()),
        redirect_uri: "http://127.0.0.1:12345/v1/oauth/callback".to_string(),
        pkce_verifier: "verifier".to_string(),
        nonce: "nonce".to_string(),
    };

    store.insert("state".to_string(), flow).await;

    assert!(matches!(
        store.begin_completion("state").await,
        OAuthFlowCompletion::Pending(_)
    ));
    assert!(matches!(
        store.begin_completion("state").await,
        OAuthFlowCompletion::Completing
    ));
}

#[tokio::test]
async fn flow_store_remembers_completed_states_for_duplicate_callbacks() {
    let store = OAuthFlowStore::default();

    assert!(matches!(
        store.begin_completion("state").await,
        OAuthFlowCompletion::Unknown
    ));
    store.mark_completed("state".to_string()).await;
    assert!(matches!(
        store.begin_completion("state").await,
        OAuthFlowCompletion::Completed
    ));
}

#[tokio::test]
async fn flow_store_expires_pending_states_after_ttl() {
    let store = OAuthFlowStore::default();
    let profile = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");
    let flow = PendingOAuthFlow {
        account_id: Some(posthaste_domain_model::AccountId::from("gmail")),
        profile,
        client_id: "client-id".to_string(),
        client_secret: Some("client-secret".to_string()),
        redirect_uri: "http://127.0.0.1:12345/v1/oauth/callback".to_string(),
        pkce_verifier: "verifier".to_string(),
        nonce: "nonce".to_string(),
    };

    let expired =
        OffsetDateTime::now_utc() - Duration::seconds(OAUTH_COMPLETION_STATE_TTL_SECONDS + 1);
    store
        .insert_at("state".to_string(), flow.clone(), expired)
        .await;
    assert!(
        matches!(
            store.begin_completion("state").await,
            OAuthFlowCompletion::Unknown
        ),
        "expired pending flow should be pruned"
    );

    // A fresh flow should still work normally.
    store.insert("state".to_string(), flow).await;
    assert!(
        matches!(
            store.begin_completion("state").await,
            OAuthFlowCompletion::Pending(_)
        ),
        "fresh pending flow should be retrievable"
    );
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d67 (N12 / M27 sub-unit (a))
#[tokio::test]
async fn flow_store_insert_prunes_expired_entries_without_a_callback() {
    // Regression for N12: previously only `begin_completion` pruned, so an
    // unauthenticated flood of `/oauth/start` (which only ever calls `insert`,
    // never `begin_completion`) never shrank the map. Now every `insert`
    // sweeps expired entries too.
    let store = OAuthFlowStore::default();
    let expired =
        OffsetDateTime::now_utc() - Duration::seconds(OAUTH_COMPLETION_STATE_TTL_SECONDS + 1);
    for index in 0..50 {
        store
            .insert_at(format!("expired-{index}"), test_flow("client"), expired)
            .await;
    }
    assert_eq!(store.len().await, 50);

    // A single fresh insert should sweep every expired entry away, leaving
    // only itself.
    store.insert("fresh".to_string(), test_flow("client")).await;
    assert_eq!(
        store.len().await,
        1,
        "insert should prune expired entries, not just begin_completion"
    );
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d67 (N12 / M27 sub-unit (a))
#[tokio::test]
async fn flow_store_insert_flood_stays_bounded_at_the_cap() {
    // Stand-in for an unauthenticated `/oauth/start` flood that never
    // completes (so nothing ever prunes via TTL either, within this test's
    // bound): every inserted state is distinct and none expires, so without a
    // hard cap the map would grow without limit.
    let store = OAuthFlowStore::default();
    for index in 0..(OAUTH_PENDING_FLOW_CAP * 4) {
        store
            .insert(format!("flood-{index}"), test_flow("client"))
            .await;
    }
    assert_eq!(
        store.len().await,
        OAUTH_PENDING_FLOW_CAP,
        "insert must evict to stay at the cap under sustained flood pressure"
    );
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d67 (N12 / M27 sub-unit (a))
#[tokio::test(start_paused = true)]
async fn flow_store_sweep_task_prunes_on_its_own_timer() {
    use crate::oauth::flow_store::OAUTH_FLOW_SWEEP_INTERVAL;
    use tokio_util::sync::CancellationToken;

    let store = Arc::new(OAuthFlowStore::default());
    let expired =
        OffsetDateTime::now_utc() - Duration::seconds(OAUTH_COMPLETION_STATE_TTL_SECONDS + 1);
    store
        .insert_at("expired".to_string(), test_flow("client"), expired)
        .await;
    assert_eq!(store.len().await, 1);

    let cancel = CancellationToken::new();
    let handle = store.clone().spawn_sweep_task(cancel.clone());

    // No request ever arrives to trigger an inline prune; only the
    // background sweep's own timer should clear the expired entry.
    tokio::time::sleep(OAUTH_FLOW_SWEEP_INTERVAL + std::time::Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(
        store.len().await,
        0,
        "the background sweep should prune expired entries on its own cadence"
    );

    cancel.cancel();
    let _ = handle.await;
}
