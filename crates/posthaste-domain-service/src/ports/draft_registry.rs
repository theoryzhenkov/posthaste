use super::*;

/// Durable authority for a draft's identity: the stable client key
/// (`X-Posthaste-Draft-Id`) and the entity id — temporary before the first
/// flush, provider-assigned after — currently embodying it (D136).
///
/// M68 carved this out of `OperationOutboxStore` as a mechanical extraction;
/// M69 (D135) made the registry AUTHORITATIVE: sync writes through to it in
/// the same transaction as every message upsert/prune, so
/// `resolve_draft_entity` is one lookup against one authority — the D131
/// alias-then-projection fallback is deleted. It is still backed by the
/// `draft_alias` table (rename is M73); the D136 vocabulary rename
/// (resolve/register/rotate/forget) and resolve-at-flush are M70+.
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

    /// Record the entity id a client draft key currently maps to.
    fn set_draft_alias(
        &self,
        account_id: &AccountId,
        draft_key: &str,
        entity_id: &str,
    ) -> Result<(), StoreError>;

    /// Rewrite draft-alias entity ids from a temporary/old id to a newly assigned
    /// provider id, keeping the stable client key pointed at the live draft.
    ///
    /// @spec docs/L1-outbox#temp-id-reconciliation
    fn update_draft_alias_entity(
        &self,
        account_id: &AccountId,
        from_entity_id: &str,
        to_entity_id: &str,
    ) -> Result<(), StoreError>;

    /// Drop a client draft key's alias (after the draft is deleted).
    fn remove_draft_alias(&self, account_id: &AccountId, draft_key: &str)
        -> Result<(), StoreError>;
}

/// Compile-level guarantee that the port is object-safe: `MailService` holds
/// it as `Arc<dyn DraftRegistry>`.
const _: fn(&dyn DraftRegistry) = |_| {};
