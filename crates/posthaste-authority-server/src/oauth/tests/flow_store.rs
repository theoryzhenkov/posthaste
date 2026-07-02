use super::*;
use crate::oauth::flow_store::OAUTH_COMPLETION_STATE_TTL_SECONDS;
use time::{Duration, OffsetDateTime};

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
