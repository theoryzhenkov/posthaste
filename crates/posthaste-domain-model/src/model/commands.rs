use super::*;

/// Command to add and/or remove JMAP keywords on a message.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetKeywordsCommand {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// Command to atomically replace all mailbox memberships for a message.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReplaceMailboxesCommand {
    pub mailbox_ids: Vec<MailboxId>,
}

/// Command to add a message to a single additional mailbox.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddToMailboxCommand {
    pub mailbox_id: MailboxId,
}

/// Command to remove a message from a single mailbox.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RemoveFromMailboxCommand {
    pub mailbox_id: MailboxId,
}

/// Result of a message mutation: updated detail (if applicable) and emitted events.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CommandResult {
    pub detail: Option<MessageDetail>,
    pub events: Vec<DomainEvent>,
}

/// The result of a message **state-assertion command** (set-keywords, mailbox
/// moves, destroy): the domain events it emitted, and nothing else. A command
/// acknowledges its change — it deliberately carries no message detail or body,
/// so archive/delete/keyword ops never serialize the body onto the settlement
/// stream (regression-gated by
/// `message_mutation_settlement_payload_excludes_the_message_body`). Reads are a
/// separate path ([`CommandResult`]/[`MessageDetail`]).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CommandAck {
    pub events: Vec<DomainEvent>,
}

/// Compose-ready content parsed from an existing provider draft.
///
/// Unlike [`MessageDetail`], this preserves all compose recipient fields that
/// are present in the cached raw MIME, including Cc and Bcc.
///
/// @spec docs/L1-outbox#operation-model
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DraftContent {
    pub from: Option<Recipient>,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    /// Stable `X-Posthaste-Draft-Id` for this draft, when present. The client
    /// keys autosave by this so a resumed edit updates the draft in place
    /// instead of creating a new one as the provider id rotates.
    ///
    /// @spec docs/L1-outbox#temp-id-reconciliation
    #[serde(default)]
    pub draft_id: Option<String>,
}

/// Result of loading draft content; includes events emitted by lazy body fetch.
///
/// @spec docs/L1-outbox#operation-model
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftContentResult {
    pub content: DraftContent,
    pub events: Vec<DomainEvent>,
}

/// The message's authoritative state read back after a mutation (the `get` of
/// set+get): present with its provider record, or removed (destroyed).
///
/// Mirrors replica-core's `MessageOutcome` (`Present`/`Removed`) at the
/// provider-record layer, so `Removed` is self-describing rather than an
/// overloaded absence. `MutationOutcome.message` is `None` only when the gateway
/// did not read the message back at all (non-message mutation, or a gateway that
/// does not yet read back).
///
/// @spec docs/eph/DESIGN-L2-optimistic-projection#4-canonical-vocabulary
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
pub enum MessageReadback {
    Present(MessageRecord),
    Removed,
}

/// Server-side outcome of a gateway mutation: the updated sync cursor and, when
/// the gateway read the message back (`set`+`get`), its authoritative state
/// after the change.
///
/// The readback drives optimistic settlement — the runtime overwrites the
/// canonical row with `replay(record, remaining unsettled assertions)`, or
/// removes it on `Removed`.
///
/// @spec docs/L1-sync#conflict-model
/// @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationOutcome {
    pub cursor: Option<SyncCursor>,
    pub message: Option<MessageReadback>,
}

/// JMAP sender identity for an account.
///
/// @spec docs/L1-jmap#core-types
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// Email address with optional display name.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

/// Locally cached sender address that previously passed provider submission.
///
/// @spec docs/L1-compose#sender-selection
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CachedSenderAddress {
    pub source_id: AccountId,
    pub name: Option<String>,
    pub email: String,
    pub last_used_at: String,
}

/// Pre-computed reply/forward metadata fetched from the gateway.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReplyContext {
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    /// The original `To` recipients of the source message. `to` holds the
    /// derived reply recipient (the original `From`); `original_to` lets a
    /// client build a reply-all recipient set (original `From` + `To` + `Cc`,
    /// minus self) without a second fetch.
    pub original_to: Vec<Recipient>,
    pub reply_subject: String,
    pub forward_subject: String,
    pub quoted_body: Option<String>,
    /// Forwarded message block: an attribution header (From/Date/Subject/To)
    /// followed by the original body, unquoted. Used to seed a forward compose.
    pub forwarded_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    /// The original message's `From` recipients, verbatim. `to` holds the
    /// *derived* reply recipient (today also the original `From`, but that
    /// derivation may evolve, e.g. `Reply-To` handling); clients build the
    /// reply attribution line ("On <date> <sender> wrote:") from this field so
    /// it always names the actual sender.
    #[serde(default)]
    pub original_from: Vec<Recipient>,
    /// The original message's date as RFC 3339 (`Date`/sent-at, falling back
    /// to received-at). Clients localize it for the reply attribution line.
    pub original_date: Option<String>,
}

/// Format a recipient list as a header value (`Name <email>, email, ...`).
///
/// Returns `None` when the list is empty so callers can omit the header line.
pub fn recipients_to_header(recipients: &[Recipient]) -> Option<String> {
    if recipients.is_empty() {
        return None;
    }
    Some(
        recipients
            .iter()
            .map(|recipient| match &recipient.name {
                Some(name) if !name.trim().is_empty() => {
                    format!("{} <{}>", name.trim(), recipient.email)
                }
                _ => recipient.email.clone(),
            })
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Build the forwarded-message block seeded into a forward compose body.
///
/// Produces a Gmail-style attribution header followed by the original body. Any
/// header line whose value is empty is omitted; the body is included verbatim.
pub fn format_forwarded_body(
    from: Option<&str>,
    date: Option<&str>,
    subject: Option<&str>,
    to: Option<&str>,
    body: &str,
) -> String {
    let mut block = String::from("---------- Forwarded message ----------\n");
    for (label, value) in [
        ("From", from),
        ("Date", date),
        ("Subject", subject),
        ("To", to),
    ] {
        if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
            block.push_str(&format!("{label}: {value}\n"));
        }
    }
    block.push('\n');
    block.push_str(body);
    block
}

/// File attachment payload for an outgoing compose request.
///
/// The frontend sends base64 content to the daemon; the provider adapter uploads
/// or embeds the bytes using the transport-native attachment path before send.
///
/// @spec docs/L1-compose#attachment-handling
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SendMessageAttachment {
    pub filename: String,
    pub mime_type: String,
    pub content_base64: String,
}

/// Request payload for sending a new email via `EmailSubmission/set`.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SendMessageRequest {
    pub from: Option<Recipient>,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    #[serde(default)]
    pub attachments: Vec<SendMessageAttachment>,
    /// Stable draft identity stamped as `X-Posthaste-Draft-Id` when this request
    /// saves a draft. The domain layer injects it from the draft key before
    /// queuing; `save_draft` writes it as a header so the id survives provider
    /// id rotation. On a `send`, it names the originating draft: the sent
    /// message is still a fresh message (no header is stamped), but the draft
    /// is destroyed as a settlement effect once the send settles success
    /// (D126) — and kept when the send parks `DispatchUncertain` (D125).
    ///
    /// @spec docs/L1-outbox#temp-id-reconciliation
    /// @spec docs/eph/RFC-L2-drafts#3-decisions-proposed
    #[serde(default)]
    pub draft_id: Option<String>,
    /// Earliest submission time (RFC 3339). Absent (or in the past) the send is
    /// due immediately — the pre-existing behavior. When set in the future the
    /// enqueued outbox send is HELD queued until due (undo-send = now + delay;
    /// send-later = the chosen time; one mechanism), then flushed by the
    /// scheduler tick / next flush pass.
    ///
    /// LOCAL-FIRST OFFLINE SEMANTICS: this is not a server-side schedule. The
    /// send fires on the first flush window at/after `send_at` while the app is
    /// running and online; if Posthaste is closed (or offline) at the due time,
    /// the send fires when it is next running + connected. UI copy must say so
    /// (e.g. "Sends when Posthaste is open").
    ///
    /// Normalized at enqueue to UTC whole-second RFC 3339 (`...Z`) so stored
    /// values compare lexicographically; skipped from serialization when absent
    /// so an immediate send's payload stays byte-identical to before.
    ///
    /// @spec docs/L1-outbox#operation-model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::format_forwarded_body;

    #[test]
    fn forwarded_body_includes_attribution_headers_then_body() {
        let block = format_forwarded_body(
            Some("Ada <ada@example.com>"),
            Some("2026-06-29T12:00:00Z"),
            Some("Subject"),
            Some("you@example.com"),
            "original body",
        );
        assert_eq!(
            block,
            "---------- Forwarded message ----------\n\
             From: Ada <ada@example.com>\n\
             Date: 2026-06-29T12:00:00Z\n\
             Subject: Subject\n\
             To: you@example.com\n\
             \n\
             original body"
        );
    }

    #[test]
    fn forwarded_body_omits_absent_or_blank_headers() {
        let block = format_forwarded_body(Some("ada@example.com"), None, Some("   "), None, "body");
        assert_eq!(
            block,
            "---------- Forwarded message ----------\n\
             From: ada@example.com\n\
             \n\
             body"
        );
    }
}
