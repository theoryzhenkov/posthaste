//! The replica **mechanism** layer of the entity store (RFC D36 layer 1
//! mount): the accept/settle/retire plumbing over replica-core's
//! [`OptimisticReplica`] kernel, plus the JSON bridge between wire
//! presentation projections and the kernel's canonical fold state.
//!
//! No view knowledge lives here — rows, predicates, windowing, sort keys and
//! dirty tracking are the projection layer's ([`crate::projection`]). The
//! version-gated race-free retire itself lives in the kernel
//! (`Replica::retire_absorbed_at`, RFC D9/V7); this layer only reads the
//! authority version out of the held projection and hands it to the seam.

use std::collections::HashMap;

use serde_json::Value;

use posthaste_replica_core::{
    MessageAssertion, MessageFoldState, MessageReplica, MutationId, OptimisticReplica, Outcome,
    PendingMessageMutation, SettlementOutcome, SettlementResult,
};

/// A message entity: the authoritative `MessageSummary` projection (the base the
/// outbox folds over). The *optimistic* projection a renderer reads is computed
/// on read — never stored. Internal (not exported): the base projection must
/// not leak past the store's `message()` accessor, which returns the folded
/// state — exposing it would open a second, non-optimistic derivation path.
#[derive(Clone, Debug)]
pub(crate) struct MessageEntity {
    pub(crate) projection: Value,
}

/// What applying an authoritative base did, so the composition layer
/// ([`crate::EntityStore`]) can drive the matching projection reaction.
pub(crate) enum BaseApplied {
    /// The base was (re)seeded and any absorbed pending retired: rederive the
    /// message's view placement.
    Updated,
    /// Authoritative removal: drop the message's rows from every view.
    Removed,
    /// The staleness guard rejected a strictly-older base: nothing changed.
    RejectedStale,
}

/// The mechanism half of the entity store: the confirmed bases (as wire
/// projections), the convergence kernel, and the retired-op buffer the host
/// drains to clear durable-outbox records.
#[derive(Default)]
pub(crate) struct ReplicaMechanism {
    pub(crate) messages: HashMap<String, MessageEntity>,
    /// The message convergence engine: confirmed fold states + the optimistic
    /// outbox, consumed through the [`OptimisticReplica`] seam. Keyed by
    /// message id (`MessageConvergence::Key = String`).
    pub(crate) engine: MessageReplica,
    /// Ids of ops retired since the last [`drain_retired`](Self::drain_retired)
    /// (at settle confirm or base catch-up). The host clears durable-outbox
    /// records only for these — an un-retired op is still pending in-engine and
    /// must survive a reload. (outbox D)
    pub(crate) retired_buffer: Vec<MutationId>,
}

impl ReplicaMechanism {
    /// Accept an optimistic message mutation into the outbox (idempotent on
    /// mutation id), recording the authority version held at accept time so the
    /// kernel's retire is gated on a strictly-higher version (the equal-version
    /// hold that survives the local move's same-modseq echo + a stale
    /// re-serve).
    pub(crate) fn accept(
        &mut self,
        mutation_id: MutationId,
        message_id: &str,
        assertion: MessageAssertion,
    ) {
        let version = self
            .messages
            .get(message_id)
            .and_then(|entity| authority_version(&entity.projection));
        self.engine.accept_at(
            PendingMessageMutation {
                id: mutation_id,
                key: message_id.to_string(),
                effect: assertion,
            },
            version,
        );
    }

    /// Settle a pending mutation by its terminal outcome. Returns the result
    /// plus the affected message's key (if the op was pending) so the caller
    /// can re-fold just it.
    ///
    /// `Confirmed` does **not** revert: the op is marked confirmed and retired
    /// only if the confirmed base already carries its effect at a strictly
    /// higher authority version (the kernel's version-gated
    /// `retire_absorbed_at`), otherwise it stays folded for the authoritative
    /// `message.updated` to retire. `Failed` retires the op unconditionally and
    /// the projection reverts to authoritative state. Out-of-order safe;
    /// idempotent on an unknown id.
    pub(crate) fn settle(
        &mut self,
        mutation_id: &MutationId,
        outcome: SettlementOutcome,
    ) -> (SettlementResult, Option<String>) {
        // Look up the affected message before settling so we can re-fold just it.
        let key = self
            .engine
            .pending()
            .iter()
            .find(|held| &held.id == mutation_id)
            .map(|held| held.key.clone());
        let result = match outcome {
            SettlementOutcome::Confirmed => {
                self.engine.mark_confirmed(mutation_id);
                let mut retired = false;
                if let Some(message_id) = key.as_ref() {
                    let current = self
                        .messages
                        .get(message_id.as_str())
                        .and_then(|e| authority_version(&e.projection));
                    let retired_ids = self.engine.retire_absorbed_at(message_id, current);
                    retired = !retired_ids.is_empty();
                    self.retired_buffer.extend(retired_ids);
                }
                SettlementResult {
                    retired,
                    reverted: false,
                }
            }
            SettlementOutcome::Failed => {
                let result = self.engine.settle(mutation_id, outcome);
                if result.retired {
                    self.retired_buffer.push(mutation_id.clone());
                }
                result
            }
        };
        (result, key)
    }

    /// Apply one authoritative message update: seed/replace the base (guarded
    /// against strictly-older authority versions), or remove it (purging any
    /// pending optimism on the gone entity), retiring whatever the new base
    /// absorbs through the kernel's version gate.
    pub(crate) fn apply_base(
        &mut self,
        message_id: &str,
        projection: &Value,
        deleted: bool,
    ) -> BaseApplied {
        if deleted {
            self.messages.remove(message_id);
            self.engine.remove_base(&message_id.to_string());
            // Authoritative removal: purge any pending optimism on this entity.
            // It is gone — the op can neither fold into a base nor revert to
            // one — so without this it leaks pending forever (has_pending stuck
            // true; the durable outbox grows unbounded on delete-heavy
            // workloads). settle(Confirmed)'s version-gated retire can't reach
            // a deleted entity (no version for the gate), and unconfirmed ops
            // are never retired there anyway. Scoped to deleted=true — a
            // *never-ingested* entity is not an authoritative removal; its
            // deferred pending must survive to fold on a later ingest.
            self.engine.remove_pending(&message_id.to_string());
            BaseApplied::Removed
        } else {
            // Staleness guard: reject a base whose authority-state version is
            // STRICTLY OLDER than the held one. A late provider re-serve carrying
            // a snapshot that predates the current state (the post-confirm
            // flicker tail) must not clobber a newer confirmed base. Equal
            // versions are idempotent (accepted); absent versions (no provider
            // version yet) skip the guard, so it is inert until the runtime
            // stamps `version` on the projection.
            if let (Some(incoming), Some(held)) = (
                authority_version(projection),
                self.messages
                    .get(message_id)
                    .and_then(|entity| authority_version(&entity.projection)),
            ) {
                if incoming < held {
                    return BaseApplied::RejectedStale;
                }
            }
            self.messages.insert(
                message_id.to_string(),
                MessageEntity {
                    projection: projection.clone(),
                },
            );
            self.engine.set_base(
                message_id.to_string(),
                fold_state_from_projection(projection),
            );
            // A base update retires any pending op the new base now carries
            // (the race-free happy-path retire) — but only at a STRICTLY HIGHER
            // version: an equal-version base (the local move's same-modseq echo,
            // or a stale re-serve) must NOT retire the op, so it stays folded and
            // holds membership through the unconfirmed window.
            let retired_ids = self
                .engine
                .retire_absorbed_at(&message_id.to_string(), authority_version(projection));
            self.retired_buffer.extend(retired_ids);
            BaseApplied::Updated
        }
    }

    /// The optimistic projection for a message: the authoritative base with the
    /// pending outbox folded over its keywords/mailboxes, or `None` if the
    /// message is not held or has been optimistically destroyed.
    pub(crate) fn optimistic_projection(&self, message_id: &str) -> Option<Value> {
        let base = &self.messages.get(message_id)?.projection;
        project_optimistic(&self.engine, message_id, base)
    }

    /// Whether the message's authoritative base has been ingested.
    pub(crate) fn is_held(&self, message_id: &str) -> bool {
        self.messages.contains_key(message_id)
    }

    /// The ordered pending outbox (for re-applying optimistic placements).
    pub(crate) fn pending(&self) -> &[PendingMessageMutation] {
        self.engine.pending()
    }

    /// Whether any optimistic mutation is still pending.
    pub(crate) fn has_pending(&self) -> bool {
        self.engine.has_pending()
    }

    /// Drain the ids of ops retired since the last drain (at settle confirm or
    /// at base catch-up).
    pub(crate) fn drain_retired(&mut self) -> Vec<MutationId> {
        std::mem::take(&mut self.retired_buffer)
    }
}

/// Fold a replica engine's pending state over one entity's base presentation
/// projection — the per-entity optimism read (one projector, RFC D38): the
/// engine folds the key's pending effects over its confirmed base and the
/// folded canonical state is written back into the projection. `None` when the
/// engine holds no base for the key or a pending destroy removed the entity.
///
/// Both near nodes consume this one recipe: the client entity store's
/// `message()` read / view placement, and the runtime's outbox overlay over
/// served mail-list rows (`posthaste-runtime`'s `apply_outbox_overlay`).
pub fn project_optimistic(engine: &MessageReplica, key: &str, base: &Value) -> Option<Value> {
    match engine.project(&key.to_string())? {
        Outcome::Present(state) => Some(apply_fold_to_projection(base.clone(), &state)),
        Outcome::Removed => None,
    }
}

/// The per-message authority-state version of a projection, if present — an
/// opaque, provider-causality-ordered counter (IMAP MODSEQ / JMAP object state,
/// stamped by the runtime). Compared opaquely by [`ReplicaMechanism::apply_base`]'s
/// staleness guard and handed to the kernel's version-gated retire; `None` (no
/// version yet) disables both for that message.
pub(crate) fn authority_version(projection: &Value) -> Option<u64> {
    projection.get("version").and_then(Value::as_u64)
}

/// Read the foldable canonical state (keywords + mailbox ids) out of a row's
/// presentation projection. Absent/!array fields read as empty.
pub fn fold_state_from_projection(projection: &Value) -> MessageFoldState {
    MessageFoldState {
        keywords: string_array(projection.get("keywords")),
        mailbox_ids: string_array(projection.get("mailboxIds")),
    }
}

/// Write the folded canonical state back into a presentation projection,
/// re-deriving the read/flag display flags from the keywords and preserving
/// every other field.
pub fn apply_fold_to_projection(mut projection: Value, state: &MessageFoldState) -> Value {
    if let Value::Object(map) = &mut projection {
        map.insert(
            "keywords".to_string(),
            Value::Array(state.keywords.iter().cloned().map(Value::String).collect()),
        );
        map.insert(
            "mailboxIds".to_string(),
            Value::Array(
                state
                    .mailbox_ids
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        map.insert(
            "isRead".to_string(),
            Value::Bool(state.keywords.iter().any(|keyword| keyword == "$seen")),
        );
        map.insert(
            "isFlagged".to_string(),
            Value::Bool(state.keywords.iter().any(|keyword| keyword == "$flagged")),
        );
    }
    projection
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
