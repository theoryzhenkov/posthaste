use jmap_client::mailbox;
use posthaste_domain_model::{GatewayError, SendFiling, SendMessageRequest};
use posthaste_observability::{events, ph_warn};
use posthaste_provider_call::SEND_TOTAL;

use crate::compose::{recipient_to_address, render_markdown};
use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};
use crate::live_compose::attachments::upload_send_attachments;
use crate::live_compose::identity::fetch_send_identity;

use posthaste_domain_model::send_identity_token;

/// Map a send-dispatch failure to a typed [`GatewayError`]. A send-class
/// timeout — or a response so short/reordered that the submission's fate is
/// unknown (P3) — is **dispatch-uncertain**: the submission may already have
/// committed, so the outbox must park it, never blind-resend (D86). A clean
/// pre-commit transport error keeps its ordinary (retryable) classification.
///
/// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
fn dispatch_uncertain(reason: impl Into<String>) -> GatewayError {
    GatewayError::DispatchUncertain(reason.into())
}

/// Send a message via `Email/set` + `EmailSubmission/set` in a single JMAP request.
///
/// Renders the Markdown body to HTML and constructs a multipart/alternative
/// MIME structure. The server handles Sent folder placement.
///
/// The submission carries a **deterministic** create-id and a stable
/// `Message-ID` derived from `idempotency_key` (the outbox op id), so a
/// re-forward of a send that already committed is deduplicated rather than
/// duplicated (D84/D85). The whole dispatch is bounded by the send-class
/// deadline ([`SEND_TOTAL`]); its expiry is classified dispatch-uncertain
/// (never a blind-retryable transient — the S1 fix), as is a truncated/reordered
/// response (P3).
///
/// The jmap-client fork additionally exposes `if_in_state` for a submission-state
/// precondition; a *meaningful* one requires persisting the pre-attempt
/// submission state across retries, which the Option-A interim (park + surface)
/// does not need — the park is the exactly-once guarantee, and the deterministic
/// create-id + `Message-ID` are the field-proving foundation for the future
/// bounded-auto-retry (D87/O1 Option B).
///
/// Draft consumption (NS2 Slice 4 — gateway-owned): `consume_draft` (the
/// originating draft's live provider id, resolved at flush) is destroyed via
/// an explicit `Email/set` destroy batched into THIS request, as the method
/// after the submission. `onSuccessDestroyEmail` cannot express this — RFC
/// 8621 §7.5 scopes it to EmailSubmission ids (it would destroy the
/// *submitted* Email, and this send submits a freshly created Email; the
/// compose buffer is the source of truth, the server-side draft may be
/// stale). The destroy result is tolerated (notFound = already gone; any
/// other failure = warn + D175 repair) — it can never fail the committed
/// send. On `DispatchUncertain` the send never settles, so the draft is kept
/// (D125).
///
/// @spec docs/L1-compose#mime-structure
/// @spec docs/L1-jmap#methods-used
/// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
pub(crate) async fn send_message(
    gateway: &LiveJmapGateway,
    request_data: &SendMessageRequest,
    consume_draft: Option<&posthaste_domain_model::MessageId>,
    idempotency_key: &str,
) -> Result<SendFiling, GatewayError> {
    let identity = fetch_send_identity(gateway, request_data.from.as_ref()).await?;
    let drafts_mailbox_id = gateway
        .fetch_mailbox_id_by_role(mailbox::Role::Drafts)
        .await?;
    let sent_mailbox_id = gateway
        .fetch_mailbox_id_by_role(mailbox::Role::Sent)
        .await?;
    let html_body = render_markdown(&request_data.body);
    let uploaded_attachments = upload_send_attachments(gateway, &request_data.attachments).await?;

    let token = send_identity_token(idempotency_key);
    // A stable RFC5322 Message-ID so a de-duplicating MTA/server drops a second
    // copy of the same send (D85; JMAP servers keying on it get the same).
    let domain = identity
        .email
        .rsplit_once('@')
        .map(|(_, domain)| domain)
        .unwrap_or("posthaste.local");
    let message_id = format!("{token}@{domain}");
    let email_create_id = format!("{token}-email");
    let submission_create_id = format!("{token}-sub");

    let mut request = gateway.client().build();
    let email_obj = request.set_email().create_with_id(email_create_id.as_str());
    email_obj.message_id([message_id.as_str()]);
    email_obj.mailbox_ids([drafts_mailbox_id.as_str()]);
    // A message you sent is read by you (IMAP/JMAP convention: sent mail carries
    // \Seen). Without it the Sent copy syncs back as unread.
    email_obj.keyword("$seen", true);
    email_obj.from([(identity.name.as_str(), identity.email.as_str())]);
    if !request_data.to.is_empty() {
        email_obj.to(request_data.to.iter().map(recipient_to_address));
    }
    if !request_data.cc.is_empty() {
        email_obj.cc(request_data.cc.iter().map(recipient_to_address));
    }
    if !request_data.bcc.is_empty() {
        email_obj.bcc(request_data.bcc.iter().map(recipient_to_address));
    }
    email_obj.subject(request_data.subject.as_str());
    email_obj.text_body(
        jmap_client::email::EmailBodyPart::new()
            .content_type("text/plain")
            .part_id("text_part"),
    );
    email_obj.body_value("text_part".to_string(), request_data.body.as_str());
    email_obj.html_body(
        jmap_client::email::EmailBodyPart::new()
            .content_type("text/html")
            .part_id("html_part"),
    );
    email_obj.body_value("html_part".to_string(), html_body.as_str());
    if let Some(in_reply_to) = &request_data.in_reply_to {
        email_obj.in_reply_to([in_reply_to.as_str()]);
    }
    if let Some(references) = &request_data.references {
        email_obj.references(references.split_whitespace());
    }
    for attachment in uploaded_attachments {
        email_obj.attachment(
            jmap_client::email::EmailBodyPart::new()
                .blob_id(attachment.blob_id)
                .name(attachment.filename)
                .content_type(attachment.mime_type),
        );
    }

    let submission_set = request.set_email_submission();
    let submission = submission_set.create_with_id(submission_create_id.as_str());
    submission.email_id(format!("#{email_create_id}"));
    submission.identity_id(identity.id.as_str());
    // The `onSuccessUpdateEmail` map is keyed by EMAILSUBMISSION id (RFC 8621
    // §7.5) — a `#creation-id` here references the SUBMISSION created above,
    // and the server applies the patch to the Email that submission names.
    // Keying it by the *Email's* creation id (the pre-fix bug) references no
    // submission, so servers silently ignore the patch and the outgoing copy
    // stays filed in Drafts forever — and, with the deterministic Message-ID
    // (D85), a same-server recipient's ingest then dedups the delivery against
    // that lingering Drafts copy: the send "vanishes" with every response
    // reporting success (the implicit Email/set response carries no error for
    // an unresolvable onSuccess key — it just doesn't update anything).
    submission_set
        .arguments()
        .on_success_update_email(submission_create_id.as_str())
        .mailbox_id(drafts_mailbox_id.as_str(), false)
        .mailbox_id(sent_mailbox_id.as_str(), true);

    // Gateway-owned draft consumption (NS2 Slice 4): destroy the originating
    // draft in the SAME request, as the method AFTER the submission — the
    // submission commits first in JMAP's sequential method processing, so a
    // failed destroy can never fail (or precede) the send. `notFound` and any
    // other destroy error are tolerated below: the draft may already be gone,
    // and a lingering copy is D175-repaired, never a send failure.
    if let Some(consume) = consume_draft {
        request.set_email().destroy([consume.as_str()]);
    }

    // Bound the dispatch by the send-class deadline AND classify any failure by
    // PHASE via `send_request_dispatch`: a transport error at/after the request
    // write (incl. jmap-client's inner request timeout that fires before this
    // outer guard, or a mid-response reset) is dispatch-uncertain, so the outbox
    // parks it rather than blind-resending it into a duplicate delivery
    // (DP-C5/C6). Only a provably pre-write connect failure stays retryable, so a
    // genuinely offline send still auto-retries. The outer timeout remains as a
    // wall-clock backstop, also classified uncertain.
    let response =
        match tokio::time::timeout(SEND_TOTAL, gateway.send_request_dispatch(request)).await {
            Ok(result) => result?,
            Err(_elapsed) => return Err(dispatch_uncertain("send timed out; delivery uncertain")),
        };
    let mut responses = response.unwrap_method_responses();
    // A submission whose response is missing/truncated (P3: a server that
    // reorders or omits a method response) leaves the send's fate unknown — park
    // it rather than let the truncation feed a blind resend.
    let mut next_response = |label: &str| {
        required_method_response((!responses.is_empty()).then(|| responses.remove(0)), label)
            .map_err(|_| dispatch_uncertain(format!("send response truncated: missing {label}")))
    };
    let mut email_set = next_response("Email/set create")?
        .unwrap_set_email()
        .map_err(map_gateway_error)?;
    email_set
        .created(&email_create_id)
        .map_err(map_gateway_error)?;

    let mut submission_set = next_response("EmailSubmission/set create")?
        .unwrap_set_email_submission()
        .map_err(map_gateway_error)?;
    submission_set
        .created(&submission_create_id)
        .map_err(map_gateway_error)?;

    let sent_update = next_response("Email/set sent update")?
        .unwrap_set_email()
        .map_err(map_gateway_error)?;
    let moved_to_sent = sent_update.has_updated();
    sent_update
        .unwrap_update_errors()
        .map_err(map_gateway_error)?;
    // The batched draft-consume destroy (when requested): the submission has
    // already committed by the time this response is read, so nothing here
    // may fail the send. A missing/failed destroy (notFound = already gone;
    // anything else = the draft lingers until D175 repairs it) only warns.
    if let Some(consume) = consume_draft {
        let destroy_applied = (!responses.is_empty())
            .then(|| responses.remove(0))
            .and_then(|response| response.unwrap_set_email().ok())
            .map(|mut destroy_set| destroy_set.destroyed(consume.as_str()).is_ok())
            .unwrap_or(false);
        if !destroy_applied {
            ph_warn!(
                events::SEND_DRAFT_CONSUME_NOT_APPLIED,
                draft_id = %consume,
                "send committed but the batched draft consume did not apply \
                 (already gone, or lingering until repair)"
            );
        }
    }
    // A sent-update the server neither applied nor rejected (an unresolvable
    // `onSuccessUpdateEmail` reference — the exact silent no-op the pre-fix
    // wrong-key bug produced): the submission has already committed, so this is
    // NOT a send failure and must not fail the op (a user retry of a delivered
    // send risks a duplicate on servers without create-id dedup). It IS a
    // typed outcome (D154): `PendingFiling` — the settlement carries it, the
    // provisional Sent overlay row stays confirmation-gated, and the log line
    // keeps the regression class visible.
    if !moved_to_sent {
        ph_warn!(
            events::SEND_SENT_MOVE_NOT_APPLIED,
            message_id = %message_id,
            "send submitted but the server did not apply the Drafts→Sent move"
        );
        return Ok(SendFiling::PendingFiling);
    }
    Ok(SendFiling::Filed)
}
