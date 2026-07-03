//! Level-1 hook execution: mint a per-invocation attenuated token, then deliver
//! the fact + message + token to a webhook (at-least-once, bounded retry,
//! dead-letter on exhaustion — ruling 5) or a local script (config-only; see the
//! module-level exec trust model).

use std::time::Duration;

use posthaste_domain_model::{DomainEvent, MessageSummary, Rule, RuleAction, RuleGrant, RuleOutcome};
use posthaste_observability::{events, ph_info};
use posthaste_provider_call::{CallClass, HttpRequestSpec};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::engine::{idempotency_key, EngineContext};
use super::RuleTokenGrant;

/// Wall-clock ceiling for a Level-1 `exec` script. A hung handler is killed and
/// dead-lettered rather than blocking the (single) evaluator forever.
const EXEC_TIMEOUT: Duration = Duration::from_secs(30);

impl EngineContext {
    /// Run a Level-1 hook (webhook or exec). Mints the scoped token, dispatches,
    /// and translates the outcome into `Delivered`/`Failed` (the caller emits the
    /// `rule.fired` fact; a failure also dead-letters here).
    pub(crate) async fn run_hook(
        &self,
        rule: &Rule,
        event: &DomainEvent,
        summary: &MessageSummary,
    ) -> RuleOutcome {
        let (grants, expiry_seconds) = match &rule.action {
            RuleAction::Webhook {
                grants,
                expiry_seconds,
                ..
            }
            | RuleAction::Exec {
                grants,
                expiry_seconds,
                ..
            } => (grants, *expiry_seconds),
            _ => return RuleOutcome::Failed,
        };

        let token = match self.mint_token(summary, grants, expiry_seconds) {
            Ok(token) => token,
            Err(reason) => {
                self.emit_delivery_failed(rule, event.seq, reason, 0, summary).await;
                return RuleOutcome::Failed;
            }
        };

        let key = idempotency_key(&rule.id, event.seq);
        let payload = json!({
            "ruleId": rule.id,
            "idempotencyKey": key,
            "event": {
                "seq": event.seq,
                "topic": event.topic,
                "accountId": event.account_id,
                "mailboxId": event.mailbox_id,
                "messageId": event.message_id,
            },
            "message": summary,
            "token": token,
        });

        match &rule.action {
            RuleAction::Webhook { url, .. } => {
                self.deliver_webhook(rule, event, summary, url, &payload).await
            }
            RuleAction::Exec { command, .. } => {
                self.deliver_exec(rule, event, summary, command, &token, &key, &payload)
                    .await
            }
            _ => RuleOutcome::Failed,
        }
    }

    /// Mint the per-invocation token: exactly the rule's grants, confined to the
    /// matched account + message, expiring after `expiry_seconds`. Least
    /// privilege — the hook cannot touch any other message, and cannot mint or
    /// manage (those verbs are never grantable to a hook).
    fn mint_token(
        &self,
        summary: &MessageSummary,
        grants: &[RuleGrant],
        expiry_seconds: u64,
    ) -> Result<String, String> {
        let minter = self
            .minter
            .as_ref()
            .ok_or_else(|| "no capability minter configured for hook actions".to_string())?;
        let expiry = OffsetDateTime::now_utc()
            .checked_add(time::Duration::seconds(expiry_seconds as i64))
            .and_then(|when| when.format(&Rfc3339).ok());
        let grant = RuleTokenGrant {
            actions: grants.iter().map(|g| g.verb().to_string()).collect(),
            account: Some(summary.source_id.to_string()),
            message: Some(summary.id.to_string()),
            expiry_rfc3339: expiry,
        };
        minter.mint(&grant)
    }

    async fn deliver_webhook(
        &self,
        rule: &Rule,
        event: &DomainEvent,
        summary: &MessageSummary,
        url: &str,
        payload: &serde_json::Value,
    ) -> RuleOutcome {
        let body = match serde_json::to_vec(payload) {
            Ok(body) => body,
            Err(error) => {
                self.emit_delivery_failed(
                    rule,
                    event.seq,
                    format!("serializing webhook payload: {error}"),
                    0,
                    summary,
                )
                .await;
                return RuleOutcome::Failed;
            }
        };
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let spec = HttpRequestSpec::post(url.to_string(), headers, body);

        // `execute` runs the whole bounded backoff schedule internally; an `Err`
        // means the retries were exhausted (ruling 5: dead-letter as a fact).
        match self
            .executor
            .execute(summary.source_id.as_str(), CallClass::Metadata, spec)
            .await
        {
            Ok(response) => {
                ph_info!(
                    events::RULE_WEBHOOK_DELIVERED,
                    rule_id = %rule.id,
                    status = response.status,
                    "rule webhook delivered"
                );
                RuleOutcome::Delivered
            }
            Err(error) => {
                self.emit_delivery_failed(
                    rule,
                    event.seq,
                    format!("webhook delivery failed: {error}"),
                    // The executor drove its full schedule before giving up.
                    posthaste_provider_call::BackoffSchedule::default().max_attempts,
                    summary,
                )
                .await;
                RuleOutcome::Failed
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn deliver_exec(
        &self,
        rule: &Rule,
        event: &DomainEvent,
        summary: &MessageSummary,
        command: &str,
        token: &str,
        key: &str,
        payload: &serde_json::Value,
    ) -> RuleOutcome {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        // PAYLOAD-IS-DATA (§7.20): `command` is a fixed host binary run with NO
        // arguments derived from event/message data — the payload reaches the
        // script only as the JSON stdin document, so a sender cannot inject a
        // command. The token lives in the environment (never on argv).
        let mut cmd = Command::new(command);
        cmd
            // On the evaluator timeout the `wait_with_output` future (which owns
            // the child) is dropped; `kill_on_drop` turns that into a real kill.
            .kill_on_drop(true)
            .env("POSTHASTE_TOKEN", token)
            .envs(exec_env_vars(event, summary, key))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.emit_delivery_failed(
                    rule,
                    event.seq,
                    format!("spawning `{command}`: {error}"),
                    0,
                    summary,
                )
                .await;
                return RuleOutcome::Failed;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            let bytes = serde_json::to_vec(payload).unwrap_or_default();
            let _ = stdin.write_all(&bytes).await;
            let _ = stdin.shutdown().await;
        }

        match tokio::time::timeout(EXEC_TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(output)) if output.status.success() => {
                ph_info!(
                    events::RULE_EXEC_COMPLETED,
                    rule_id = %rule.id,
                    command = %command,
                    "rule exec completed"
                );
                RuleOutcome::Delivered
            }
            Ok(Ok(output)) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                self.emit_delivery_failed(
                    rule,
                    event.seq,
                    format!("`{command}` exited {}: {}", output.status, stderr.trim()),
                    1,
                    summary,
                )
                .await;
                RuleOutcome::Failed
            }
            Ok(Err(error)) => {
                self.emit_delivery_failed(
                    rule,
                    event.seq,
                    format!("`{command}` failed: {error}"),
                    1,
                    summary,
                )
                .await;
                RuleOutcome::Failed
            }
            Err(_elapsed) => {
                // Timed out: the future holding the child is dropped here, and
                // `kill_on_drop` kills it. Dead-letter.
                self.emit_delivery_failed(
                    rule,
                    event.seq,
                    format!("`{command}` timed out after {}s", EXEC_TIMEOUT.as_secs()),
                    1,
                    summary,
                )
                .await;
                RuleOutcome::Failed
            }
        }
    }
}

/// The `PH_*` convenience env vars exported to a Level-1 `exec` script
/// (RFC-L2-scripting ruling 21: "posthastectl IS the SDK" — a handler is
/// type-free bash; these dissolve the "manual JSON parsing" problem for the
/// common fields, while the full event+message JSON stays on stdin for
/// anything not covered here). Pure and independently testable — kept out of
/// `deliver_exec` so a test doesn't need a live `EngineContext`/process spawn
/// to assert on the exported set.
///
/// `PH_FROM` prefers the sender's email; falls back to the display name when
/// the email is absent (rare — providers without a parsed address); empty
/// when neither is known. `PH_SUBJECT` and `PH_KEYWORDS` (comma-separated)
/// default to empty/none when absent.
fn exec_env_vars(
    event: &DomainEvent,
    summary: &MessageSummary,
    idempotency_key: &str,
) -> Vec<(&'static str, String)> {
    let from = summary
        .from_email
        .as_deref()
        .or(summary.from_name.as_deref())
        .unwrap_or("")
        .to_string();
    vec![
        ("PH_IDEMPOTENCY_KEY", idempotency_key.to_string()),
        ("PH_ACCOUNT", summary.source_id.to_string()),
        ("PH_MESSAGE_ID", summary.id.to_string()),
        ("PH_FROM", from),
        (
            "PH_SUBJECT",
            summary.subject.clone().unwrap_or_default(),
        ),
        ("PH_KEYWORDS", summary.keywords.join(",")),
        ("PH_EVENT_SEQ", event.seq.to_string()),
        ("PH_TOPIC", event.topic.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(source_id: &str, id: &str) -> MessageSummary {
        serde_json::from_value(serde_json::json!({
            "id": id, "sourceId": source_id, "sourceName": "Acct", "sourceThreadId": "t1",
            "conversationId": "c1", "subject": "Re: invoice", "fromName": "Ada Lovelace",
            "fromEmail": "ada@example.com", "to": [], "preview": null,
            "receivedAt": "2026-06-24T00:00:00Z", "hasAttachment": false, "isRead": false,
            "isFlagged": false, "mailboxIds": ["inbox"], "keywords": ["instruct", "urgent"]
        }))
        .expect("valid MessageSummary fixture")
    }

    fn event(seq: i64, topic: &str) -> DomainEvent {
        DomainEvent {
            seq,
            account_id: "acct-1".into(),
            topic: topic.to_string(),
            occurred_at: "2026-06-24T00:00:00Z".to_string(),
            mailbox_id: None,
            message_id: Some("msg-1".into()),
            payload: serde_json::Value::Null,
        }
    }

    /// The documented minimum set (RFC-L2-scripting ruling 21) is present with
    /// the expected values, keyed off the message summary + triggering event.
    #[test]
    fn exec_env_vars_covers_the_documented_minimum_set() {
        let vars = exec_env_vars(
            &event(91, "message.updated"),
            &summary("acct-1", "msg-1"),
            "rule:tagger:91",
        );
        let map: std::collections::HashMap<_, _> = vars.into_iter().collect();
        assert_eq!(map["PH_IDEMPOTENCY_KEY"], "rule:tagger:91");
        assert_eq!(map["PH_ACCOUNT"], "acct-1");
        assert_eq!(map["PH_MESSAGE_ID"], "msg-1");
        assert_eq!(map["PH_FROM"], "ada@example.com");
        assert_eq!(map["PH_SUBJECT"], "Re: invoice");
        assert_eq!(map["PH_KEYWORDS"], "instruct,urgent");
        assert_eq!(map["PH_EVENT_SEQ"], "91");
        assert_eq!(map["PH_TOPIC"], "message.updated");
    }

    /// `PH_FROM` falls back to the display name when no email is parsed, and is
    /// empty (never a literal "null") when neither is known.
    #[test]
    fn exec_env_vars_from_falls_back_then_empties() {
        let mut with_name_only = summary("acct-1", "msg-1");
        with_name_only.from_email = None;
        with_name_only.from_name = Some("Ada".to_string());
        let vars = exec_env_vars(&event(1, "message.updated"), &with_name_only, "k");
        let map: std::collections::HashMap<_, _> = vars.into_iter().collect();
        assert_eq!(map["PH_FROM"], "Ada");

        let mut with_neither = summary("acct-1", "msg-1");
        with_neither.from_email = None;
        with_neither.from_name = None;
        with_neither.subject = None;
        with_neither.keywords = Vec::new();
        let vars = exec_env_vars(&event(1, "message.updated"), &with_neither, "k");
        let map: std::collections::HashMap<_, _> = vars.into_iter().collect();
        assert_eq!(map["PH_FROM"], "");
        assert_eq!(map["PH_SUBJECT"], "");
        assert_eq!(map["PH_KEYWORDS"], "");
    }

    /// Two different triggering events for the same message produce two
    /// different `PH_EVENT_SEQ` values — the deterministic-idempotency-key
    /// building block a handler composes with (redelivery of the *same* event
    /// reproduces the same seq; a distinct event does not).
    #[test]
    fn exec_env_vars_event_seq_tracks_the_triggering_event() {
        let s = summary("acct-1", "msg-1");
        let a = exec_env_vars(&event(5, "message.updated"), &s, "k")
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        let b = exec_env_vars(&event(6, "message.updated"), &s, "k")
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(a["PH_EVENT_SEQ"], "5");
        assert_eq!(b["PH_EVENT_SEQ"], "6");
        assert_ne!(a["PH_EVENT_SEQ"], b["PH_EVENT_SEQ"]);
    }
}
