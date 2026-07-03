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
                self.emit_delivery_failed(rule, event.seq, reason, 0, summary);
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
                );
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
                );
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
            .env("PH_IDEMPOTENCY_KEY", key)
            .env("PH_ACCOUNT_ID", summary.source_id.as_str())
            .env("PH_MESSAGE_ID", summary.id.as_str())
            .env("PH_TOPIC", &event.topic)
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
                );
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
                );
                RuleOutcome::Failed
            }
            Ok(Err(error)) => {
                self.emit_delivery_failed(
                    rule,
                    event.seq,
                    format!("`{command}` failed: {error}"),
                    1,
                    summary,
                );
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
                );
                RuleOutcome::Failed
            }
        }
    }
}
