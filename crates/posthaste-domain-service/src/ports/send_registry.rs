use super::*;

/// Durable authority for a provisional sent message's adoption alias: the
/// mapping from a send op's provisional entity id (`send-<id>`, the
/// overlay-only Sent row) to the real provider id its copy landed under once
/// adoption matched it.
///
/// A `Destroy`/`ReplaceMailboxes`/`SetKeywords` op enqueued against a
/// provisional `send-<id>` cannot be pushed to the provider — `send-<id>` has
/// no IMAP message. The flush resolves the alias to retarget the gateway call
/// to the adopted real id; if the alias is absent it defers (the send is still
/// in flight) or no-ops (the send failed/gone). Adoption sets the alias
/// BEFORE retiring the send op, so the alias is always set before the send op
/// is gone — a flush never observes "alias absent + send op gone" on an
/// adopted send (only on a discarded one).
///
/// Mirrors `DraftRegistry` for drafts. Backed by the `send_alias` table.
/// Aliases linger past the adopted message's destruction (see the table's
/// schema comment) — there is no `remove_send_alias` on this port.
pub trait SendRegistry: Send + Sync {
    /// Resolve a provisional send entity id to the real provider id its copy
    /// was adopted under. `None` when the send has not yet been adopted (the
    /// flush then defers or no-ops based on the send op's state).
    fn resolve_send_alias(
        &self,
        account_id: &AccountId,
        send_entity_id: &str,
    ) -> Result<Option<String>, StoreError>;

    /// Record the adoption alias: `send_entity_id` → `adopted_message_id`.
    /// Called by `adopt_sent_copies` BEFORE retiring the send op, so the alias
    /// is always set before the send op leaves the log. Idempotent: adoption
    /// is once-per-send, so a re-run (a crash retry) overwrites the same
    /// mapping.
    fn set_send_alias(
        &self,
        account_id: &AccountId,
        send_entity_id: &str,
        adopted_message_id: &str,
    ) -> Result<(), StoreError>;
}

/// Compile-level guarantee that the port is object-safe: `MailService` holds
/// it as `Arc<dyn SendRegistry>`.
const _: fn(&dyn SendRegistry) = |_| {};
