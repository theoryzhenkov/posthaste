use super::*;

#[test]
fn openid_claim_decoding_verifies_signature_with_matching_jwk() {
    let (id_token, jwks) = signed_id_token("test-key", "expected-nonce");

    let claims = decode_verified_openid_claims(
        &OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile"),
        "client-id",
        &id_token,
        "test-key",
        &jwks,
        "expected-nonce",
        OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now"),
    )
    .expect("signed token should verify");

    assert_eq!(claims.email.as_deref(), Some("user@example.test"));
}

#[test]
fn openid_claim_decoding_rejects_tampered_signature() {
    let (mut id_token, jwks) = signed_id_token("test-key", "expected-nonce");
    id_token.push('a');

    let error = decode_verified_openid_claims(
        &OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile"),
        "client-id",
        &id_token,
        "test-key",
        &jwks,
        "expected-nonce",
        OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now"),
    )
    .expect_err("tampered signature should be rejected");

    assert!(matches!(error, GatewayError::Rejected(message) if message.contains("invalid")));
}

#[test]
fn jwks_cache_duration_uses_cache_control_max_age() {
    let mut headers = oauth2::http::HeaderMap::new();
    headers.insert(
        oauth2::http::header::CACHE_CONTROL,
        oauth2::http::HeaderValue::from_static("public, max-age=120"),
    );

    assert_eq!(jwks_cache_duration(&headers), Duration::seconds(120));
}

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

fn sample_jwks() -> JwkSet {
    signed_id_token("stale-key", "nonce").1
}

fn fixed_now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now")
}

/// M25 gate: a burst of concurrent cold-cache validations issues exactly one
/// discovery + JWKS fetch (single-flight), the rest reuse the leader's result.
#[tokio::test(start_paused = true)]
async fn concurrent_cold_cache_validations_issue_one_jwks_fetch() {
    let entry = Arc::new(JwksCacheEntry::default());
    let fetches = Arc::new(AtomicUsize::new(0));
    let now = fixed_now();

    let mut handles = Vec::new();
    for _ in 0..8 {
        let entry = Arc::clone(&entry);
        let fetches = Arc::clone(&fetches);
        handles.push(tokio::spawn(async move {
            jwks_single_flight(&entry, now, false, || async {
                fetches.fetch_add(1, AtomicOrdering::SeqCst);
                // Hold the single-flight lock long enough for the burst to overlap.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok(CachedJwks {
                    jwks: sample_jwks(),
                    expires_at: now + Duration::hours(1),
                })
            })
            .await
            .expect("jwks");
        }));
    }
    for handle in handles {
        handle.await.expect("join");
    }

    assert_eq!(fetches.load(AtomicOrdering::SeqCst), 1);
}

/// M25 gate: a fetch failure with a previously-fetched (now expired) key set
/// serves the stale keys instead of failing the code exchange.
#[tokio::test]
async fn stale_jwks_served_when_refresh_fetch_fails() {
    let now = fixed_now();
    let entry = JwksCacheEntry::with_cached(CachedJwks {
        jwks: sample_jwks(),
        expires_at: now - Duration::hours(1), // expired, but within the stale bound
    });

    let jwks = jwks_single_flight(&entry, now, false, || async {
        Err(GatewayError::Network("idp unreachable".to_string()))
    })
    .await
    .expect("stale keys should be served on fetch failure");

    assert_eq!(jwks.keys.len(), 1);
}

/// M25 gate: a cold cache plus a failed fetch has no fallback and must hard-fail.
#[tokio::test]
async fn cold_cache_and_failed_fetch_hard_fails() {
    let now = fixed_now();
    let entry = JwksCacheEntry::default();

    let error = jwks_single_flight(&entry, now, false, || async {
        Err(GatewayError::Network("idp unreachable".to_string()))
    })
    .await
    .expect_err("a cold cache with a failed fetch must hard-fail");

    assert!(matches!(error, GatewayError::Network(_)));
}

/// The stale fallback is bounded: keys stale beyond the max-stale window are not
/// served, even though a cached copy exists.
#[tokio::test]
async fn stale_jwks_beyond_the_bound_hard_fails() {
    let now = fixed_now();
    let entry = JwksCacheEntry::with_cached(CachedJwks {
        jwks: sample_jwks(),
        expires_at: now - Duration::hours(7), // 7h past expiry, beyond the 6h bound
    });

    let error = jwks_single_flight(&entry, now, false, || async {
        Err(GatewayError::Network("idp unreachable".to_string()))
    })
    .await
    .expect_err("keys stale beyond the bound must not be served");

    assert!(matches!(error, GatewayError::Network(_)));
}

/// M25 gate: a hung IdP on the JWKS path is bounded — under virtual time the
/// coordination never hangs past the total-timeout budget the shared client
/// enforces in production.
#[tokio::test(start_paused = true)]
async fn hung_idp_jwks_fetch_is_bounded_by_the_total_timeout_budget() {
    let now = fixed_now();
    let entry = JwksCacheEntry::default();

    let outcome = tokio::time::timeout(OAUTH_HTTP_TOTAL_TIMEOUT, async {
        jwks_single_flight(&entry, now, false, || async {
            std::future::pending::<Result<CachedJwks, GatewayError>>().await
        })
        .await
    })
    .await;

    assert!(outcome.is_err(), "a hung IdP must not hang the fetch");
}

/// The shared client's `.timeout()` bounds a hung IdP on the request path
/// (token exchange / refresh / discovery all use the same mechanism).
#[tokio::test]
async fn reqwest_client_timeout_bounds_a_hung_idp_socket() {
    // A listener that accepts the TCP connection but never responds — the classic
    // hung IdP. The client's total timeout must surface an error, not block.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                held.push(stream); // keep the socket open, never respond
            }
        }
    });

    let client = oauth2::reqwest::ClientBuilder::new()
        .timeout(std::time::Duration::from_millis(200))
        .build()
        .expect("client");

    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.get(format!("http://{addr}/")).send(),
    )
    .await
    .expect("request must resolve well within the outer guard");

    assert!(result.is_err(), "a hung IdP must surface a timeout error");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
