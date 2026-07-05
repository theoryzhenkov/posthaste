use super::*;

/// Durable authority for a draft's identity: the stable client key
/// (`X-Posthaste-Draft-Id`) and the entity id — temporary before the first
/// flush, provider-assigned after — currently embodying it (D136).
///
/// M68 carved this out of `OperationOutboxStore` as a mechanical extraction;
/// M69 (D135) made the registry AUTHORITATIVE: sync writes through to it in
/// the same transaction as every message upsert/prune, so
/// `resolve_draft_entity` is one lookup against one authority — the D131
/// alias-then-projection fallback is deleted. M70 (D136) made draft outbox ops
/// carry the STABLE key as their entity id and resolve it through this port at
/// FLUSH time (immediately before the gateway call), and moved the forget to
/// the destroy's settlement / sync-confirmed disappearance — never enqueue.
/// It is still backed by the `draft_alias` table (rename is M73); the D136
/// vocabulary rename (resolve/register/rotate/forget) lands with it.
///
/// @spec docs/eph/RFC-L2-draft-identity#22-d136--one-seam-the-draftregistry-port-resolve-at-flush
pub trait DraftRegistry: Send + Sync {
    /// Resolve a stable client draft key to the entity id currently representing
    /// that draft (a temporary id before its first flush, a provider id after).
    /// Returns `None` for a key never saved in this runtime and never observed
    /// by sync — one lookup against the one authority (M69/D135).
    ///
    /// @spec docs/L1-outbox#temp-id-reconciliation
    fn resolve_draft_entity(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<Option<String>, StoreError>;

    /// Record the entity id a client draft key currently maps to: the birth
    /// self-mapping at save-enqueue, or the repoint to the provider id a flush
    /// assigned (the rotation write — M70's collapse of the pre-M70 dual
    /// outbox+alias rewrite).
    fn set_draft_alias(
        &self,
        account_id: &AccountId,
        draft_key: &str,
        entity_id: &str,
    ) -> Result<(), StoreError>;

    /// Drop a client draft key's mapping. Confirmed destruction ONLY — the
    /// `DraftDelete` settlement or sync-observed disappearance (M69/M70) —
    /// never at enqueue: an in-flight op resolves its key at flush and must
    /// still find the mapping. Idempotent: the settlement forget and the
    /// sync-observed forget may both run; the second is a no-op.
    fn remove_draft_alias(&self, account_id: &AccountId, draft_key: &str)
        -> Result<(), StoreError>;
}

/// Compile-level guarantee that the port is object-safe: `MailService` holds
/// it as `Arc<dyn DraftRegistry>`.
const _: fn(&dyn DraftRegistry) = |_| {};
