//! The unsubscribe family: RFC 8058 one-click execution. The POST runs
//! backend-side on a locked-down client — https-only, credential-free, no
//! redirect downgrade, bounded timeout — and only for a message whose stored
//! target re-validates (stored data is untrusted input). Acceptance means
//! the POST is underway; its outcome is published on the event stream, never
//! as an HTTP reply.

use std::sync::OnceLock;
use std::time::Duration;

use axum::http::StatusCode;
use posthaste_client_models::{ApiErrorKind, UnsubscribeIntent};
use posthaste_domain_model::{DomainEvent, EVENT_TOPIC_MESSAGE_UPDATED};

use super::{now_rfc3339, ApiFailure};
use crate::AppState;

/// RFC 8058 §3.2: the POST body is exactly this form-encoded pair.
const ONE_CLICK_BODY: &str = "List-Unsubscribe=One-Click";

/// Total deadline for the outbound POST. The remote is an arbitrary
/// third-party list server; fail fast and report through the event.
const ONE_CLICK_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn unsubscribe(app: &AppState, intent: UnsubscribeIntent) -> Result<u64, ApiFailure> {
    // Body-free read: the parsed targets live on the header projection.
    let detail = app
        .service
        .get_message_header(&intent.account_id, &intent.message_id)?
        .ok_or_else(|| ApiFailure::unknown_id(format!("message {}", intent.message_id.as_str())))?;
    let Some(targets) = detail.list_unsubscribe else {
        return Err(no_target("message has no unsubscribe target"));
    };
    if !targets.one_click {
        return Err(no_target(
            "message has no one-click (RFC 8058) unsubscribe target",
        ));
    }
    let Some(https) = targets.https else {
        return Err(no_target("message has no https unsubscribe target"));
    };
    let url = validated_one_click_url(&https)?;

    // Accepted: the POST proceeds in the background and its outcome — either
    // way — is published as a message-scoped event.
    let events = app.events.clone();
    let account_id = intent.account_id;
    let message_id = intent.message_id;
    tokio::spawn(async move {
        let outcome = post_one_click(url).await;
        let payload = match outcome {
            Ok(http_status) => serde_json::json!({
                "changes": { "unsubscribed": true },
                "unsubscribed": { "ok": true, "httpStatus": http_status },
            }),
            Err(reason) => serde_json::json!({
                "changes": { "unsubscribed": true },
                "unsubscribed": { "ok": false, "error": reason },
            }),
        };
        events.publish(&[DomainEvent {
            seq: 0,
            account_id,
            topic: EVENT_TOPIC_MESSAGE_UPDATED.to_string(),
            occurred_at: now_rfc3339(),
            mailbox_id: None,
            message_id: Some(message_id),
            payload,
        }]);
    });
    Ok(app.events.generation())
}

fn no_target(message: &str) -> ApiFailure {
    ApiFailure::new(StatusCode::CONFLICT, ApiErrorKind::Conflict, message, false)
}

/// Re-validate the stored https target before any request is built: the
/// shared conservative validator (the one ingest used) and a full URL parse
/// must both agree — https scheme, no userinfo, a DNS-name host (never an IP
/// literal).
fn validated_one_click_url(raw: &str) -> Result<reqwest::Url, ApiFailure> {
    let reject = || no_target("stored unsubscribe target is not a valid https URL");
    posthaste_domain_model::validate_one_click_https(raw).map_err(|_| reject())?;
    let url = reqwest::Url::parse(raw).map_err(|_| reject())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.domain().is_none()
    {
        return Err(reject());
    }
    Ok(url)
}

/// The locked-down outbound client, built once: no cookie store, no auth, no
/// default headers; https-only; redirects followed only https→https and at
/// most 3 hops.
fn one_click_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 3 {
                    attempt.error("too many redirects")
                } else if attempt.url().scheme() != "https" {
                    attempt.error("redirect to a non-https target")
                } else {
                    attempt.follow()
                }
            }))
            .https_only(true)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("static one-click client config should build")
    })
}

/// Perform the RFC 8058 POST. Returns the 2xx status; anything else —
/// non-2xx answer, blocked redirect, timeout, connect failure — is an error
/// string safe to surface (the response body is never read).
async fn post_one_click(url: reqwest::Url) -> Result<u16, String> {
    let response = one_click_client()
        .post(url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(ONE_CLICK_BODY)
        .timeout(ONE_CLICK_TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                "the unsubscribe request timed out".to_string()
            } else if error.is_redirect() {
                "the list server redirected to a non-https target".to_string()
            } else {
                "could not reach the list server".to_string()
            }
        })?;
    let status = response.status();
    if status.is_success() {
        Ok(status.as_u16())
    } else {
        Err(format!("the list server answered HTTP {}", status.as_u16()))
    }
}
