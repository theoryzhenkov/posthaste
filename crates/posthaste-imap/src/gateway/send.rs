use super::*;
use posthaste_call_policy::SEND_TOTAL;
use posthaste_domain_model::SendFiling;

use crate::smtp::smtp_stable_message_id;

pub(crate) async fn send_message_via_smtp(
    gateway: &LiveImapSmtpGateway,
    smtp_config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
    idempotency_key: &str,
) -> Result<SendFiling, GatewayError> {
    // SMTP has no submission idempotency token; a stable Message-ID is the only
    // dedup hook (D85, best-effort). The send is bounded by the send-class
    // deadline: a timeout may leave the message already transmitted to the MTA,
    // so it is classified dispatch-uncertain — parked, never blind-resent (D86;
    // O5: at-most-once-on-uncertainty is the accepted SMTP contract).
    let message_id = smtp_stable_message_id(idempotency_key, smtp_config);
    let submitted = match tokio::time::timeout(
        SEND_TOTAL,
        submit_smtp_message(smtp_config, request, Some(&message_id)),
    )
    .await
    {
        Ok(result) => result.map_err(imap_error_to_gateway)?,
        Err(_elapsed) => {
            return Err(GatewayError::DispatchUncertain(
                "SMTP send timed out; delivery uncertain".to_string(),
            ))
        }
    };

    if smtp_sent_copy_strategy(&smtp_config.provider) != SmtpSentCopyStrategy::AppendToSentMailbox {
        // The provider files sent mail itself (e.g. Gmail SMTP duplicates
        // into Sent): filing is the provider's confirmed behavior.
        return Ok(SendFiling::Filed);
    }
    let Some(sent_mailbox) = gateway
        .discovery
        .mailboxes
        .iter()
        .find(|mailbox| mailbox.selectable && mailbox.role == Some("sent"))
    else {
        ph_warn!(
            events::IMAP_SMTP_SENT_MAILBOX_MISSING,
            "SMTP send accepted but no selectable IMAP Sent mailbox was discovered"
        );
        return Ok(SendFiling::PendingFiling);
    };
    let append_result = match gateway.sessions.acquire("append_sent_copy").await {
        Ok(mut lease) => {
            let result =
                append_smtp_sent_copy(lease.client(), &sent_mailbox.name, &submitted.raw_message)
                    .await;
            lease.finish(result)
        }
        Err(error) => Err(error),
    };
    if let Err(error) = append_result {
        // Delivery committed; only the Sent copy is unconfirmed. Typed as
        // PendingFiling (D154) — never a failure (a retry would re-send).
        ph_warn!(
            events::IMAP_SMTP_SENT_APPEND_FAILED,
            mailbox = sent_mailbox.name,
            error = %error,
            "SMTP send accepted but IMAP Sent copy append failed"
        );
        return Ok(SendFiling::PendingFiling);
    }
    Ok(SendFiling::Filed)
}
